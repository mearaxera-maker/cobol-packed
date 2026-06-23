from __future__ import annotations

import html
import json
from pathlib import Path
from typing import Any


def load_report_files(paths: list[str | Path]) -> list[dict[str, Any]]:
    """Load oracle/golden JSON report files."""
    reports: list[dict[str, Any]] = []
    for path_text in paths:
        path = Path(path_text)
        payload = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(payload, dict):
            raise ValueError(f"dashboard report must be a JSON object: {path}")
        payload.setdefault("__source_path", str(path))
        reports.append(payload)
    return reports


def build_oracle_dashboard(
    reports: list[dict[str, Any]],
    *,
    title: str = "COBOL Oracle Dashboard",
) -> str:
    """Render a standalone HTML dashboard for oracle and golden-compare reports."""
    rows = [_dashboard_row(report, idx + 1) for idx, report in enumerate(reports)]
    passed = sum(1 for row in rows if row["passed"])
    total = len(rows)
    failed = total - passed
    row_html = "\n".join(_render_row(row) for row in rows)
    detail_html = "\n".join(_render_details(row) for row in rows)
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{_esc(title)}</title>
  <style>
    :root {{
      color-scheme: light dark;
      --bg: #f7f8fa;
      --fg: #1d2433;
      --muted: #5d6678;
      --line: #d8dde7;
      --pass: #0f7b3f;
      --fail: #b42318;
      --panel: #ffffff;
      --code: #101828;
    }}
    @media (prefers-color-scheme: dark) {{
      :root {{
        --bg: #111318;
        --fg: #f5f7fb;
        --muted: #a7b0c0;
        --line: #2d3442;
        --panel: #181c24;
        --code: #eef2f8;
      }}
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: var(--bg);
      color: var(--fg);
    }}
    main {{
      max-width: 1120px;
      margin: 0 auto;
      padding: 32px 20px 48px;
    }}
    h1 {{ margin: 0 0 20px; font-size: 28px; }}
    h2 {{ margin: 28px 0 12px; font-size: 18px; }}
    .summary {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
      gap: 12px;
      margin-bottom: 24px;
    }}
    .metric {{
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 14px 16px;
    }}
    .metric span {{ color: var(--muted); display: block; font-size: 13px; }}
    .metric strong {{ display: block; font-size: 28px; margin-top: 4px; }}
    table {{
      width: 100%;
      border-collapse: collapse;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      overflow: hidden;
    }}
    th, td {{
      padding: 10px 12px;
      border-bottom: 1px solid var(--line);
      text-align: left;
      vertical-align: top;
      font-size: 14px;
    }}
    th {{ color: var(--muted); font-weight: 600; }}
    tr:last-child td {{ border-bottom: 0; }}
    .status {{
      display: inline-block;
      min-width: 64px;
      font-weight: 700;
    }}
    .pass {{ color: var(--pass); }}
    .fail {{ color: var(--fail); }}
    details {{
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      margin: 10px 0;
      padding: 10px 12px;
    }}
    summary {{ cursor: pointer; font-weight: 600; }}
    pre {{
      overflow-x: auto;
      white-space: pre-wrap;
      background: color-mix(in srgb, var(--panel), var(--code) 8%);
      border: 1px solid var(--line);
      border-radius: 6px;
      padding: 10px;
      color: var(--code);
    }}
  </style>
</head>
<body>
<main>
  <h1>{_esc(title)}</h1>
  <section class="summary">
    <div class="metric"><span>Total reports</span><strong>{total}</strong></div>
    <div class="metric"><span>Passed</span><strong>{passed}</strong></div>
    <div class="metric"><span>Failed</span><strong>{failed}</strong></div>
  </section>
  <h2>Results</h2>
  <table>
    <thead>
      <tr><th>Status</th><th>Type</th><th>Name</th><th>Return Code</th><th>Source</th></tr>
    </thead>
    <tbody>
{row_html}
    </tbody>
  </table>
  <h2>Details</h2>
{detail_html}
</main>
</body>
</html>
"""


def _dashboard_row(report: dict[str, Any], index: int) -> dict[str, Any]:
    kind = "golden-compare" if "matched" in report else "oracle-suite"
    passed = bool(report.get("matched")) if kind == "golden-compare" else bool(report.get("passed"))
    if kind == "golden-compare":
        name = report.get("expected_path") or report.get("project_dir") or f"report-{index}"
        stdout = report.get("actual", "")
    else:
        command = report.get("command", [])
        name = " ".join(str(part) for part in command) if command else f"report-{index}"
        stdout = report.get("stdout", "")
    return {
        "index": index,
        "kind": kind,
        "passed": passed,
        "name": str(name),
        "returncode": report.get("returncode", ""),
        "source": str(report.get("__source_path", "")),
        "stdout": str(stdout),
        "stderr": str(report.get("stderr", "")),
        "expected": str(report.get("expected", "")),
    }


def _render_row(row: dict[str, Any]) -> str:
    status_class = "pass" if row["passed"] else "fail"
    status_text = "PASS" if row["passed"] else "FAIL"
    return (
        "      <tr>"
        f"<td><span class=\"status {status_class}\">{status_text}</span></td>"
        f"<td>{_esc(row['kind'])}</td>"
        f"<td>{_esc(row['name'])}</td>"
        f"<td>{_esc(row['returncode'])}</td>"
        f"<td>{_esc(row['source'])}</td>"
        "</tr>"
    )


def _render_details(row: dict[str, Any]) -> str:
    expected = ""
    if row["expected"]:
        expected = f"<h3>Expected</h3><pre>{_esc(row['expected'])}</pre>"
    return f"""  <details {'open' if not row['passed'] else ''}>
    <summary>{_esc(row['name'])}</summary>
    {expected}
    <h3>Stdout</h3>
    <pre>{_esc(row['stdout'])}</pre>
    <h3>Stderr</h3>
    <pre>{_esc(row['stderr'])}</pre>
  </details>"""


def _esc(value: Any) -> str:
    return html.escape(str(value), quote=True)
