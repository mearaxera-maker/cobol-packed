from __future__ import annotations

import json
import sys
from dataclasses import dataclass, field
from importlib import import_module
from typing import Any, BinaryIO

LSP_ERROR = 1
LSP_WARNING = 2
LSP_INFORMATION = 3


def diagnostics_from_result(result: dict[str, Any]) -> list[dict[str, Any]]:
    """Convert `cobol2rust.convert_cobol` diagnostics into LSP diagnostics."""
    if result.get("ok"):
        return []
    raw = result.get("diagnostics_json") or "[]"
    diagnostics = json.loads(raw)
    out: list[dict[str, Any]] = []
    for diagnostic in diagnostics:
        line = max(int(diagnostic.get("line") or 1) - 1, 0)
        character = max(int(diagnostic.get("column") or 1) - 1, 0)
        severity = diagnostic.get("severity", "Error")
        out.append(
            {
                "range": {
                    "start": {"line": line, "character": character},
                    "end": {"line": line, "character": character + 1},
                },
                "severity": _lsp_severity(str(severity)),
                "code": diagnostic.get("code", "E_UNKNOWN"),
                "source": "cobol2rust",
                "message": diagnostic.get("message", ""),
            }
        )
    return out


def analyze_document(
    source: str,
    *,
    dialect: str = "ibm",
    source_format: str = "free",
    copybooks: dict[str, str] | None = None,
    converter: Any | None = None,
) -> dict[str, Any]:
    """Run converter validation and return diagnostics plus generated Rust preview."""
    converter = converter or _load_converter()
    options = {"source_format": source_format, "copybooks": copybooks or {}}
    result = converter.convert_cobol(source, dialect, options)
    return {
        "ok": bool(result.get("ok")),
        "diagnostics": diagnostics_from_result(result),
        "rust_preview": result.get("rust") if result.get("ok") else None,
    }


@dataclass
class Cobol2RustLanguageServer:
    stdin: BinaryIO = field(default_factory=lambda: sys.stdin.buffer)
    stdout: BinaryIO = field(default_factory=lambda: sys.stdout.buffer)
    dialect: str = "ibm"
    source_format: str = "free"
    copybooks: dict[str, str] = field(default_factory=dict)
    documents: dict[str, str] = field(default_factory=dict)
    shutdown_requested: bool = False

    def run(self) -> None:
        while True:
            message = self._read_message()
            if message is None:
                return
            response = self.handle_message(message)
            if response is not None:
                self._write_message(response)
            if message.get("method") == "exit":
                return

    def handle_message(self, message: dict[str, Any]) -> dict[str, Any] | None:
        method = message.get("method")
        if method == "initialize":
            options = message.get("params", {}).get("initializationOptions", {})
            self.dialect = options.get("dialect", self.dialect)
            self.source_format = options.get("sourceFormat", self.source_format)
            self.copybooks = options.get("copybooks", self.copybooks)
            return {
                "jsonrpc": "2.0",
                "id": message.get("id"),
                "result": {
                    "capabilities": {
                        "textDocumentSync": 2,
                        "executeCommandProvider": {
                            "commands": ["cobol2rust.previewRust"],
                        },
                    },
                    "serverInfo": {"name": "cobol2rust-lsp", "version": "0.1.0"},
                },
            }
        if method == "shutdown":
            self.shutdown_requested = True
            return {"jsonrpc": "2.0", "id": message.get("id"), "result": None}
        if method == "textDocument/didOpen":
            text_document = message.get("params", {}).get("textDocument", {})
            uri = text_document.get("uri", "")
            self.documents[uri] = text_document.get("text", "")
            self.publish_diagnostics(uri)
            return None
        if method == "textDocument/didChange":
            params = message.get("params", {})
            uri = params.get("textDocument", {}).get("uri", "")
            changes = params.get("contentChanges", [])
            if uri and changes:
                self.documents[uri] = changes[-1].get("text", self.documents.get(uri, ""))
                self.publish_diagnostics(uri)
            return None
        if method == "cobol2rust/rustPreview":
            params = message.get("params", {})
            uri = params.get("textDocument", {}).get("uri", "")
            source = self.documents.get(uri, params.get("text", ""))
            analysis = analyze_document(
                source,
                dialect=self.dialect,
                source_format=self.source_format,
                copybooks=self.copybooks,
            )
            return {
                "jsonrpc": "2.0",
                "id": message.get("id"),
                "result": {
                    "rust": analysis.get("rust_preview"),
                    "diagnostics": analysis["diagnostics"],
                },
            }
        if "id" in message:
            return {
                "jsonrpc": "2.0",
                "id": message["id"],
                "error": {"code": -32601, "message": f"method not found: {method}"},
            }
        return None

    def publish_diagnostics(self, uri: str) -> None:
        source = self.documents.get(uri, "")
        try:
            analysis = analyze_document(
                source,
                dialect=self.dialect,
                source_format=self.source_format,
                copybooks=self.copybooks,
            )
            diagnostics = analysis["diagnostics"]
        except Exception as error:  # pragma: no cover - defensive server boundary
            diagnostics = [
                {
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 1},
                    },
                    "severity": LSP_ERROR,
                    "source": "cobol2rust",
                    "message": f"cobol2rust analysis failed: {error}",
                }
            ]
        self._write_message(
            {
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {"uri": uri, "diagnostics": diagnostics},
            }
        )

    def _read_message(self) -> dict[str, Any] | None:
        headers: dict[str, str] = {}
        while True:
            line = self.stdin.readline()
            if not line:
                return None
            decoded = line.decode("ascii").strip()
            if not decoded:
                break
            key, _, value = decoded.partition(":")
            headers[key.lower()] = value.strip()
        length = int(headers.get("content-length", "0"))
        if length <= 0:
            return None
        return json.loads(self.stdin.read(length).decode("utf-8"))

    def _write_message(self, payload: dict[str, Any]) -> None:
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
        self.stdout.write(header)
        self.stdout.write(body)
        self.stdout.flush()


def main() -> None:
    Cobol2RustLanguageServer().run()


def _lsp_severity(severity: str) -> int:
    normalized = severity.lower()
    if normalized == "warning":
        return LSP_WARNING
    if normalized in {"info", "information"}:
        return LSP_INFORMATION
    return LSP_ERROR


def _load_converter() -> Any:
    return import_module("cobol_converter._native")


if __name__ == "__main__":
    main()
