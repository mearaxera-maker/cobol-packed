from __future__ import annotations

import json
from pathlib import Path


def test_interactive_conversion_notebook_covers_required_workflow() -> None:
    notebook = (
        Path(__file__).resolve().parents[2]
        / "notebooks"
        / "interactive_conversion.ipynb"
    )
    payload = json.loads(notebook.read_text(encoding="utf-8"))
    cells = payload["cells"]
    joined_source = "\n".join(
        "".join(cell.get("source", [])) for cell in cells if cell.get("cell_type") == "code"
    )

    assert "source_path.read_text" in joined_source
    assert "cobol_converter.preprocess" in joined_source
    assert "cobol_converter.check_cobol" in joined_source
    assert "diagnostics" in joined_source
    assert "cobol_converter.convert_cobol" in joined_source
    assert "result[\"rust\"]" in joined_source
