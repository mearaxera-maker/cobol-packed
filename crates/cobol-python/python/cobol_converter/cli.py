from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import importlib.util
from pathlib import Path

import click

import cobol_converter as cobol2rust

from .advisor import build_codebase_refactoring_report, build_refactoring_advice
from .dashboard import build_oracle_dashboard, load_report_files
from .graph_viewer import build_dependency_graph_html
from .migration_project import (
    build_file_map,
    load_migration_config,
    project_output_dir,
    write_generated_file_map,
)
from .oracle import compare_generated_project_to_golden, record_golden_output, run_oracle_suite

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - used on Python 3.8-3.10
    import tomli as tomllib


COPYBOOK_SUFFIXES = frozenset({"", ".cbl", ".cob", ".cobol", ".copy", ".cpy"})
DEFAULT_COPYBOOK_MAX_BYTES = 4 * 1024 * 1024


def _copybook_skip(root: Path, path: Path, reason: str) -> None:
    try:
        name = path.relative_to(root)
    except ValueError:
        name = path
    click.echo(f"skipped copybook file {name}: {reason}", err=True)


def _read_copybooks(
    copybook_dir: tuple[str, ...],
    *,
    include_all_files: bool = False,
    max_bytes: int = DEFAULT_COPYBOOK_MAX_BYTES,
) -> dict[str, str]:
    if max_bytes < 0:
        raise click.ClickException("--copybook-max-bytes must be zero or greater")
    copybooks: dict[str, str] = {}
    for root_text in copybook_dir:
        root = Path(root_text)
        for path in root.rglob("*"):
            if path.is_symlink():
                _copybook_skip(root, path, "symlinks are skipped")
                continue
            if not path.is_file():
                continue
            if not include_all_files and path.suffix.lower() not in COPYBOOK_SUFFIXES:
                _copybook_skip(root, path, "unsupported extension")
                continue
            try:
                size = path.stat().st_size
            except OSError as error:
                _copybook_skip(root, path, f"could not inspect file: {error}")
                continue
            if max_bytes and size > max_bytes:
                _copybook_skip(root, path, f"exceeds --copybook-max-bytes ({max_bytes})")
                continue
            try:
                copybooks[str(path.relative_to(root))] = path.read_text(encoding="utf-8")
            except UnicodeDecodeError as error:
                _copybook_skip(root, path, f"not valid UTF-8 text: {error}")
    return copybooks


def _load_config(config: str | None) -> dict:
    if not config:
        return {}
    path = Path(config)
    if not path.exists():
        raise click.ClickException(f"config file does not exist: {path}")
    return tomllib.loads(path.read_text(encoding="utf-8"))


