from __future__ import annotations

import importlib
import json
import sys
import tempfile
import types
import unittest
from pathlib import Path


class FakeClickCommand:
    def __init__(self, func):
        self.func = func

    def __call__(self, *args, **kwargs):
        return self.func(*args, **kwargs)

    def command(self, *_args, **_kwargs):
        return lambda func: FakeClickCommand(func)

    def group(self, *_args, **_kwargs):
        return lambda func: FakeClickCommand(func)


def fake_click_module() -> types.SimpleNamespace:
    class ClickException(Exception):
        pass

    class PathType:
        def __init__(self, *_args, **_kwargs):
            pass

    def identity_decorator(*_args, **_kwargs):
        return lambda func: func

    return types.SimpleNamespace(
        ClickException=ClickException,
        Path=PathType,
        argument=identity_decorator,
        command=identity_decorator,
        group=lambda *_args, **_kwargs: lambda func: FakeClickCommand(func),
        option=identity_decorator,
        echo=lambda *_args, **_kwargs: None,
        secho=lambda *_args, **_kwargs: None,
        progressbar=lambda items, **_kwargs: items,
    )


def scratch_root(name: str) -> Path:
    path = Path(tempfile.mkdtemp(prefix=f"cobol-python-{name}-"))
    path.mkdir(parents=True, exist_ok=True)
    return path


