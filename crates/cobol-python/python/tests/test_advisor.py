from __future__ import annotations

import json
import sys
import types

import cobol_converter
from cobol_converter.advisor import (
    build_codebase_refactoring_report,
    build_refactoring_advice,
)


def test_build_refactoring_advice_returns_capability_backed_findings() -> None:
    analysis = {
        "unsupported_features": [
            {
                "feature": "ALTER",
                "capability_id": "procedure.alter",
                "status": "blocked",
                "paragraphs": ["P1", "P2"],
                "advice": "Refactor ALTER before migration.",
            }
        ]
    }

    advice = build_refactoring_advice(analysis)

    assert advice == [
        {
            "feature": "ALTER",
            "capability_id": "procedure.alter",
            "status": "blocked",
            "paragraphs": ["P1", "P2"],
            "advice": "Refactor ALTER before migration.",
        }
    ]


def test_refactoring_advice_public_api_uses_native_analysis() -> None:
    fake = types.SimpleNamespace()
    calls: list[tuple[str, str]] = []

    def analyze_source(path: str, source: str) -> str:
        calls.append((path, source))
        return json.dumps(
            {
                "unsupported_features": [
                    {
                        "feature": "NEXT SENTENCE",
                        "status": "partial",
                        "paragraphs": ["MAIN"],
                    }
                ]
            }
        )

    fake.analyze_source = analyze_source
    previous = sys.modules.get("cobol_converter._native")
    sys.modules["cobol_converter._native"] = fake
    try:
        advice = cobol_converter.refactoring_advice("payroll.cbl", "NEXT SENTENCE.")
    finally:
        if previous is None:
            sys.modules.pop("cobol_converter._native", None)
        else:
            sys.modules["cobol_converter._native"] = previous

    assert calls == [("payroll.cbl", "NEXT SENTENCE.")]
    assert advice == [
        {
            "feature": "NEXT SENTENCE",
            "capability_id": None,
            "status": "partial",
            "paragraphs": ["MAIN"],
            "advice": "Review and refactor NEXT SENTENCE before migration.",
        }
    ]


def test_build_codebase_refactoring_report_summarizes_files_and_features() -> None:
    sources = {
        "payroll.cbl": "ALTER P1 TO PROCEED TO P2.",
        "clean.cbl": "DISPLAY 'OK'.",
    }
    calls: list[tuple[str, str]] = []

    def analyzer(path: str, source: str) -> str:
        calls.append((path, source))
        if path == "payroll.cbl":
            return json.dumps(
                {
                    "unsupported_features": [
                        {
                            "feature": "ALTER",
                            "capability_id": "procedure.alter",
                            "status": "blocked",
                            "paragraphs": ["P1"],
                        }
                    ]
                }
            )
        return json.dumps({"unsupported_features": []})

    report = build_codebase_refactoring_report(sources, analyzer=analyzer)

    assert calls == [
        ("clean.cbl", "DISPLAY 'OK'."),
        ("payroll.cbl", "ALTER P1 TO PROCEED TO P2."),
    ]
    assert report["total_files"] == 2
    assert report["files_with_findings"] == 1
    assert report["total_findings"] == 1
    assert report["features"] == [
        {
            "feature": "ALTER",
            "capability_id": "procedure.alter",
            "status": "blocked",
            "count": 1,
            "files": ["payroll.cbl"],
            "paragraphs": ["P1"],
        }
    ]
    assert report["files"][1]["path"] == "payroll.cbl"
    assert report["files"][1]["findings"][0]["advice"] == (
        "Review and refactor ALTER before migration."
    )
