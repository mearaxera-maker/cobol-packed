from __future__ import annotations

from cobol_converter.graph_viewer import build_dependency_graph_html, dependency_edges


def test_dependency_edges_extracts_copy_and_call_relationships() -> None:
    analysis = {
        "program_id": "PAYROLL",
        "copybooks": ["CUSTOMER"],
        "calls": [
            {"target": "TAXCALC", "literal": True, "line": 12},
            {"target": "WS-PROGRAM", "literal": False, "line": 13},
        ],
    }

    edges = dependency_edges(analysis)

    assert edges == [
        {"source": "PAYROLL", "target": "copybook:CUSTOMER", "kind": "COPY"},
        {"source": "PAYROLL", "target": "TAXCALC", "kind": "CALL"},
        {"source": "PAYROLL", "target": "WS-PROGRAM", "kind": "CALL dynamic"},
    ]


def test_build_dependency_graph_html_renders_escaped_svg() -> None:
    analysis = {
        "path": "src/PAYROLL.cbl",
        "program_id": "PAY<ROLL>",
        "copybooks": ["CUST&ADDR"],
        "calls": [{"target": "TAXCALC", "literal": True, "line": 12}],
    }

    html = build_dependency_graph_html(analysis, title="Payroll Graph")

    assert "Payroll Graph" in html
    assert "<svg" in html
    assert "PAY&lt;ROLL&gt;" in html
    assert "CUST&amp;ADDR" in html
    assert "TAXCALC" in html
    assert "CALL" in html
