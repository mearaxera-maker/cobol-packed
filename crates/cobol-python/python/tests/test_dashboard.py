from __future__ import annotations

import json
from pathlib import Path

from cobol_converter.dashboard import build_oracle_dashboard, load_report_files


def test_build_oracle_dashboard_summarizes_oracle_and_golden_reports() -> None:
    html = build_oracle_dashboard(
        [
            {
                "command": ["cargo", "test"],
                "passed": True,
                "returncode": 0,
                "stdout": "ok\n",
                "stderr": "",
            },
            {
                "project_dir": "generated/payroll",
                "expected_path": "golden/payroll.gnucobol.stdout",
                "matched": False,
                "returncode": 0,
                "expected": "100\n",
                "actual": "200\n",
                "stderr": "<runtime>",
            },
        ],
        title="Migration Oracle",
    )

    assert "Migration Oracle" in html
    assert "Total reports</span><strong>2</strong>" in html
    assert "Passed</span><strong>1</strong>" in html
    assert "Failed</span><strong>1</strong>" in html
    assert "golden/payroll.gnucobol.stdout" in html
    assert "&lt;runtime&gt;" in html


def test_load_report_files_reads_json_payloads(tmp_path: Path) -> None:
    first = tmp_path / "oracle.json"
    second = tmp_path / "compare.json"
    first.write_text(json.dumps({"passed": True}), encoding="utf-8")
    second.write_text(json.dumps({"matched": False}), encoding="utf-8")

    reports = load_report_files([first, second])

    assert reports == [
        {"passed": True, "__source_path": str(first)},
        {"matched": False, "__source_path": str(second)},
    ]
