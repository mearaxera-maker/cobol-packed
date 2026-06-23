from __future__ import annotations

import json
from typing import Any, Callable

AnalysisRunner = Callable[[str, str], str | dict[str, Any]]


def build_refactoring_advice(analysis: dict[str, Any]) -> list[dict[str, Any]]:
    """Normalize unsupported-feature analysis into migration refactoring advice."""
    findings: list[dict[str, Any]] = []
    for feature in analysis.get("unsupported_features", []):
        name = str(feature.get("feature") or "feature")
        findings.append(
            {
                "feature": name,
                "capability_id": feature.get("capability_id"),
                "status": feature.get("status", "unknown"),
                "paragraphs": list(feature.get("paragraphs") or []),
                "advice": feature.get("advice") or f"Review and refactor {name} before migration.",
            }
        )
    return findings


def build_codebase_refactoring_report(
    sources: dict[str, str],
    *,
    analyzer: AnalysisRunner,
) -> dict[str, Any]:
    """Analyze a source mapping and summarize unsupported-feature advice."""
    files: list[dict[str, Any]] = []
    feature_summary: dict[tuple[str, str | None, str], dict[str, Any]] = {}

    for path, source in sorted(sources.items()):
        raw_analysis = analyzer(path, source)
        analysis = json.loads(raw_analysis) if isinstance(raw_analysis, str) else raw_analysis
        findings = build_refactoring_advice(analysis)
        files.append({"path": path, "count": len(findings), "findings": findings})
        for finding in findings:
            key = (
                finding["feature"],
                finding.get("capability_id"),
                finding.get("status", "unknown"),
            )
            entry = feature_summary.setdefault(
                key,
                {
                    "feature": finding["feature"],
                    "capability_id": finding.get("capability_id"),
                    "status": finding.get("status", "unknown"),
                    "count": 0,
                    "files": set(),
                    "paragraphs": set(),
                },
            )
            entry["count"] += 1
            entry["files"].add(path)
            entry["paragraphs"].update(finding.get("paragraphs") or [])

    features = []
    for entry in feature_summary.values():
        features.append(
            {
                **entry,
                "files": sorted(entry["files"]),
                "paragraphs": sorted(entry["paragraphs"]),
            }
        )
    features.sort(key=lambda item: (item["feature"], item["status"], item["capability_id"] or ""))

    return {
        "total_files": len(files),
        "files_with_findings": sum(1 for item in files if item["count"]),
        "total_findings": sum(item["count"] for item in files),
        "features": features,
        "files": files,
    }