class CliTests(unittest.TestCase):
    def test_cli_module_imports_without_native_extension(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        previous = sys.modules.pop("cobol_converter._native", None)
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            self.assertTrue(hasattr(module, "main"))
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is not None:
                sys.modules["cobol_converter._native"] = previous

    def test_cli_text_output_uses_atomic_temp_write(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        previous = sys.modules.pop("cobol_converter._native", None)
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("atomic-text-output")
            output = root / "reports" / "summary.json"
            original_write_text = Path.write_text

            def reject_direct_destination_write(self, *args, **kwargs):
                if self == output:
                    raise AssertionError("direct destination write")
                return original_write_text(self, *args, **kwargs)

            Path.write_text = reject_direct_destination_write
            try:
                module._write_text_output(str(output), '{"ok": true}')
            finally:
                Path.write_text = original_write_text

            self.assertEqual(output.read_text(encoding="utf-8"), '{"ok": true}')
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is not None:
                sys.modules["cobol_converter._native"] = previous

    def test_cli_text_output_no_clobber_requires_force(self) -> None:
        click_module = fake_click_module()
        previous_click = sys.modules.get("click")
        sys.modules["click"] = click_module
        previous = sys.modules.pop("cobol_converter._native", None)
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("no-clobber-text-output")
            output = root / "reports" / "summary.json"
            output.parent.mkdir(parents=True)
            output.write_text("old", encoding="utf-8")

            with self.assertRaises(click_module.ClickException):
                module._write_text_output(str(output), "new")

            self.assertEqual(output.read_text(encoding="utf-8"), "old")

            module._write_text_output(str(output), "new", force=True)

            self.assertEqual(output.read_text(encoding="utf-8"), "new")
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is not None:
                sys.modules["cobol_converter._native"] = previous

    def test_read_copybooks_copybook_policy_filters_unrelated_and_oversized_files(self) -> None:
        click_module = fake_click_module()
        previous_click = sys.modules.get("click")
        sys.modules["click"] = click_module
        previous = sys.modules.pop("cobol_converter._native", None)
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("copybook-policy")
            (root / "FIELDS.cpy").write_text("01 WS-FIELD PIC X.\n", encoding="utf-8")
            (root / "COPYBOOK").write_text("01 WS-NO-SUFFIX PIC X.\n", encoding="utf-8")
            (root / "README.txt").write_text("notes, not a copybook\n", encoding="utf-8")
            large_text = "A" * (4 * 1024 * 1024 + 1)
            (root / "TOO-BIG.cpy").write_text(large_text, encoding="utf-8")
            diagnostics: list[str] = []
            original_echo = module.click.echo
            module.click.echo = lambda message, *args, **kwargs: diagnostics.append(str(message))
            try:
                copybooks = module._read_copybooks((str(root),))
                self.assertEqual(
                    copybooks,
                    {
                        "COPYBOOK": "01 WS-NO-SUFFIX PIC X.\n",
                        "FIELDS.cpy": "01 WS-FIELD PIC X.\n",
                    },
                )
                self.assertTrue(
                    any("README.txt" in item and "unsupported extension" in item for item in diagnostics)
                )
                self.assertTrue(
                    any("TOO-BIG.cpy" in item and "exceeds" in item for item in diagnostics)
                )

                all_files = module._read_copybooks(
                    (str(root),),
                    include_all_files=True,
                    max_bytes=len(large_text) + 1,
                )
                self.assertIn("README.txt", all_files)
                self.assertIn("TOO-BIG.cpy", all_files)
            finally:
                module.click.echo = original_echo
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is not None:
                sys.modules["cobol_converter._native"] = previous

    def test_init_migration_scaffolds_jcl_style_file_mapping(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("init-migration-file-map") / "migration"

            module.init_migration(str(root))

            config = (root / "cobol2rust.toml").read_text(encoding="utf-8")
            self.assertIn("[file_map]", config)
            self.assertIn('INFILE = "data/input.dat"', config)
            self.assertIn("[[dd]]", config)
            self.assertIn('name = "SORTWK01"', config)
            self.assertIn('path = "work/sortwk01.dat"', config)
            loaded = module.load_migration_config(root / "cobol2rust.toml")
            self.assertEqual(
                loaded.file_map,
                {
                    "INFILE": "data/input.dat",
                    "OUTFILE": "data/output.dat",
                    "SORTWK01": "work/sortwk01.dat",
                },
            )
            self.assertTrue((root / "data").is_dir())
            self.assertTrue((root / "work").is_dir())
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click

    def test_check_uses_shared_converter_config(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()
        calls: list[tuple[str, str, dict]] = []

        def check_cobol(source: str, dialect: str, options: dict) -> dict:
            calls.append((source, dialect, options))
            return {"ok": True, "diagnostics": [], "diagnostics_json": "[]"}

        def convert_cobol(_source: str, _dialect: str, _options: dict) -> dict:
            raise AssertionError("check command must not generate Rust")

        fake.check_cobol = check_cobol
        fake.convert_cobol = convert_cobol
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("cli-config")
            source = root / "program.cbl"
            source.write_text("DISPLAY 'OK'.\n", encoding="utf-8")
            copybooks = root / "copybooks"
            copybooks.mkdir(exist_ok=True)
            (copybooks / "FIELDS.cpy").write_text("01 WS-FLAG PIC X.\n", encoding="utf-8")
            config = root / "cobol2rust.toml"
            config.write_text(
                """
[converter]
dialect = "ibm_zos"
source_format = "free"
copybook_dirs = ["copybooks"]
""",
                encoding="utf-8",
            )

            module.check(
                str(source),
                "ibm",
                "auto",
                (),
                True,
                str(config),
                None,
            )

            self.assertEqual(calls[0][1], "ibm_zos")
            self.assertEqual(calls[0][2]["source_format"], "free")
            self.assertEqual(calls[0][2]["copybooks"], {"FIELDS.cpy": "01 WS-FLAG PIC X.\n"})
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_check_copybook_policy_can_include_all_files(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()
        calls: list[tuple[str, str, dict]] = []

        def check_cobol(source: str, dialect: str, options: dict) -> dict:
            calls.append((source, dialect, options))
            return {"ok": True, "diagnostics": [], "diagnostics_json": "[]"}

        fake.check_cobol = check_cobol
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("copybook-policy-command")
            source = root / "program.cbl"
            source.write_text("DISPLAY 'OK'.\n", encoding="utf-8")
            copybooks = root / "copybooks"
            copybooks.mkdir(exist_ok=True)
            (copybooks / "README.txt").write_text("01 WS-FROM-TXT PIC X.\n", encoding="utf-8")

            module.check(
                str(source),
                "ibm",
                "auto",
                (str(copybooks),),
                False,
                None,
                None,
                copybook_all_files=True,
                copybook_max_bytes=1024,
            )

            self.assertEqual(calls[0][2]["copybooks"], {"README.txt": "01 WS-FROM-TXT PIC X.\n"})
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_preprocess_uses_shared_config_and_writes_output(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()
        calls: list[tuple[str, dict, str]] = []

        def preprocess(source: str, copybooks: dict, source_format: str = "auto") -> str:
            calls.append((source, copybooks, source_format))
            return source.replace("COPY FIELDS.", copybooks["FIELDS.cpy"])

        fake.preprocess = preprocess
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("preprocess-cli")
            source = root / "program.cbl"
            source.write_text("COPY FIELDS.\n", encoding="utf-8")
            copybooks = root / "copybooks"
            copybooks.mkdir(exist_ok=True)
            (copybooks / "FIELDS.cpy").write_text("01 WS-FLAG PIC X.\n", encoding="utf-8")
            config = root / "cobol2rust.toml"
            config.write_text(
                """
[converter]
source_format = "free"
copybook_dirs = ["copybooks"]
""",
                encoding="utf-8",
            )
            output = root / "reports" / "expanded" / "program.cbl"

            module.preprocess(
                str(source),
                "auto",
                (),
                str(config),
                str(output),
            )

            self.assertEqual(calls[0][1], {"FIELDS.cpy": "01 WS-FLAG PIC X.\n"})
            self.assertEqual(calls[0][2], "free")
            self.assertEqual(output.read_text(encoding="utf-8"), "01 WS-FLAG PIC X.\n\n")
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_convert_json_output_creates_parent_directories(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()

        def convert_project(source: str, dialect: str, output_dir: str, options: dict) -> str:
            return json.dumps(
                {
                    "diagnostics": [],
                    "out_dir": output_dir,
                    "report_path": str(Path(output_dir) / "migration-report.json"),
                    "dialect": dialect,
                    "source_format": options["source_format"],
                    "source_len": len(source),
                }
            )

        fake.convert_project = convert_project
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("convert-json")
            source = root / "program.cbl"
            source.write_text("DISPLAY 'OK'.\n", encoding="utf-8")
            output_dir = root / "generated" / "program"
            report = root / "reports" / "convert" / "program.json"

            module.convert(
                str(source),
                "ibm_zos",
                "free",
                (),
                str(output_dir),
                None,
                str(report),
            )

            payload = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(payload["out_dir"], str(output_dir))
            self.assertEqual(payload["dialect"], "ibm_zos")
            self.assertEqual(payload["source_format"], "free")
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_check_json_output_creates_parent_directories(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()

        def check_cobol(_source: str, _dialect: str, _options: dict) -> dict:
            return {
                "ok": False,
                "diagnostics": [{"severity": "error", "message": "unsupported"}],
                "diagnostics_json": json.dumps(
                    [{"severity": "error", "message": "unsupported"}]
                ),
            }

        fake.check_cobol = check_cobol
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("check-json")
            source = root / "program.cbl"
            source.write_text("ALTER A TO PROCEED TO B.\n", encoding="utf-8")
            output = root / "reports" / "check" / "diagnostics.json"

            module.check(
                str(source),
                "ibm",
                "auto",
                (),
                False,
                None,
                str(output),
            )

            payload = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(payload["ok"], False)
            self.assertEqual(payload["diagnostics"][0]["message"], "unsupported")
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_batch_convert_can_verify_generated_project_builds(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()
        convert_calls: list[tuple[str, str, str, dict]] = []

        root = scratch_root("cli-batch-verify")
        source_dir = root / "cobol"
        source_dir.mkdir(parents=True, exist_ok=True)
        (source_dir / "payroll.cbl").write_text("DISPLAY 'PAY'.\n", encoding="utf-8")
        output_dir = root / "generated"
        summary = root / "reports" / "batch.json"
        summary.parent.mkdir(parents=True, exist_ok=True)

        def convert_project(source: str, dialect: str, output_dir_text: str, options: dict) -> str:
            convert_calls.append((source, dialect, output_dir_text, options))
            Path(output_dir_text).mkdir(parents=True, exist_ok=True)
            return (
                '{"out_dir": "'
                + output_dir_text.replace("\\", "\\\\")
                + '", "generated_files": [], "report_path": "report.json", "diagnostics": []}'
            )

        fake.convert_project = convert_project
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            run_calls: list[tuple[list[str], str]] = []

            def fake_run(command, cwd, text, stdout, stderr, check):
                run_calls.append((list(command), str(cwd)))
                return types.SimpleNamespace(returncode=0, stdout="ok", stderr="")

            module.subprocess.run = fake_run

            module.batch_convert(
                str(source_dir),
                str(output_dir),
                None,
                "ibm",
                "free",
                (),
                str(summary),
                None,
                False,
                True,
            )

            report = __import__("json").loads(summary.read_text(encoding="utf-8"))
            self.assertEqual(report["generated"], 1)
            self.assertEqual(report["failures"], 0)
            self.assertEqual(report["projects"][0]["build"]["passed"], True)
            self.assertEqual(run_calls[0][0], ["cargo", "check", "--offline"])
            self.assertTrue(run_calls[0][1].endswith("generated\\payroll"))
            self.assertEqual(convert_calls[0][1], "ibm")
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_batch_convert_reports_output_directory_collisions(self) -> None:
        previous_click = sys.modules.get("click")
        click_module = fake_click_module()
        sys.modules["click"] = click_module
        fake = types.SimpleNamespace()
        convert_calls: list[tuple[str, str, str, dict]] = []

        root = scratch_root("cli-batch-output-collision")
        source_dir = root / "cobol"
        source_dir.mkdir(parents=True, exist_ok=True)
        (source_dir / "payroll.cbl").write_text("DISPLAY 'PAY'.\n", encoding="utf-8")
        (source_dir / "payroll.cob").write_text("DISPLAY 'ALT'.\n", encoding="utf-8")
        output_dir = root / "generated"
        summary = root / "reports" / "batch.json"

        def convert_project(source: str, dialect: str, output_dir_text: str, options: dict) -> str:
            convert_calls.append((source, dialect, output_dir_text, options))
            Path(output_dir_text).mkdir(parents=True, exist_ok=True)
            return (
                '{"out_dir": "'
                + output_dir_text.replace("\\", "\\\\")
                + '", "generated_files": [], "report_path": "report.json", "diagnostics": []}'
            )

        fake.convert_project = convert_project
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")

            with self.assertRaises(click_module.ClickException):
                module.batch_convert(
                    str(source_dir),
                    str(output_dir),
                    None,
                    "ibm",
                    "free",
                    (),
                    str(summary),
                    None,
                    False,
                    False,
                )

            report = json.loads(summary.read_text(encoding="utf-8"))
            self.assertEqual(report["total"], 2)
            self.assertEqual(report["generated"], 1)
            self.assertEqual(report["failures"], 1)
            self.assertEqual(len(convert_calls), 1)
            self.assertEqual(report["projects"][1]["status"], "failed")
            self.assertEqual(
                report["projects"][1]["diagnostics"][0]["code"],
                "E_BATCH_OUTPUT_COLLISION",
            )
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_batch_check_writes_summary_for_source_tree(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()
        calls: list[tuple[str, str, dict]] = []

        root = scratch_root("batch-check")
        source_dir = root / "cobol"
        source_dir.mkdir(parents=True, exist_ok=True)
        (source_dir / "good.cbl").write_text("DISPLAY 'OK'.\n", encoding="utf-8")
        (source_dir / "bad.cbl").write_text("ALTER A TO PROCEED TO B.\n", encoding="utf-8")
        copybooks = root / "copybooks"
        copybooks.mkdir()
        (copybooks / "FIELDS.cpy").write_text("01 WS-FLAG PIC X.\n", encoding="utf-8")
        config = root / "cobol2rust.toml"
        config.write_text(
            """
[converter]
dialect = "ibm_zos"
source_format = "free"
copybook_dirs = ["copybooks"]
""",
            encoding="utf-8",
        )
        summary = root / "reports" / "batch-check" / "summary.json"

        def check_cobol(source: str, dialect: str, options: dict) -> dict:
            calls.append((source, dialect, options))
            diagnostics = []
            if "ALTER" in source:
                diagnostics = [{"code": "E_ALTER", "message": "ALTER unsupported"}]
            return {
                "ok": not diagnostics,
                "diagnostics": diagnostics,
                "diagnostics_json": json.dumps(diagnostics),
            }

        fake.check_cobol = check_cobol
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            module.batch_check(
                str(source_dir),
                str(config),
                "ibm",
                "auto",
                (),
                str(summary),
                False,
                False,
            )

            report = json.loads(summary.read_text(encoding="utf-8"))
            self.assertEqual(report["total"], 2)
            self.assertEqual(report["ok"], 1)
            self.assertEqual(report["blocked"], 1)
            self.assertEqual(report["failures"], 0)
            self.assertEqual(report["files"][0]["input"], "bad.cbl")
            self.assertEqual(report["files"][0]["diagnostics"][0]["code"], "E_ALTER")
            self.assertEqual(calls[0][1], "ibm_zos")
            self.assertEqual(calls[0][2]["source_format"], "free")
            self.assertEqual(calls[0][2]["copybooks"], {"FIELDS.cpy": "01 WS-FLAG PIC X.\n"})
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_batch_convert_uses_config_copybook_dirs_relative_to_config(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()
        convert_calls: list[tuple[str, str, str, dict]] = []

        root = scratch_root("cli-batch-config-copybooks")
        source_dir = root / "cobol"
        source_dir.mkdir(parents=True, exist_ok=True)
        (source_dir / "payroll.cbl").write_text("COPY FIELDS.\n", encoding="utf-8")
        copybooks = root / "copybooks"
        copybooks.mkdir(exist_ok=True)
        (copybooks / "FIELDS.cpy").write_text("01 WS-FLAG PIC X.\n", encoding="utf-8")
        config = root / "cobol2rust.toml"
        config.write_text(
            """
[converter]
dialect = "ibm_zos"
source_format = "free"
copybook_dirs = ["copybooks"]
""",
            encoding="utf-8",
        )

        def convert_project(source: str, dialect: str, output_dir_text: str, options: dict) -> str:
            convert_calls.append((source, dialect, output_dir_text, options))
            Path(output_dir_text).mkdir(parents=True, exist_ok=True)
            return (
                '{"out_dir": "'
                + output_dir_text.replace("\\", "\\\\")
                + '", "generated_files": [], "report_path": "report.json", "diagnostics": []}'
            )

        fake.convert_project = convert_project
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            summary = root / "reports" / "batch.json"

            module.batch_convert(
                str(source_dir),
                str(root / "generated"),
                str(config),
                "ibm",
                "auto",
                (),
                str(summary),
                None,
                False,
                False,
            )

            self.assertEqual(convert_calls[0][1], "ibm_zos")
            self.assertEqual(convert_calls[0][3]["source_format"], "free")
            self.assertEqual(convert_calls[0][3]["copybooks"], {"FIELDS.cpy": "01 WS-FLAG PIC X.\n"})
            self.assertTrue(summary.exists())
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_doctor_reports_tool_availability_and_docker_fallback(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")

            def fake_which(name: str) -> str | None:
                return {"cargo": "C:/bin/cargo.exe", "docker": "C:/bin/docker.exe"}.get(name)

            previous_find_spec = module.importlib.util.find_spec
            module.shutil.which = fake_which
            module.importlib.util.find_spec = lambda _name: None

            report = module._doctor_report()

            self.assertEqual(report["tools"]["cargo"]["available"], True)
            self.assertEqual(report["tools"]["cargo"]["source"], "path")
            self.assertEqual(report["tools"]["maturin"]["available"], False)
            self.assertEqual(report["tools"]["cobc"]["available"], False)
            self.assertEqual(report["tools"]["docker"]["available"], True)
            self.assertEqual(report["ready"]["build_generated_rust"], True)
            self.assertEqual(report["ready"]["build_python_package"], False)
            self.assertEqual(report["ready"]["oracle_validation"], False)
            self.assertIn("docker build -f docker/python-toolkit/Dockerfile", report["docker"])

            output = scratch_root("doctor") / "doctor.json"
            output.parent.mkdir(parents=True, exist_ok=True)
            module.doctor(str(output))
            self.assertEqual(json.loads(output.read_text(encoding="utf-8")), report)
        finally:
            if "module" in locals():
                module.importlib.util.find_spec = previous_find_spec
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click

    def test_report_commands_create_parent_directories(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()
        fake.analyze_source = lambda _path, _source: json.dumps(
            {"unsupported_features": [{"feature": "ALTER", "advice": "rewrite"}]}
        )
        fake.dependency_graph_dot = lambda _path, _source: "digraph G {}\n"
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("report-parents")
            source = root / "program.cbl"
            source.write_text("DISPLAY 'OK'.\n", encoding="utf-8")

            advisor_output = root / "reports" / "advisor" / "features.json"
            graph_output = root / "reports" / "graphs" / "dependencies.dot"
            oracle_output = root / "reports" / "oracle" / "oracle.json"
            golden_record_output = root / "reports" / "golden" / "record.json"
            golden_output = root / "reports" / "golden" / "compare.json"

            module.advisor(str(source), str(advisor_output))
            module.graph_dot(str(source), str(graph_output))

            def fake_run(command, cwd, text, stdout, stderr, check):
                return types.SimpleNamespace(returncode=0, stdout="ok", stderr="")

            module.subprocess.run = fake_run
            module.oracle_run(str(root), str(oracle_output))

            module.record_golden_output = lambda *_args, **_kwargs: {
                "passed": True,
                "stdout_path": "golden/program.gnucobol.stdout",
                "metadata_path": "golden/program.gnucobol.json",
            }
            module.golden_record(
                str(source),
                str(root / "golden"),
                None,
                "gnucobol",
                "free",
                "cobc",
                str(golden_record_output),
            )

            module.compare_generated_project_to_golden = lambda *_args, **_kwargs: {
                "matched": True,
                "expected_path": "golden/program.gnucobol.stdout",
                "actual_stdout": "OK\n",
                "expected_stdout": "OK\n",
            }
            module.golden_compare(
                str(root / "generated"),
                str(root / "golden" / "program.gnucobol.stdout"),
                (),
                False,
                str(golden_output),
            )

            self.assertEqual(json.loads(advisor_output.read_text(encoding="utf-8"))[0]["feature"], "ALTER")
            self.assertEqual(graph_output.read_text(encoding="utf-8"), "digraph G {}\n")
            self.assertEqual(json.loads(oracle_output.read_text(encoding="utf-8"))["passed"], True)
            self.assertEqual(
                json.loads(golden_record_output.read_text(encoding="utf-8"))["stdout_path"],
                "golden/program.gnucobol.stdout",
            )
            self.assertEqual(json.loads(golden_output.read_text(encoding="utf-8"))["matched"], True)
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_batch_advisor_writes_codebase_refactoring_summary(self) -> None:
        previous_click = sys.modules.get("click")
        sys.modules["click"] = fake_click_module()
        fake = types.SimpleNamespace()
        calls: list[tuple[str, str]] = []

        def analyze_source(path: str, source: str) -> str:
            calls.append((path, source))
            if path.endswith("payroll.cbl"):
                return json.dumps(
                    {
                        "unsupported_features": [
                            {
                                "feature": "ALTER",
                                "capability_id": "procedure.alter",
                                "status": "blocked",
                                "paragraphs": ["P1", "P2"],
                            }
                        ]
                    }
                )
            return json.dumps({"unsupported_features": []})

        fake.analyze_source = analyze_source
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("batch-advisor")
            source_dir = root / "cobol"
            source_dir.mkdir()
            (source_dir / "payroll.cbl").write_text("ALTER P1 TO PROCEED TO P2.\n", encoding="utf-8")
            (source_dir / "clean.cbl").write_text("DISPLAY 'OK'.\n", encoding="utf-8")
            summary = root / "reports" / "advisor" / "summary.json"

            module.batch_advisor(str(source_dir), str(summary), False)

            payload = json.loads(summary.read_text(encoding="utf-8"))
            self.assertEqual(payload["total_files"], 2)
            self.assertEqual(payload["files_with_findings"], 1)
            self.assertEqual(payload["total_findings"], 1)
            self.assertEqual(payload["features"][0]["feature"], "ALTER")
            self.assertEqual(payload["features"][0]["files"], ["payroll.cbl"])
            self.assertEqual(
                calls,
                [
                    ("clean.cbl", "DISPLAY 'OK'.\n"),
                    ("payroll.cbl", "ALTER P1 TO PROCEED TO P2.\n"),
                ],
            )
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous

    def test_batch_advisor_strict_fails_when_findings_exist(self) -> None:
        previous_click = sys.modules.get("click")
        click_module = fake_click_module()
        sys.modules["click"] = click_module
        fake = types.SimpleNamespace()

        def analyze_source(_path: str, _source: str) -> str:
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

        fake.analyze_source = analyze_source
        previous = sys.modules.get("cobol_converter._native")
        sys.modules["cobol_converter._native"] = fake
        sys.modules.pop("cobol_converter.cli", None)
        try:
            module = importlib.import_module("cobol_converter.cli")
            root = scratch_root("batch-advisor-strict")
            source_dir = root / "cobol"
            source_dir.mkdir()
            (source_dir / "payroll.cbl").write_text("ALTER P1 TO PROCEED TO P2.\n", encoding="utf-8")
            summary = root / "reports" / "advisor" / "summary.json"

            with self.assertRaises(click_module.ClickException):
                module.batch_advisor(str(source_dir), str(summary), False, True)

            payload = json.loads(summary.read_text(encoding="utf-8"))
            self.assertEqual(payload["total_files"], 1)
            self.assertEqual(payload["files_with_findings"], 1)
            self.assertEqual(payload["total_findings"], 1)
        finally:
            sys.modules.pop("cobol_converter.cli", None)
            if previous_click is None:
                sys.modules.pop("click", None)
            else:
                sys.modules["click"] = previous_click
            if previous is None:
                sys.modules.pop("cobol_converter._native", None)
            else:
                sys.modules["cobol_converter._native"] = previous


if __name__ == "__main__":
    unittest.main()
