from __future__ import annotations

from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - used on Python 3.8-3.10
    import tomli as tomllib


PACKAGE_ROOT = Path(__file__).resolve().parents[2]
REPO_ROOT = Path(__file__).resolve().parents[4]
NOTEBOOK_PATH = "notebooks/interactive_conversion.ipynb"


def test_distribution_metadata_includes_interactive_notebook() -> None:
    pyproject = tomllib.loads((PACKAGE_ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    maturin_includes = pyproject["tool"]["maturin"]["include"]
    notebook_formats = {
        entry.get("format", "both")
        for entry in maturin_includes
        if entry["path"] == NOTEBOOK_PATH
    }

    assert "sdist" in notebook_formats
    assert "wheel" in notebook_formats

    cargo = tomllib.loads((PACKAGE_ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    cargo_includes = set(cargo["package"]["include"])

    assert "notebooks/**" in cargo_includes or NOTEBOOK_PATH in cargo_includes


def test_python_release_workflow_checks_notebook_in_sdist() -> None:
    workflow = (REPO_ROOT / ".github" / "workflows" / "python-release.yml").read_text(
        encoding="utf-8"
    )

    assert NOTEBOOK_PATH in workflow
    assert "sdist missing release files" in workflow


def test_python_user_guide_documents_codebase_refactoring_advisor() -> None:
    guide = (REPO_ROOT / "docs" / "python.md").read_text(encoding="utf-8")

    assert "cobol2rust batch advisor" in guide
    assert "Add `--strict` to fail CI when findings are present." in guide
    assert "refactoring_advice_for_sources" in guide
    assert "files_with_findings" in guide
