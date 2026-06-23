from __future__ import annotations

import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path

import cobol_converter


class PublicApiTests(unittest.TestCase):
    def test_public_api_delegates_convert_and_preprocess_to_native_extension(self) -> None:
        fake = types.SimpleNamespace()
        calls: list[tuple[str, tuple, dict]] = []

        def convert_cobol(source: str, dialect: str, options: dict) -> dict:
            calls.append(("convert_cobol", (source, dialect, options), {}))
            return {"ok": True, "rust": "pub struct Program;\n", "diagnostics": []}

        def check_cobol(source: str, dialect: str, options: dict) -> dict:
            calls.append(("check_cobol", (source, dialect, options), {}))
            return {"ok": True, "diagnostics": [], "diagnostics_json": "[]"}

        def preprocess(source: str, copybooks: dict, source_format: str = "auto") -> str:
            calls.append(("preprocess", (source, copybooks, source_format), {}))
            return "expanded"

        fake.convert_cobol = convert_cobol
        fake.check_cobol = check_cobol
        fake.preprocess = preprocess
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        try:
            self.assertEqual(
                cobol_converter.convert_cobol("DISPLAY 'X'.", "ibm_zos", {}),
                {
                    "ok": True,
                    "rust": "pub struct Program;\n",
                    "diagnostics": [],
                },
            )
            self.assertEqual(
                cobol_converter.check_cobol("DISPLAY 'X'.", "ibm_zos", {}),
                {"ok": True, "diagnostics": [], "diagnostics_json": "[]"},
            )
            self.assertEqual(
                cobol_converter.preprocess(
                    "COPY REC.", {"REC.cpy": "01 X PIC X."}, source_format="free"
                ),
                "expanded",
            )
            self.assertEqual(
                calls,
                [
                    ("convert_cobol", ("DISPLAY 'X'.", "ibm_zos", {}), {}),
                    ("check_cobol", ("DISPLAY 'X'.", "ibm_zos", {}), {}),
                    ("preprocess", ("COPY REC.", {"REC.cpy": "01 X PIC X."}, "free"), {}),
                ],
            )
        finally:
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_public_api_exposes_golden_helpers_with_runner_injection(self) -> None:
        commands: list[tuple[list[str], Path | None]] = []

        def runner(
            command: list[str], cwd: Path | None = None
        ) -> subprocess.CompletedProcess[str]:
            commands.append((command, cwd))
            if command[0] == "cobc":
                return subprocess.CompletedProcess(command, 0, "", "")
            return subprocess.CompletedProcess(command, 0, "HELLO\n", "")

        with tempfile.TemporaryDirectory(prefix="cobol-python-public-api-") as tmp:
            root = Path(tmp)
            source = root / "hello.cbl"
            source.write_text(
                "IDENTIFICATION DIVISION.\n"
                "PROGRAM-ID. HELLO.\n"
                "PROCEDURE DIVISION.\n"
                "MAIN.\n"
                'DISPLAY "HELLO".\n'
                "STOP RUN.\n",
                encoding="utf-8",
            )
            golden = root / "golden"

            record = cobol_converter.record_golden_output(source, golden, runner=runner)

            self.assertTrue(record["passed"])
            stdout = golden / "hello.gnucobol.stdout"
            self.assertEqual(stdout.read_text(encoding="utf-8"), "HELLO\n")

            project = root / "generated"
            project.mkdir()
            comparison = cobol_converter.compare_generated_project_to_golden(
                project, stdout, runner=runner
            )

            self.assertTrue(comparison["matched"])
            self.assertIn("record_golden_output", cobol_converter.__all__)
            self.assertIn("compare_generated_project_to_golden", cobol_converter.__all__)
            self.assertGreaterEqual(len(commands), 3)


if __name__ == "__main__":
    unittest.main()
