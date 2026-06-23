from __future__ import annotations

from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[4]
PACKAGE_ROOT = Path(__file__).resolve().parents[2]


def test_onboarding_docs_show_requested_beginner_cli_commands() -> None:
    docs = (REPO_ROOT / "docs" / "python.md").read_text(encoding="utf-8")
    readme = (PACKAGE_ROOT / "README.md").read_text(encoding="utf-8")
    combined = f"{docs}\n{readme}"

    assert "cobol2rust convert --dialect ibm_zos --output-dir rust_src input.cbl" in combined
    assert "cobol2rust check --strict input.cbl" in combined
    assert "cobol2rust oracle run" in combined
    assert "cobol2rust init-migration" in combined


def test_package_readme_documents_codebase_advisor_surface() -> None:
    readme = (PACKAGE_ROOT / "README.md").read_text(encoding="utf-8")

    assert "cobol_converter.refactoring_advice_for_sources" in readme
    assert "cobol2rust batch advisor" in readme
    assert "Add `--strict` to fail CI when findings are present." in readme
    assert "files_with_findings" in readme


def test_package_readme_documents_public_oracle_helpers() -> None:
    readme = (PACKAGE_ROOT / "README.md").read_text(encoding="utf-8")

    assert "cobol_converter.record_golden_output" in readme
    assert "cobol_converter.compare_generated_project_to_golden" in readme
    assert "runner=" in readme
