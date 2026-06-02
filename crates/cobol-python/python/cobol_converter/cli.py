from __future__ import annotations

import json
import subprocess
from pathlib import Path

import click

import cobol2rust

from .dashboard import build_oracle_dashboard, load_report_files
from .oracle import compare_generated_project_to_golden, record_golden_output

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - used on Python 3.8-3.10
    import tomli as tomllib


def _read_copybooks(copybook_dir: tuple[str, ...]) -> dict[str, str]:
    copybooks: dict[str, str] = {}
    for root_text in copybook_dir:
        root = Path(root_text)
        for path in root.rglob("*"):
            if path.is_file():
                copybooks[str(path.relative_to(root))] = path.read_text(encoding="utf-8")
    return copybooks


def _load_config(config: str | None) -> dict:
    if not config:
        return {}
    path = Path(config)
    if not path.exists():
        raise click.ClickException(f"config file does not exist: {path}")
    return tomllib.loads(path.read_text(encoding="utf-8"))


def _converter_config(config: dict) -> dict:
    return config.get("converter", config)


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


@click.group()
def main() -> None:
    """COBOL-to-Rust migration tools."""


@main.command()
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--dialect", default="ibm", show_default=True)
@click.option("--source-format", default="auto", show_default=True)
@click.option("--copybook-dir", multiple=True, type=click.Path(exists=True, file_okay=False))
@click.option("--output-dir", required=True, type=click.Path(file_okay=False))
@click.option("--config", type=click.Path(dir_okay=False))
def convert(
    input: str,
    dialect: str,
    source_format: str,
    copybook_dir: tuple[str, ...],
    output_dir: str,
    config: str | None,
) -> None:
    """Convert a COBOL source file and write a complete generated Rust project."""
    loaded = _converter_config(_load_config(config))
    dialect = loaded.get("dialect", dialect)
    source_format = loaded.get("source_format", source_format)
    configured_dirs = tuple(loaded.get("copybook_dirs", []))
    copybook_dir = copybook_dir + configured_dirs
    source = Path(input).read_text(encoding="utf-8")
    result = json.loads(cobol2rust.convert_project(
        source,
        dialect,
        output_dir,
        {"source_format": source_format, "copybooks": _read_copybooks(copybook_dir)},
    ))
    if result.get("diagnostics"):
        _print_diagnostics(json.dumps(result["diagnostics"]))
        raise click.ClickException("conversion failed")

    click.secho(f"generated Rust project: {result['out_dir']}", fg="green")
    click.echo(f"migration report: {result['report_path']}")


@main.command()
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--dialect", default="ibm", show_default=True)
@click.option("--source-format", default="auto", show_default=True)
@click.option("--copybook-dir", multiple=True, type=click.Path(exists=True, file_okay=False))
@click.option("--strict", is_flag=True)
def check(
    input: str,
    dialect: str,
    source_format: str,
    copybook_dir: tuple[str, ...],
    strict: bool,
) -> None:
    """Run converter validation and print diagnostics."""
    source = Path(input).read_text(encoding="utf-8")
    result = cobol2rust.convert_cobol(
        source,
        dialect,
        {"source_format": source_format, "copybooks": _read_copybooks(copybook_dir)},
    )
    if result["ok"]:
        click.secho("no blocking diagnostics", fg="green")
        return

    _print_diagnostics(result["diagnostics_json"])
    if strict:
        raise click.ClickException("strict check failed")


@main.group()
def batch() -> None:
    """Batch migration tools."""


@batch.command("convert")
@click.option("--source-dir", required=True, type=click.Path(exists=True, file_okay=False))
@click.option("--output-dir", required=True, type=click.Path(file_okay=False))
@click.option("--config", type=click.Path(dir_okay=False))
@click.option("--dialect", default="ibm", show_default=True)
@click.option("--source-format", default="auto", show_default=True)
@click.option("--copybook-dir", multiple=True, type=click.Path(exists=True, file_okay=False))
@click.option("--summary", type=click.Path(dir_okay=False))
def batch_convert(
    source_dir: str,
    output_dir: str,
    config: str | None,
    dialect: str,
    source_format: str,
    copybook_dir: tuple[str, ...],
    summary: str | None,
) -> None:
    """Convert every COBOL source under a directory into project subdirectories."""
    loaded = _converter_config(_load_config(config))
    dialect = loaded.get("dialect", dialect)
    source_format = loaded.get("source_format", source_format)
    configured_dirs = tuple(loaded.get("copybook_dirs", []))
    copybook_dir = copybook_dir + configured_dirs
    sources = _source_files(Path(source_dir))
    if not sources:
        raise click.ClickException(f"no COBOL sources found under {source_dir}")
    result = json.loads(cobol2rust.batch_convert_sources(
        sources,
        dialect,
        output_dir,
        {"source_format": source_format, "copybooks": _read_copybooks(copybook_dir)},
    ))
    if summary:
        Path(summary).write_text(json.dumps(result, indent=2), encoding="utf-8")
    click.secho(
        f"batch: {result['generated']}/{result['total']} generated, {result['blocked']} blocked",
        fg="green" if result["blocked"] == 0 else "yellow",
    )
    if result["blocked"]:
        raise click.ClickException("batch conversion completed with blocked programs")


