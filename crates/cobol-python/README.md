# cobol-converter

Python bindings and a CLI wrapper for the Rust COBOL-to-Rust converter.

```bash
pip install maturin
maturin develop --features extension-module
cobol2rust convert --dialect ibm --output-dir rust_src program.cbl
```

The extension module exposes:

- `cobol2rust.preprocess(source, copybooks=None, source_format="auto")`
- `cobol2rust.convert_cobol(source, dialect, options=None)`
- `cobol2rust.analyze_source(path, source)`
- `cobol2rust.dependency_graph_dot(path, source)`
- `cobol2rust.convert_project(source, dialect, output_dir, options=None)`
- `cobol2rust.batch_convert_sources(sources, dialect, output_dir, options=None)`

`convert_cobol` returns a result-shaped dictionary:

```python
{"ok": True, "rust": "...", "diagnostics": []}
{"ok": False, "rust": None, "diagnostics_json": "[...]"}
```

CLI examples:

```bash
cobol2rust check --strict program.cbl
cobol2rust convert --dialect ibm --output-dir generated/program program.cbl
cobol2rust batch convert --source-dir cobol --output-dir generated --summary reports/batch.json
cobol2rust advisor program.cbl --output reports/advisor.json
cobol2rust graph dot program.cbl --output reports/dependencies.dot
cobol2rust oracle run --repo-root . --json-output reports/oracle.json
cobol2rust golden record program.cbl --golden-dir golden --source-format free
cobol2rust golden compare generated/program golden/program.gnucobol.stdout
cobol2rust oracle dashboard --report reports/oracle.json --report reports/program-compare.json --output reports/oracle-dashboard.html
```

Pytest regression helpers are available from `cobol_converter.pytest_helpers`:

```python
from cobol_converter.pytest_helpers import assert_generated_project_matches_golden


def test_program_matches_reference():
    assert_generated_project_matches_golden(
        "generated/program",
        "golden/program.gnucobol.stdout",
    )
```
