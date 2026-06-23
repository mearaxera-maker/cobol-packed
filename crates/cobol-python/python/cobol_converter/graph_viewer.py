from __future__ import annotations

import html
from typing import Any


def dependency_edges(analysis: dict[str, Any]) -> list[dict[str, str]]:
    """Return COPY and CALL edges from an analysis payload."""
    program = str(analysis.get("program_id") or analysis.get("path") or "program")
    edges: list[dict[str, str]] = []
    for copybook in analysis.get("copybooks", []):
        edges.append(
            {
                "source": program,
                "target": f"copybook:{copybook}",
                "kind": "COPY",
            }
        )
    for call in analysis.get("calls", []):
        edges.append(
            {
                "source": program,
                "target": str(call.get("target", "UNKNOWN")),
                "kind": "CALL" if call.get("literal", False) else "CALL dynamic",
            }
        )
    return edges


def build_dependency_graph_html(
    analysis: dict[str, Any],
    *,
    title: str = "COBOL Dependency Graph",
) -> str:
    """Render a standalone HTML dependency graph viewer."""
    program = str(analysis.get("program_id") or analysis.get("path") or "program")
    edges = dependency_edges(analysis)
    nodes = _graph_nodes(program, edges)
    positions = _layout(nodes, program)
    width = max(760, max((x for x, _ in positions.values()), default=0) + 220)
    height = max(360, max((y for _, y in positions.values()), default=0) + 120)
    svg_edges = "\n".join(_render_svg_edge(edge, positions) for edge in edges)
    svg_nodes = "\n".join(_render_svg_node(node, positions[node], node == program) for node in nodes)
    table_rows = "\n".join(_render_edge_row(edge) for edge in edges)
    if not table_rows:
        table_rows = '<tr><td colspan="3">No COPY or CALL dependencies found.</td></tr>'
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
      --panel: #ffffff;
      --program: #1f5eff;
      --copy: #0f7b3f;
      --call: #8b4e00;
    }}
    @media (prefers-color-scheme: dark) {{
      :root {{
        --bg: #111318;
        --fg: #f5f7fb;
        --muted: #a7b0c0;
        --line: #2d3442;
        --panel: #181c24;
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
    h1 {{ margin: 0 0 8px; font-size: 28px; }}
    .meta {{ color: var(--muted); margin-bottom: 20px; }}
    .graph {{
      overflow-x: auto;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 12px;
    }}
    svg {{ min-width: {width}px; max-width: none; }}
    .node rect {{ fill: var(--panel); stroke: var(--line); stroke-width: 1.5; }}
    .node.program rect {{ stroke: var(--program); stroke-width: 2.5; }}
    .node text {{ fill: var(--fg); font-size: 13px; dominant-baseline: middle; }}
    .edge {{ stroke: var(--muted); stroke-width: 1.6; fill: none; }}
    .edge-label {{ fill: var(--muted); font-size: 12px; }}
    table {{
      width: 100%;
      margin-top: 20px;
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
      font-size: 14px;
    }}
    th {{ color: var(--muted); }}
    tr:last-child td {{ border-bottom: 0; }}
  </style>
</head>
<body>
<main>
  <h1>{_esc(title)}</h1>
  <div class="meta">Source: {_esc(analysis.get("path", ""))}</div>
  <section class="graph" aria-label="Dependency graph">
    <svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img">
      <defs>
        <marker id="arrow" markerWidth="10" markerHeight="10" refX="8" refY="3" orient="auto">
          <path d="M0,0 L0,6 L9,3 z" fill="currentColor"></path>
        </marker>
      </defs>
{svg_edges}
{svg_nodes}
    </svg>
  </section>
  <table>
    <thead>
      <tr><th>Source</th><th>Relationship</th><th>Target</th></tr>
    </thead>
    <tbody>
      {table_rows}
    </tbody>
  </table>
</main>
</body>
</html>
"""


def _graph_nodes(program: str, edges: list[dict[str, str]]) -> list[str]:
    nodes = [program]
    seen = {program}
    for edge in edges:
        for key in ("source", "target"):
            value = edge[key]
            if value not in seen:
                seen.add(value)
                nodes.append(value)
    return nodes


def _layout(nodes: list[str], program: str) -> dict[str, tuple[int, int]]:
    copy_nodes = [node for node in nodes if node.startswith("copybook:")]
    call_nodes = [node for node in nodes if node != program and not node.startswith("copybook:")]
    positions: dict[str, tuple[int, int]] = {program: (90, 140)}
    for idx, node in enumerate(copy_nodes):
        positions[node] = (360, 80 + idx * 86)
    for idx, node in enumerate(call_nodes):
        positions[node] = (620, 80 + idx * 86)
    return positions


def _render_svg_edge(edge: dict[str, str], positions: dict[str, tuple[int, int]]) -> str:
    source_x, source_y = positions[edge["source"]]
    target_x, target_y = positions[edge["target"]]
    start_x = source_x + 170
    end_x = target_x
    mid_x = (start_x + end_x) // 2
    label_y = (source_y + target_y) // 2 - 8
    path = f"M{start_x},{source_y} C{mid_x},{source_y} {mid_x},{target_y} {end_x},{target_y}"
    return (
        f'      <path class="edge" d="{path}" marker-end="url(#arrow)"></path>\n'
        f'      <text class="edge-label" x="{mid_x - 36}" y="{label_y}">{_esc(edge["kind"])}</text>'
    )


def _render_svg_node(node: str, position: tuple[int, int], is_program: bool) -> str:
    x, y = position
    label = node.removeprefix("copybook:")
    class_name = "node program" if is_program else "node"
    return (
        f'      <g class="{class_name}">'
        f'<rect x="{x}" y="{y - 24}" width="170" height="48" rx="8"></rect>'
        f'<text x="{x + 12}" y="{y}">{_esc(label)}</text>'
        "</g>"
    )


def _render_edge_row(edge: dict[str, str]) -> str:
    return (
        "<tr>"
        f"<td>{_esc(edge['source'])}</td>"
        f"<td>{_esc(edge['kind'])}</td>"
        f"<td>{_esc(edge['target'])}</td>"
        "</tr>"
    )


def _esc(value: Any) -> str:
    return html.escape(str(value), quote=True)