def _write_text_output(path_text: str | Path, text: str, *, force: bool = False) -> None:
    path = Path(path_text)
    if path.exists() and not force:
        raise click.ClickException(
            f"output file already exists: {path}; pass --force to overwrite"
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temp_path_text = tempfile.mkstemp(
        prefix=f".{path.name}.",
        suffix=".tmp",
        dir=path.parent,
        text=True,
    )
    temp_path = Path(temp_path_text)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(text)
        os.replace(temp_path, path)
    except Exception:
        try:
            temp_path.unlink()
        except FileNotFoundError:
            pass
        raise


def _write_json_output(path_text: str | Path, payload: dict | list, *, force: bool = False) -> None:
    _write_text_output(path_text, json.dumps(payload, indent=2), force=force)


def _converter_config(config: dict) -> dict:
    return config.get("converter", config)


def _configured_copybook_dirs(config_path: str | None, config: dict) -> tuple[str, ...]:
    dirs = tuple(config.get("copybook_dirs", []))
    if not config_path:
        return dirs
    root = Path(config_path).parent
    return tuple(
        str(path if path.is_absolute() else root / path)
        for path in (Path(value) for value in dirs)
    )


def _source_files(source_dir: Path) -> dict[str, str]:
    suffixes = {".cbl", ".cob", ".cobol", ".CBL", ".COB", ".COBOL"}
    return {
        str(path.relative_to(source_dir)): path.read_text(encoding="utf-8")
        for path in sorted(source_dir.rglob("*"))
        if path.is_file() and path.suffix in suffixes
    }


def _print_diagnostics(diagnostics_json: str) -> None:
    diagnostics = json.loads(diagnostics_json)
    for diagnostic in diagnostics:
        location = ""
        if diagnostic.get("file"):
            location = diagnostic["file"]
            if diagnostic.get("line") is not None:
                location += f":{diagnostic['line']}"
        prefix = f"{diagnostic.get('severity', 'Error')} {diagnostic.get('code', 'E_UNKNOWN')}"
        if location:
            prefix += f" at {location}"
        click.secho(prefix, fg="red" if diagnostic.get("severity") == "Error" else "yellow")
        click.echo(f"  {diagnostic.get('message', '')}")


def _tool_status(name: str, module: str | None = None) -> dict:
    path = shutil.which(name)
    if path is not None:
        return {"available": True, "path": path, "source": "path"}
    if module is not None:
        spec = importlib.util.find_spec(module)
        if spec is not None:
            return {"available": True, "path": spec.origin, "source": "python-module"}
    return {"available": False, "path": None, "source": None}


def _doctor_report() -> dict:
    tools = {
        "cargo": _tool_status("cargo"),
        "maturin": _tool_status("maturin", module="maturin"),
        "cobc": _tool_status("cobc"),
        "docker": _tool_status("docker"),
    }
    return {
        "tools": tools,
        "ready": {
            "build_generated_rust": tools["cargo"]["available"],
            "build_python_package": tools["cargo"]["available"]
            and tools["maturin"]["available"],
            "oracle_validation": tools["cobc"]["available"],
            "docker_sample": tools["docker"]["available"],
        },
        "docker": (
            "docker build -f docker/python-toolkit/Dockerfile "
            "-t cobol-converter-python:local . && "
            "docker run --rm -it cobol-converter-python:local cobol2rust-run-sample"
        ),
    }


@click.group()
def main() -> None:
    """COBOL-to-Rust migration tools."""


@main.command()
@click.option("--json-output", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def doctor(json_output: str | None, force: bool = False) -> None:
    """Check local tools needed for conversion, packaging, and oracle workflows."""
    report = _doctor_report()
    if json_output:
        _write_json_output(json_output, report, force=force)
    for name, status in report["tools"].items():
        color = "green" if status["available"] else "yellow"
        location = status["path"] or "not found"
        click.secho(f"{name}: {location}", fg=color)
    if not report["ready"]["oracle_validation"] and report["ready"]["docker_sample"]:
        click.echo("GnuCOBOL is not on PATH; use the Docker sample image for oracle workflows.")
        click.echo(report["docker"])


@main.command()
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--dialect", default="ibm", show_default=True)
@click.option("--source-format", default="auto", show_default=True)
@click.option("--copybook-dir", multiple=True, type=click.Path(exists=True, file_okay=False))
@click.option(
    "--copybook-all-files",
    is_flag=True,
    help="Read every regular file under copybook roots instead of filtering by copybook suffix.",
)
@click.option(
    "--copybook-max-bytes",
    default=DEFAULT_COPYBOOK_MAX_BYTES,
    show_default=True,
    type=int,
    help="Skip copybook files larger than this many bytes; use 0 to disable.",
)
@click.option("--output-dir", required=True, type=click.Path(file_okay=False))
@click.option("--config", type=click.Path(dir_okay=False))
@click.option("--json-output", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def convert(
    input: str,
    dialect: str,
    source_format: str,
    copybook_dir: tuple[str, ...],
    output_dir: str,
    config: str | None,
    json_output: str | None,
    force: bool = False,
    copybook_all_files: bool = False,
    copybook_max_bytes: int = DEFAULT_COPYBOOK_MAX_BYTES,
) -> None:
    """Convert a COBOL source file and write a complete generated Rust project."""
    loaded = _converter_config(_load_config(config))
    dialect = loaded.get("dialect", dialect)
    source_format = loaded.get("source_format", source_format)
    configured_dirs = _configured_copybook_dirs(config, loaded)
    copybook_dir = copybook_dir + configured_dirs
    source = Path(input).read_text(encoding="utf-8")
    copybooks = _read_copybooks(
        copybook_dir,
        include_all_files=copybook_all_files,
        max_bytes=copybook_max_bytes,
    )
    result = json.loads(cobol2rust.convert_project(
        source,
        dialect,
        output_dir,
        {"source_format": source_format, "copybooks": copybooks},
    ))
    if json_output:
        _write_json_output(json_output, result, force=force)
    if result.get("diagnostics"):
        _print_diagnostics(json.dumps(result["diagnostics"]))
        raise click.ClickException("conversion failed")

    click.secho(f"generated Rust project: {result['out_dir']}", fg="green")
    click.echo(f"migration report: {result['report_path']}")


@main.command()
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--source-format", default="auto", show_default=True)
@click.option("--copybook-dir", multiple=True, type=click.Path(exists=True, file_okay=False))
@click.option(
    "--copybook-all-files",
    is_flag=True,
    help="Read every regular file under copybook roots instead of filtering by copybook suffix.",
)
@click.option(
    "--copybook-max-bytes",
    default=DEFAULT_COPYBOOK_MAX_BYTES,
    show_default=True,
    type=int,
    help="Skip copybook files larger than this many bytes; use 0 to disable.",
)
@click.option("--config", type=click.Path(dir_okay=False))
@click.option("--output", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def preprocess(
    input: str,
    source_format: str,
    copybook_dir: tuple[str, ...],
    config: str | None,
    output: str | None,
    force: bool = False,
    copybook_all_files: bool = False,
    copybook_max_bytes: int = DEFAULT_COPYBOOK_MAX_BYTES,
) -> None:
    """Expand COPY members and write or print preprocessed COBOL source."""
    loaded = _converter_config(_load_config(config))
    source_format = loaded.get("source_format", source_format)
    configured_dirs = _configured_copybook_dirs(config, loaded)
    copybook_dir = copybook_dir + configured_dirs
    source = Path(input).read_text(encoding="utf-8")
    copybooks = _read_copybooks(
        copybook_dir,
        include_all_files=copybook_all_files,
        max_bytes=copybook_max_bytes,
    )
    expanded = cobol2rust.preprocess(
        source,
        copybooks,
        source_format=source_format,
    )
    if output:
        _write_text_output(output, expanded, force=force)
        click.secho(f"wrote preprocessed source: {output}", fg="green")
        return
    click.echo(expanded)


@main.command()
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--dialect", default="ibm", show_default=True)
@click.option("--source-format", default="auto", show_default=True)
@click.option("--copybook-dir", multiple=True, type=click.Path(exists=True, file_okay=False))
@click.option(
    "--copybook-all-files",
    is_flag=True,
    help="Read every regular file under copybook roots instead of filtering by copybook suffix.",
)
@click.option(
    "--copybook-max-bytes",
    default=DEFAULT_COPYBOOK_MAX_BYTES,
    show_default=True,
    type=int,
    help="Skip copybook files larger than this many bytes; use 0 to disable.",
)
@click.option("--strict", is_flag=True)
@click.option("--config", type=click.Path(dir_okay=False))
@click.option("--json-output", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def check(
    input: str,
    dialect: str,
    source_format: str,
    copybook_dir: tuple[str, ...],
    strict: bool,
    config: str | None,
    json_output: str | None,
    force: bool = False,
    copybook_all_files: bool = False,
    copybook_max_bytes: int = DEFAULT_COPYBOOK_MAX_BYTES,
) -> None:
    """Run converter validation and print diagnostics."""
    loaded = _converter_config(_load_config(config))
    dialect = loaded.get("dialect", dialect)
    source_format = loaded.get("source_format", source_format)
    configured_dirs = _configured_copybook_dirs(config, loaded)
    copybook_dir = copybook_dir + configured_dirs
    source = Path(input).read_text(encoding="utf-8")
    copybooks = _read_copybooks(
        copybook_dir,
        include_all_files=copybook_all_files,
        max_bytes=copybook_max_bytes,
    )
    result = cobol2rust.check_cobol(
        source,
        dialect,
        {"source_format": source_format, "copybooks": copybooks},
    )
    if json_output:
        _write_json_output(json_output, result, force=force)
    if result["ok"]:
        click.secho("no blocking diagnostics", fg="green")
        return

    _print_diagnostics(result["diagnostics_json"])
    if strict:
        raise click.ClickException("strict check failed")


@main.group()
def batch() -> None:
    """Batch migration tools."""


@batch.command("advisor")
@click.option("--source-dir", required=True, type=click.Path(exists=True, file_okay=False))
@click.option("--summary", required=True, type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
@click.option("--progress/--no-progress", default=True, show_default=True)
@click.option("--strict", is_flag=True, help="Fail when advisory findings are present.")
def batch_advisor(
    source_dir: str,
    summary: str,
    progress: bool,
    strict: bool = False,
    force: bool = False,
) -> None:
    """Scan a COBOL source tree and write a refactoring-advice summary."""
    source_root = Path(source_dir)
    sources = _source_files(source_root)
    if not sources:
        raise click.ClickException(f"no COBOL sources found under {source_dir}")
    items = list(sources.items())

    if progress:
        with click.progressbar(
            items,
            label="Advising COBOL sources",
            show_pos=True,
            item_show_func=lambda item: item[0] if item else "",
        ) as bar:
            sources = {path: source for path, source in bar}

    report = build_codebase_refactoring_report(
        sources,
        analyzer=lambda path, source: cobol2rust.analyze_source(path, source),
    )
    _write_json_output(summary, report, force=force)
    click.secho(
        f"advisor: {report['total_findings']} findings across "
        f"{report['files_with_findings']}/{report['total_files']} files",
        fg="green" if report["total_findings"] == 0 else "yellow",
    )
    if strict and report["total_findings"]:
        raise click.ClickException("batch advisor found refactoring findings")


@batch.command("check")
@click.option("--source-dir", required=True, type=click.Path(exists=True, file_okay=False))
@click.option("--config", type=click.Path(dir_okay=False))
@click.option("--dialect", default="ibm", show_default=True)
@click.option("--source-format", default="auto", show_default=True)
@click.option("--copybook-dir", multiple=True, type=click.Path(exists=True, file_okay=False))
@click.option(
    "--copybook-all-files",
    is_flag=True,
    help="Read every regular file under copybook roots instead of filtering by copybook suffix.",
)
@click.option(
    "--copybook-max-bytes",
    default=DEFAULT_COPYBOOK_MAX_BYTES,
    show_default=True,
    type=int,
    help="Skip copybook files larger than this many bytes; use 0 to disable.",
)
@click.option("--summary", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
@click.option("--progress/--no-progress", default=True, show_default=True)
@click.option("--strict", is_flag=True)
def batch_check(
    source_dir: str,
    config: str | None,
    dialect: str,
    source_format: str,
    copybook_dir: tuple[str, ...],
    summary: str | None,
    progress: bool,
    strict: bool,
    force: bool = False,
    copybook_all_files: bool = False,
    copybook_max_bytes: int = DEFAULT_COPYBOOK_MAX_BYTES,
) -> None:
    """Validate every COBOL source under a directory and write a summary."""
    loaded_config = load_migration_config(config) if config else None
    loaded = loaded_config.converter if loaded_config else _converter_config(_load_config(config))
    dialect = loaded.get("dialect", dialect)
    source_format = loaded.get("source_format", source_format)
    configured_dirs = _configured_copybook_dirs(config, loaded)
    copybook_dir = copybook_dir + configured_dirs
    source_root = Path(source_dir)
    sources = _source_files(source_root)
    if not sources:
        raise click.ClickException(f"no COBOL sources found under {source_dir}")
    copybooks = _read_copybooks(
        copybook_dir,
        include_all_files=copybook_all_files,
        max_bytes=copybook_max_bytes,
    )
    items = list(sources.items())

    def check_items(source_items) -> dict:
        files = []
        for source_name, source_text in source_items:
            try:
                checked = cobol2rust.check_cobol(
                    source_text,
                    dialect,
                    {"source_format": source_format, "copybooks": copybooks},
                )
                diagnostics = checked.get("diagnostics") or []
                files.append(
                    {
                        "input": source_name,
                        "ok": bool(checked.get("ok")) and not diagnostics,
                        "status": "ok" if bool(checked.get("ok")) and not diagnostics else "blocked",
                        "diagnostics": diagnostics,
                    }
                )
            except Exception as error:  # pragma: no cover - defensive CLI reporting
                files.append(
                    {
                        "input": source_name,
                        "ok": False,
                        "status": "failed",
                        "diagnostics": [
                            {
                                "code": "E_BATCH_CHECK",
                                "severity": "Error",
                                "message": str(error),
                                "file": source_name,
                                "line": None,
                                "column": None,
                            }
                        ],
                    }
                )
        return {
            "total": len(files),
            "ok": sum(1 for item in files if item["status"] == "ok"),
            "blocked": sum(1 for item in files if item["status"] == "blocked"),
            "failures": sum(1 for item in files if item["status"] == "failed"),
            "files": files,
        }

    if progress:
        with click.progressbar(
            items,
            label="Checking COBOL sources",
            show_pos=True,
            item_show_func=lambda item: item[0] if item else "",
        ) as bar:
            result = check_items(bar)
    else:
        result = check_items(items)
    if summary:
        _write_json_output(summary, result, force=force)
    click.secho(
        f"batch check: {result['ok']}/{result['total']} ok, "
        f"{result['blocked']} blocked, {result['failures']} failed",
        fg="green" if result["blocked"] == 0 and result["failures"] == 0 else "yellow",
    )
    if strict and (result["blocked"] or result["failures"]):
        raise click.ClickException("batch check found blocked or failed programs")


@batch.command("convert")
@click.option("--source-dir", required=True, type=click.Path(exists=True, file_okay=False))
@click.option("--output-dir", required=True, type=click.Path(file_okay=False))
@click.option("--config", type=click.Path(dir_okay=False))
@click.option("--dialect", default="ibm", show_default=True)
@click.option("--source-format", default="auto", show_default=True)
@click.option("--copybook-dir", multiple=True, type=click.Path(exists=True, file_okay=False))
@click.option(
    "--copybook-all-files",
    is_flag=True,
    help="Read every regular file under copybook roots instead of filtering by copybook suffix.",
)
@click.option(
    "--copybook-max-bytes",
    default=DEFAULT_COPYBOOK_MAX_BYTES,
    show_default=True,
    type=int,
    help="Skip copybook files larger than this many bytes; use 0 to disable.",
)
@click.option("--summary", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
@click.option(
    "--file-map-config",
    type=click.Path(exists=True, dir_okay=False),
    help="TOML config with [file_map] or [[dd]] entries to write as cobol-file-map.json.",
)
@click.option("--progress/--no-progress", default=True, show_default=True)
@click.option(
    "--verify-build",
    is_flag=True,
    help="Run cargo check --offline in each generated project and include results in the summary.",
)
def batch_convert(
    source_dir: str,
    output_dir: str,
    config: str | None,
    dialect: str,
    source_format: str,
    copybook_dir: tuple[str, ...],
    summary: str | None,
    file_map_config: str | None,
    progress: bool,
    verify_build: bool,
    force: bool = False,
    copybook_all_files: bool = False,
    copybook_max_bytes: int = DEFAULT_COPYBOOK_MAX_BYTES,
) -> None:
    """Convert every COBOL source under a directory into project subdirectories."""
    loaded_config = load_migration_config(config) if config else None
    loaded = loaded_config.converter if loaded_config else _converter_config(_load_config(config))
    dialect = loaded.get("dialect", dialect)
    source_format = loaded.get("source_format", source_format)
    configured_dirs = _configured_copybook_dirs(config, loaded)
    copybook_dir = copybook_dir + configured_dirs
    source_root = Path(source_dir)
    sources = _source_files(source_root)
    if not sources:
        raise click.ClickException(f"no COBOL sources found under {source_dir}")
    file_map: dict[str, str] = {}
    if loaded_config:
        file_map.update(
            build_file_map(loaded_config.file_map, source_root=Path(config).parent)
        )
    if file_map_config:
        extra_config = load_migration_config(file_map_config)
        file_map.update(
            build_file_map(extra_config.file_map, source_root=Path(file_map_config).parent)
        )
    copybooks = _read_copybooks(
        copybook_dir,
        include_all_files=copybook_all_files,
        max_bytes=copybook_max_bytes,
    )
    items = list(sources.items())

    def convert_items(source_items) -> dict:
        projects = []
        seen_outputs: dict[Path, str] = {}
        for source_name, source_text in source_items:
            project_dir = project_output_dir(output_dir, source_name)
            previous_input = seen_outputs.get(project_dir)
            if previous_input is not None:
                projects.append(
                    {
                        "input": source_name,
                        "out_dir": str(project_dir),
                        "status": "failed",
                        "diagnostics": [
                            {
                                "code": "E_BATCH_OUTPUT_COLLISION",
                                "severity": "Error",
                                "message": (
                                    "batch output directory collides with "
                                    f"{previous_input}; rename one source or choose distinct output roots"
                                ),
                                "file": source_name,
                                "line": None,
                                "column": None,
                            }
                        ],
                    }
                )
                continue
            seen_outputs[project_dir] = source_name
            try:
                project = json.loads(
                    cobol2rust.convert_project(
                        source_text,
                        dialect,
                        str(project_dir),
                        {"source_format": source_format, "copybooks": copybooks},
                    )
                )
                diagnostics = project.get("diagnostics") or []
                status = "blocked" if diagnostics else "generated"
                entry = {
                    "input": source_name,
                    "out_dir": str(project_dir),
                    "status": status,
                    "diagnostics": diagnostics,
                }
                if status == "generated":
                    file_map_path = write_generated_file_map(project_dir, file_map)
                    if file_map_path is not None:
                        entry["file_map"] = str(file_map_path)
                    if verify_build:
                        entry["build"] = _verify_generated_project_build(project_dir)
                        if not entry["build"]["passed"]:
                            entry["status"] = "failed"
                            entry["diagnostics"] = [
                                {
                                    "code": "E_GENERATED_BUILD",
                                    "severity": "Error",
                                    "message": "generated Rust project failed cargo check",
                                    "file": str(project_dir),
                                    "line": None,
                                    "column": None,
                                }
                            ]
                projects.append(entry)
            except Exception as error:  # pragma: no cover - defensive CLI reporting
                projects.append(
                    {
                        "input": source_name,
                        "out_dir": str(project_dir),
                        "status": "failed",
                        "diagnostics": [
                            {
                                "code": "E_BATCH_CONVERT",
                                "severity": "Error",
                                "message": str(error),
                                "file": source_name,
                                "line": None,
                                "column": None,
                            }
                        ],
                    }
                )
        return {
            "total": len(projects),
            "generated": sum(1 for item in projects if item["status"] == "generated"),
            "blocked": sum(1 for item in projects if item["status"] == "blocked"),
            "failures": sum(1 for item in projects if item["status"] == "failed"),
            "projects": projects,
        }

    if progress:
        with click.progressbar(
            items,
            label="Converting COBOL sources",
            show_pos=True,
            item_show_func=lambda item: item[0] if item else "",
        ) as bar:
            result = convert_items(bar)
    else:
        result = convert_items(items)
    if summary:
        _write_json_output(summary, result, force=force)
    click.secho(
        f"batch: {result['generated']}/{result['total']} generated, "
        f"{result['blocked']} blocked, {result['failures']} failed",
        fg="green" if result["blocked"] == 0 and result["failures"] == 0 else "yellow",
    )
    if result["blocked"] or result["failures"]:
        raise click.ClickException("batch conversion completed with blocked or failed programs")


def _verify_generated_project_build(project_dir: Path) -> dict:
    command = ["cargo", "check", "--offline"]
    completed = subprocess.run(
        command,
        cwd=project_dir,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    return {
        "command": command,
        "returncode": completed.returncode,
        "passed": completed.returncode == 0,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


@main.command()
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--output", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def advisor(input: str, output: str | None, force: bool = False) -> None:
    """Report unsupported migration features and suggested rewrites."""
    path = Path(input)
    analysis = json.loads(cobol2rust.analyze_source(str(path), path.read_text(encoding="utf-8")))
    features = build_refactoring_advice(analysis)
    if output:
        _write_json_output(output, features, force=force)
    if not features:
        click.secho("no advisory findings", fg="green")
        return
    for feature in features:
        heading = feature.get("feature", "feature")
        if feature.get("capability_id") or feature.get("status"):
            heading += f" [{feature.get('capability_id', 'capability')}: {feature.get('status', 'unknown')}]"
        click.secho(heading, fg="yellow", bold=True)
        click.echo(f"  {feature.get('advice', '')}")


@main.group()
def graph() -> None:
    """Dependency graph tools."""


@graph.command("dot")
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--output", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def graph_dot(input: str, output: str | None, force: bool = False) -> None:
    """Write COPY/CALL dependency graph as Graphviz DOT."""
    path = Path(input)
    dot = cobol2rust.dependency_graph_dot(str(path), path.read_text(encoding="utf-8"))
    if output:
        _write_text_output(output, dot, force=force)
        click.secho(f"wrote {output}", fg="green")
    else:
        click.echo(dot)


@graph.command("html")
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--output", required=True, type=click.Path(dir_okay=False))
@click.option("--title", default="COBOL Dependency Graph", show_default=True)
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def graph_html(input: str, output: str, title: str, force: bool = False) -> None:
    """Render COPY/CALL dependencies as a standalone HTML graph viewer."""
    path = Path(input)
    analysis = json.loads(cobol2rust.analyze_source(str(path), path.read_text(encoding="utf-8")))
    html = build_dependency_graph_html(analysis, title=title)
    _write_text_output(output, html, force=force)
    click.secho(f"wrote {output}", fg="green")


@main.group()
def oracle() -> None:
    """Oracle validation tools."""


@oracle.command("run")
@click.option("--repo-root", default=".", type=click.Path(file_okay=False), show_default=True)
@click.option("--json-output", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def oracle_run(repo_root: str, json_output: str | None, force: bool = False) -> None:
    """Run the Rust oracle suite and capture a summary."""
    summary = run_oracle_suite(repo_root)
    if json_output:
        _write_json_output(json_output, summary, force=force)
    click.echo(summary["stdout"])
    if summary["stderr"]:
        click.echo(summary["stderr"], err=True)
    if not summary["passed"]:
        raise click.ClickException("oracle suite failed")


@oracle.command("dashboard")
@click.option(
    "--report",
    "reports",
    multiple=True,
    required=True,
    type=click.Path(exists=True, dir_okay=False),
    help="Oracle or golden-compare JSON report. Repeat for multiple reports.",
)
@click.option("--output", required=True, type=click.Path(dir_okay=False))
@click.option("--title", default="COBOL Oracle Dashboard", show_default=True)
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def oracle_dashboard(
    reports: tuple[str, ...],
    output: str,
    title: str,
    force: bool = False,
) -> None:
    """Render a standalone HTML dashboard from oracle JSON reports."""
    try:
        loaded = load_report_files(list(reports))
        html = build_oracle_dashboard(loaded, title=title)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise click.ClickException(str(error)) from error
    _write_text_output(output, html, force=force)
    click.secho(f"wrote oracle dashboard: {output}", fg="green")


@main.group()
def golden() -> None:
    """Golden-file management for migration regression tests."""


@golden.command("record")
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--golden-dir", default="golden", type=click.Path(file_okay=False), show_default=True)
@click.option("--name")
@click.option("--dialect", default="gnucobol", show_default=True)
@click.option("--source-format", default="free", show_default=True)
@click.option("--cobc", default="cobc", show_default=True)
@click.option("--json-output", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def golden_record(
    input: str,
    golden_dir: str,
    name: str | None,
    dialect: str,
    source_format: str,
    cobc: str,
    json_output: str | None,
    force: bool = False,
) -> None:
    """Record reference stdout from GnuCOBOL."""
    try:
        result = record_golden_output(
            input,
            golden_dir,
            name=name,
            dialect=dialect,
            source_format=source_format,
            cobc=cobc,
        )
    except FileNotFoundError as error:
        raise click.ClickException(
            f"{error.filename} was not found; install GnuCOBOL or set --cobc"
        ) from error
    except ValueError as error:
        raise click.ClickException(str(error)) from error

    if json_output:
        _write_json_output(json_output, result, force=force)
    if not result.get("passed"):
        click.echo(json.dumps(result, indent=2))
        raise click.ClickException(f"golden record failed during {result.get('phase')}")
    click.secho(f"recorded {result['stdout_path']}", fg="green")


@golden.command("compare")
@click.argument("project_dir", type=click.Path(exists=True, file_okay=False))
@click.argument("golden_stdout", type=click.Path(exists=True, dir_okay=False))
@click.option("--arg", "program_args", multiple=True)
@click.option("--online", is_flag=True, help="Allow Cargo network access instead of --offline.")
@click.option("--json-output", type=click.Path(dir_okay=False))
@click.option("--force", is_flag=True, help="Overwrite existing output files.")
def golden_compare(
    project_dir: str,
    golden_stdout: str,
    program_args: tuple[str, ...],
    online: bool,
    json_output: str | None,
    force: bool = False,
) -> None:
    """Compare generated Rust project stdout to a recorded golden file."""
    try:
        result = compare_generated_project_to_golden(
            project_dir,
            golden_stdout,
            args=program_args,
            offline=not online,
        )
    except FileNotFoundError as error:
        raise click.ClickException(f"{error.filename} was not found") from error
    if json_output:
        _write_json_output(json_output, result, force=force)
    if result["matched"]:
        click.secho("golden output matched", fg="green")
        return
    click.echo(json.dumps(result, indent=2))
    raise click.ClickException("golden output mismatch")


@main.command("init-migration")
@click.argument("directory", type=click.Path(file_okay=False))
def init_migration(directory: str) -> None:
    """Scaffold a migration project directory."""
    root = Path(directory)
    for child in ["cobol", "copybooks", "data", "golden", "generated", "reports", "work"]:
        (root / child).mkdir(parents=True, exist_ok=True)
    config = root / "cobol2rust.toml"
    if not config.exists():
        _write_text_output(
            config,
            '[converter]\ndialect = "ibm"\nsource_format = "auto"\ncopybook_dirs = ["copybooks"]\n\n'
            '# Runtime DD/file assignments for generated projects.\n'
            '[file_map]\n'
            'INFILE = "data/input.dat"\n'
            'OUTFILE = "data/output.dat"\n\n'
            '# Equivalent JCL-like form:\n'
            '[[dd]]\n'
            'name = "SORTWK01"\n'
            'path = "work/sortwk01.dat"\n',
        )
    click.secho(f"initialized migration project at {root}", fg="green")