@main.command()
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--output", type=click.Path(dir_okay=False))
def advisor(input: str, output: str | None) -> None:
    """Report unsupported migration features and suggested rewrites."""
    path = Path(input)
    analysis = json.loads(cobol2rust.analyze_source(str(path), path.read_text(encoding="utf-8")))
    features = analysis.get("unsupported_features", [])
    if output:
        Path(output).write_text(json.dumps(features, indent=2), encoding="utf-8")
    if not features:
        click.secho("no advisory findings", fg="green")
        return
    for feature in features:
        click.secho(feature.get("feature", "feature"), fg="yellow", bold=True)
        click.echo(f"  {feature.get('advice', '')}")


@main.group()
def graph() -> None:
    """Dependency graph tools."""


@graph.command("dot")
@click.argument("input", type=click.Path(exists=True, dir_okay=False))
@click.option("--output", type=click.Path(dir_okay=False))
def graph_dot(input: str, output: str | None) -> None:
    """Write COPY/CALL dependency graph as Graphviz DOT."""
    path = Path(input)
    dot = cobol2rust.dependency_graph_dot(str(path), path.read_text(encoding="utf-8"))
    if output:
        Path(output).write_text(dot, encoding="utf-8")
        click.secho(f"wrote {output}", fg="green")
    else:
        click.echo(dot)


@main.group()
def oracle() -> None:
    """Oracle validation tools."""


@oracle.command("run")
@click.option("--repo-root", default=".", type=click.Path(file_okay=False), show_default=True)
@click.option("--json-output", type=click.Path(dir_okay=False))
def oracle_run(repo_root: str, json_output: str | None) -> None:
    """Run the Rust oracle suite and capture a summary."""
    command = ["cargo", "test", "--features", "converter", "--test", "oracle_gnucobol"]
    completed = subprocess.run(
        command,
        cwd=repo_root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    summary = {
        "command": command,
        "returncode": completed.returncode,
        "passed": completed.returncode == 0,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }
    if json_output:
        Path(json_output).write_text(json.dumps(summary, indent=2), encoding="utf-8")
    click.echo(completed.stdout)
    if completed.stderr:
        click.echo(completed.stderr, err=True)
    if completed.returncode != 0:
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
def oracle_dashboard(reports: tuple[str, ...], output: str, title: str) -> None:
    """Render a standalone HTML dashboard from oracle JSON reports."""
    try:
        loaded = load_report_files(list(reports))
        html = build_oracle_dashboard(loaded, title=title)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise click.ClickException(str(error)) from error
    Path(output).parent.mkdir(parents=True, exist_ok=True)
    Path(output).write_text(html, encoding="utf-8")
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
def golden_record(
    input: str,
    golden_dir: str,
    name: str | None,
    dialect: str,
    source_format: str,
    cobc: str,
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
def golden_compare(
    project_dir: str,
    golden_stdout: str,
    program_args: tuple[str, ...],
    online: bool,
    json_output: str | None,
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
        Path(json_output).write_text(json.dumps(result, indent=2), encoding="utf-8")
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
    for child in ["cobol", "copybooks", "golden", "generated", "reports"]:
        (root / child).mkdir(parents=True, exist_ok=True)
    config = root / "cobol2rust.toml"
    if not config.exists():
        config.write_text(
            '[converter]\ndialect = "ibm"\nsource_format = "auto"\ncopybook_dirs = ["copybooks"]\n',
            encoding="utf-8",
        )
    click.secho(f"initialized migration project at {root}", fg="green")
