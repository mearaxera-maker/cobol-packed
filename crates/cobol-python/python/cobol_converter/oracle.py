from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Callable

CommandRunner = Callable[[list[str], Path | None], subprocess.CompletedProcess[str]]


def _default_runner(command: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def _source_format_flag(source_format: str) -> str:
    normalized = source_format.lower().replace("_", "-")
    if normalized in {"free", "free-form"}:
        return "-free"
    if normalized in {"fixed", "fixed-form"}:
        return "-fixed"
    if normalized == "auto":
        return "-free"
    raise ValueError(f"unsupported GnuCOBOL source format: {source_format}")


def _safe_name(path: Path, name: str | None) -> str:
    raw = name or path.stem
    out = "".join(ch.lower() if ch.isalnum() else "-" for ch in raw)
    while "--" in out:
        out = out.replace("--", "-")
    out = out.strip("-")
    return out or "program"


def record_golden_output(
    input_path: str | Path,
    golden_dir: str | Path,
    *,
    name: str | None = None,
    dialect: str = "gnucobol",
    source_format: str = "free",
    cobc: str = "cobc",
    runner: CommandRunner = _default_runner,
) -> dict:
    """Compile and run a COBOL file with GnuCOBOL, then persist stdout plus metadata."""
    source = Path(input_path)
    target_dir = Path(golden_dir)
    target_dir.mkdir(parents=True, exist_ok=True)
    record_name = _safe_name(source, name)
    stdout_path = target_dir / f"{record_name}.{dialect}.stdout"
    metadata_path = target_dir / f"{record_name}.{dialect}.json"

    temp = Path(tempfile.mkdtemp(prefix="cobol2rust-golden-", dir=target_dir))
    try:
        exe = temp / ("program.exe" if _is_windows() else "program")
        compile_command = [
            cobc,
            "-x",
            _source_format_flag(source_format),
            "-o",
            str(exe),
            str(source),
        ]
        compile_result = runner(compile_command, temp)
        if compile_result.returncode != 0:
            metadata = {
                "passed": False,
                "phase": "compile",
                "source": str(source),
                "dialect": dialect,
                "source_format": source_format,
                "command": compile_command,
                "returncode": compile_result.returncode,
                "stdout": compile_result.stdout,
                "stderr": compile_result.stderr,
            }
            metadata_path.write_text(json.dumps(metadata, indent=2), encoding="utf-8")
            return metadata

        run_command = [str(exe)]
        run_result = runner(run_command, temp)
        stdout_path.write_text(run_result.stdout, encoding="utf-8")
        metadata = {
            "passed": run_result.returncode == 0,
            "phase": "run",
            "source": str(source),
            "dialect": dialect,
            "source_format": source_format,
            "stdout_path": str(stdout_path),
            "metadata_path": str(metadata_path),
            "compile_command": compile_command,
            "run_command": run_command,
            "returncode": run_result.returncode,
            "stderr": run_result.stderr,
        }
        metadata_path.write_text(json.dumps(metadata, indent=2), encoding="utf-8")
        return metadata
    finally:
        shutil.rmtree(temp, ignore_errors=True)


def run_generated_project(
    project_dir: str | Path,
    *,
    args: tuple[str, ...] = (),
    offline: bool = True,
    runner: CommandRunner = _default_runner,
) -> dict:
    """Run a generated Rust project and return captured process output."""
    project = Path(project_dir)
    command = ["cargo", "run"]
    if offline:
        command.append("--offline")
    if args:
        command.extend(["--", *args])
    completed = runner(command, project)
    return {
        "command": command,
        "project_dir": str(project),
        "returncode": completed.returncode,
        "passed": completed.returncode == 0,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def compare_generated_project_to_golden(
    project_dir: str | Path,
    golden_stdout: str | Path,
    *,
    args: tuple[str, ...] = (),
    offline: bool = True,
    runner: CommandRunner = _default_runner,
) -> dict:
    """Run a generated project and compare its stdout to a recorded golden file."""
    expected_path = Path(golden_stdout)
    expected = expected_path.read_text(encoding="utf-8")
    actual = run_generated_project(project_dir, args=args, offline=offline, runner=runner)
    matched = actual["passed"] and actual["stdout"] == expected
    return {
        "matched": matched,
        "expected_path": str(expected_path),
        "expected": expected,
        "actual": actual["stdout"],
        "stderr": actual["stderr"],
        "returncode": actual["returncode"],
        "command": actual["command"],
        "project_dir": actual["project_dir"],
    }


def _is_windows() -> bool:
    return os.name == "nt"
