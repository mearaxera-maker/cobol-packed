from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from cobol_converter.migration_project import (
    build_file_map,
    load_migration_config,
    write_generated_file_map,
)


class MigrationProjectTests(unittest.TestCase):
    def scratch(self, name: str) -> Path:
        path = Path(tempfile.mkdtemp(prefix=f"cobol-python-{name}-"))
        path.mkdir(parents=True, exist_ok=True)
        return path

    def test_load_migration_config_reads_converter_and_jcl_style_file_map(self) -> None:
        root = self.scratch("migration-config")
        config = root / "cobol2rust.toml"
        config.write_text(
            """
[converter]
dialect = "ibm_zos"
source_format = "free"
copybook_dirs = ["copybooks", "vendor/copy"]

[file_map]
INFILE = "data/input.dat"
OUTFILE = "data/output.dat"

[[dd]]
name = "SORTWK01"
path = "work/sortwk01.dat"
""",
            encoding="utf-8",
        )

        loaded = load_migration_config(config)

        self.assertEqual(loaded.converter["dialect"], "ibm_zos")
        self.assertEqual(loaded.converter["copybook_dirs"], ["copybooks", "vendor/copy"])
        self.assertEqual(
            loaded.file_map,
            {
                "INFILE": "data/input.dat",
                "OUTFILE": "data/output.dat",
                "SORTWK01": "work/sortwk01.dat",
            },
        )

    def test_write_generated_file_map_materializes_cobol_file_map_json(self) -> None:
        root = self.scratch("file-map")
        project = root / "generated" / "PAYROLL"
        source_root = root / "migration"
        mapping = build_file_map(
            {"INFILE": "data/input.dat", "OUTFILE": str(source_root / "out.dat")},
            source_root=source_root,
        )

        written = write_generated_file_map(project, mapping)

        self.assertEqual(written, project / "cobol-file-map.json")
        self.assertEqual(
            json.loads(written.read_text(encoding="utf-8")),
            {
                "INFILE": str(source_root / "data" / "input.dat"),
                "OUTFILE": str(source_root / "out.dat"),
            },
        )

    def test_write_generated_file_map_uses_atomic_temp_write(self) -> None:
        root = self.scratch("file-map-atomic")
        project = root / "generated" / "PAYROLL"
        output = project / "cobol-file-map.json"
        original_write_text = Path.write_text

        def reject_direct_destination_write(self, *args, **kwargs):
            if self == output:
                raise AssertionError("direct destination write")
            return original_write_text(self, *args, **kwargs)

        Path.write_text = reject_direct_destination_write
        try:
            written = write_generated_file_map(project, {"INFILE": "data/input.dat"})
        finally:
            Path.write_text = original_write_text

        self.assertEqual(written, output)
        self.assertEqual(
            json.loads(output.read_text(encoding="utf-8")),
            {"INFILE": "data/input.dat"},
        )


if __name__ == "__main__":
    unittest.main()
