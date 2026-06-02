# Python Converter Interface

This package is intended for migration engineers who need the COBOL converter without writing Rust.

## Install For Local Development

From the repository root:

```bash
cd crates/cobol-python
pip install maturin
maturin develop --features extension-module
```

The install provides:

- `cobol2rust`, a Python extension module backed by the Rust converter.
- `cobol2rust`, a command-line script from `cobol_converter.cli`.

## Library Use

```python
import cobol2rust

source = """
IDENTIFICATION DIVISION.
PROGRAM-ID. HELLOPY.
PROCEDURE DIVISION.
MAIN.
    DISPLAY "HELLO".
    STOP RUN.
"""

result = cobol2rust.convert_cobol(
    source,
    "ibm",
    {"source_format": "free", "copybooks": {}},
)

if result["ok"]:
    print(result["rust"][:400])
else:
    print(result["diagnostics_json"])
```

`preprocess` can be used separately:

```python
expanded = cobol2rust.preprocess(
    "DATA DIVISION.\nCOPY FIELDS.\n",
    {"FIELDS.cpy": "01 WS-FLAG PIC X VALUE \"Y\".\n"},
    source_format="free",
)
```

## CLI Use

Convert one source file into a complete generated Rust project:

```bash
cobol2rust convert --dialect ibm --source-format free --output-dir rust_src program.cbl
```

Convert every COBOL source under a directory into generated project subdirectories:

```bash
cobol2rust batch convert \
  --source-dir cobol \
  --copybook-dir copybooks \
  --output-dir generated \
  --summary reports/batch-summary.json
```

Both commands can read shared settings from `cobol2rust.toml`:

```toml
[converter]
dialect = "ibm"
source_format = "auto"
copybook_dirs = ["copybooks"]
```

Run validation and print diagnostics:

```bash
cobol2rust check --strict --dialect ibm program.cbl
```

Report unsupported migration patterns such as `ALTER`:

```bash
cobol2rust advisor program.cbl --output reports/advisor.json
```

Create a Graphviz DOT dependency graph from `COPY` and `CALL` references:

```bash
cobol2rust graph dot program.cbl --output reports/dependencies.dot
```

Run the Rust oracle suite through the Python CLI and capture a JSON summary:

```bash
cobol2rust oracle run --repo-root . --json-output reports/oracle.json
```

Record golden stdout from a reference GnuCOBOL run:

```bash
cobol2rust golden record cobol/program.cbl \
  --golden-dir golden \
  --dialect gnucobol \
  --source-format free
```

Compare a generated Rust project against the recorded stdout:

```bash
cobol2rust golden compare generated/program golden/program.gnucobol.stdout \
  --json-output reports/program-compare.json
```

Generate a standalone HTML dashboard from oracle and golden-compare reports:

```bash
cobol2rust oracle dashboard \
  --report reports/oracle.json \
  --report reports/program-compare.json \
  --output reports/oracle-dashboard.html
```

Python tests can use the pytest helper:

```python
from cobol_converter.pytest_helpers import assert_generated_project_matches_golden


def test_program_matches_reference():
    assert_generated_project_matches_golden(
        "generated/program",
        "golden/program.gnucobol.stdout",
    )
```

Scaffold a migration workspace:

```bash
cobol2rust init-migration my-migration
```

The scaffold creates:

- `cobol/`
- `copybooks/`
- `golden/`
- `generated/`
- `reports/`
- `cobol2rust.toml`

## Current Limitations

This is an early Python-accessible slice. It exposes in-memory conversion, complete single-file project generation, basic batch project generation, a dependency graph primitive, refactoring advice for `ALTER`, a basic oracle command wrapper, golden stdout recording, pytest comparison helpers, and standalone oracle dashboard generation. The full objective still needs Graphviz rendering helpers, Docker/GnuCOBOL setup, and optional VS Code tooling.
