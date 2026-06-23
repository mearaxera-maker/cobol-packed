from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from cobol_converter.pytest_helpers import assert_generated_project_matches_golden


def test_assert_generated_project_matches_golden_accepts_runner(tmp_path: Path) -> None:
    project = tmp_path / "generated"
    project.mkdir()
    golden = tmp_path / "program.gnucobol.stdout"
    golden.write_text("OK\n", encoding="utf-8")
    calls: list[tuple[list[str], Path | None]] = []

    def runner(command: list[str], cwd: Path | None) -> subprocess.CompletedProcess[str]:
        calls.append((command, cwd))
        return subprocess.CompletedProcess(command, 0, stdout="OK\n", stderr="")

    assert_generated_project_matches_golden(project, golden, runner=runner)

    assert calls == [(["cargo", "run", "--offline"], project)]


def test_assert_generated_project_matches_golden_reports_mismatch(
    tmp_path: Path,
) -> None:
    project = tmp_path / "generated"
    project.mkdir()
    golden = tmp_path / "program.gnucobol.stdout"
    golden.write_text("EXPECTED\n", encoding="utf-8")

    def runner(command: list[str], cwd: Path | None) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(command, 0, stdout="ACTUAL\n", stderr="trace\n")

    with pytest.raises(AssertionError) as error:
        assert_generated_project_matches_golden(project, golden, runner=runner)

    message = str(error.value)
    assert "generated project stdout did not match golden output" in message
    assert "EXPECTED" in message
    assert "ACTUAL" in message
    assert "trace" in message
