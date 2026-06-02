from __future__ import annotations

import subprocess
from pathlib import Path

from cobol_converter.oracle import (
    compare_generated_project_to_golden,
    record_golden_output,
)


def test_record_golden_output_writes_stdout_and_metadata(tmp_path: Path) -> None:
    source = tmp_path / "hello.cbl"
    source.write_text(
        "IDENTIFICATION DIVISION.\n"
        "PROGRAM-ID. HELLO.\n"
        "PROCEDURE DIVISION.\n"
        "MAIN.\n"
        "DISPLAY \"HELLO\".\n"
        "STOP RUN.\n",
        encoding="utf-8",
    )
    golden = tmp_path / "golden"

    def runner(command: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        if command[0] == "cobc":
            return subprocess.CompletedProcess(command, 0, "", "")
        return subprocess.CompletedProcess(command, 0, "HELLO\n", "")

    result = record_golden_output(
        source,
        golden,
        name="hello",
        dialect="gnucobol",
        source_format="free",
        runner=runner,
    )

    assert result["passed"] is True
    assert (golden / "hello.gnucobol.stdout").read_text(encoding="utf-8") == "HELLO\n"
    metadata = (golden / "hello.gnucobol.json").read_text(encoding="utf-8")
    assert '"source":' in metadata
    assert '"source_format": "free"' in metadata


def test_compare_generated_project_to_golden_reports_match(tmp_path: Path) -> None:
    project = tmp_path / "generated"
    project.mkdir()
    golden = tmp_path / "hello.gnucobol.stdout"
    golden.write_text("HELLO\n", encoding="utf-8")

    def runner(command: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        assert command[:3] == ["cargo", "run", "--offline"]
        assert cwd == project
        return subprocess.CompletedProcess(command, 0, "HELLO\n", "")

    result = compare_generated_project_to_golden(project, golden, runner=runner)

    assert result["matched"] is True
    assert result["actual"] == "HELLO\n"
    assert result["expected"] == "HELLO\n"
