from __future__ import annotations

from pathlib import Path

from .oracle import CommandRunner, compare_generated_project_to_golden


def assert_generated_project_matches_golden(
    project_dir: str | Path,
    golden_stdout: str | Path,
    *,
    args: tuple[str, ...] = (),
    offline: bool = True,
    runner: CommandRunner | None = None,
) -> None:
    """Pytest helper for asserting a generated Rust project matches recorded stdout."""
    compare_kwargs = {"args": args, "offline": offline}
    if runner is not None:
        compare_kwargs["runner"] = runner
    result = compare_generated_project_to_golden(
        project_dir,
        golden_stdout,
        **compare_kwargs,
    )
    assert result["matched"], (
        "generated project stdout did not match golden output\n"
        f"project: {result['project_dir']}\n"
        f"golden: {result['expected_path']}\n"
        f"returncode: {result['returncode']}\n"
        f"expected:\n{result['expected']}\n"
        f"actual:\n{result['actual']}\n"
        f"stderr:\n{result['stderr']}"
    )
