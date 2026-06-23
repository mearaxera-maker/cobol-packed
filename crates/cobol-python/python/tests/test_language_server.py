from __future__ import annotations

import json

from cobol_converter.language_server import analyze_document, diagnostics_from_result


class FakeConverter:
    def __init__(self, result: dict) -> None:
        self.result = result
        self.calls: list[tuple[str, str, dict]] = []

    def convert_cobol(self, source: str, dialect: str, options: dict) -> dict:
        self.calls.append((source, dialect, options))
        return self.result


def test_diagnostics_from_result_maps_converter_errors_to_lsp_shape() -> None:
    result = {
        "ok": False,
        "diagnostics_json": json.dumps(
            [
                {
                    "code": "E_TEST",
                    "severity": "Error",
                    "message": "broken",
                    "file": "input.cbl",
                    "line": 3,
                    "column": 5,
                }
            ]
        ),
    }

    diagnostics = diagnostics_from_result(result)

    assert diagnostics == [
        {
            "range": {
                "start": {"line": 2, "character": 4},
                "end": {"line": 2, "character": 5},
            },
            "severity": 1,
            "code": "E_TEST",
            "source": "cobol2rust",
            "message": "broken",
        }
    ]


def test_analyze_document_returns_preview_and_uses_converter_options() -> None:
    converter = FakeConverter({"ok": True, "rust": "pub fn main() {}\n", "diagnostics": []})

    analysis = analyze_document(
        "DISPLAY \"OK\".",
        dialect="gnucobol",
        source_format="free",
        copybooks={"COPY.cpy": "01 X PIC X."},
        converter=converter,
    )

    assert analysis["diagnostics"] == []
    assert analysis["rust_preview"] == "pub fn main() {}\n"
    assert converter.calls == [
        (
            "DISPLAY \"OK\".",
            "gnucobol",
            {"source_format": "free", "copybooks": {"COPY.cpy": "01 X PIC X."}},
        )
    ]
