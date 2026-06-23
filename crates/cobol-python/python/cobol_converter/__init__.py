"""Python tooling for the COBOL-to-Rust converter."""

from __future__ import annotations

import json
from importlib import import_module
from typing import Any

__all__ = [
    "__version__",
    "analyze_source",
    "batch_convert_sources",
    "check_cobol",
    "convert_cobol",
    "convert_project",
    "dependency_graph_dot",
    "preprocess",
    "refactoring_advice",
    "refactoring_advice_for_sources",
    "run_oracle_suite",
]

__version__ = "0.1.0"


def _native_module() -> Any:
    return import_module("cobol_converter._native")


def preprocess(
    source: str,
    copybooks: dict[str, str] | None = None,
    *,
    source_format: str = "auto",
) -> str:
    """Expand COPY members from an in-memory copybook mapping."""
    return _native_module().preprocess(source, copybooks or {}, source_format=source_format)


def convert_cobol(source: str, dialect: str, options: dict[str, Any] | None = None) -> dict:
    """Convert COBOL source in memory and return Rust or structured diagnostics."""
    return _native_module().convert_cobol(source, dialect, options or {})


def check_cobol(source: str, dialect: str, options: dict[str, Any] | None = None) -> dict:
    """Validate COBOL source in memory without generating Rust output."""
    return _native_module().check_cobol(source, dialect, options or {})


def analyze_source(path: str, source: str) -> str:
    """Return converter source analysis as a JSON string."""
    return _native_module().analyze_source(path, source)


def dependency_graph_dot(path: str, source: str) -> str:
    """Return a DOT dependency graph for COPY and CALL relationships."""
    return _native_module().dependency_graph_dot(path, source)


def refactoring_advice(path: str, source: str) -> list[dict[str, Any]]:
    """Return normalized refactoring advice for unsupported migration features."""
    from .advisor import build_refactoring_advice

    return build_refactoring_advice(json.loads(analyze_source(path, source)))


def refactoring_advice_for_sources(sources: dict[str, str]) -> dict[str, Any]:
    """Return a codebase-level refactoring-advice report for source mappings."""
    from .advisor import build_codebase_refactoring_report

    return build_codebase_refactoring_report(sources, analyzer=analyze_source)


def convert_project(
    source: str,
    dialect: str,
    output_dir: str,
    options: dict[str, Any] | None = None,
) -> str:
    """Convert source into a generated Rust project and return JSON metadata."""
    return _native_module().convert_project(source, dialect, output_dir, options or {})


def batch_convert_sources(
    sources: dict[str, str],
    dialect: str,
    output_dir: str,
    options: dict[str, Any] | None = None,
) -> str:
    """Convert multiple in-memory COBOL sources into generated project directories."""
    return _native_module().batch_convert_sources(sources, dialect, output_dir, options or {})


def run_oracle_suite(repo_root: str = ".") -> dict:
    """Run the Rust oracle suite from Python and return a structured summary."""
    from .oracle import run_oracle_suite as _run_oracle_suite

    return _run_oracle_suite(repo_root)
