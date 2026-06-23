# cobol-converter

Python bindings and a CLI wrapper for the Rust COBOL-to-Rust converter.

```bash
pip install cobol-converter
```

For repository development:

```bash
pip install maturin
maturin develop --features extension-module
cobol2rust convert --dialect ibm_zos --output-dir rust_src input.cbl
```

The `cobol_converter` Python package exposes lazy wrappers around the native
extension:

- `cobol_converter.preprocess(source, copybooks=None, source_format="auto")`
- `cobol_converter.convert_cobol(source, dialect, options=None)`
- `cobol_converter.check_cobol(source, dialect, options=None)`
- `cobol_converter.analyze_source(path, source)`
- `cobol_converter.refactoring_advice(path, source)`
- `cobol_converter.refactoring_advice_for_sources(sources)`
- `cobol_converter.dependency_graph_dot(path, source)`
- `cobol_converter.convert_project(source, dialect, output_dir, options=None)`
- `cobol_converter.batch_convert_sources(sources, dialect, output_dir, options=None)`

Installed command-line entry points:

- `cobol2rust`
- `cobol2rust-lsp`

Interactive example:

- `notebooks/interactive_conversion.ipynb` loads COBOL, expands copybooks,
  displays diagnostics, and previews generated Rust from Python.

`convert_cobol` returns a result-shaped dictionary:

```python
{"ok": True, "rust": "...", "diagnostics": []}
{"ok": False, "rust": None, "diagnostics": [{"code": "...", "message": "..."}], "diagnostics_json": "[...]"}
```

`check_cobol` uses the same diagnostic shape but performs validation only:

```python
{"ok": True, "diagnostics": [], "diagnostics_json": "[]"}
{"ok": False, "diagnostics": [{"code": "...", "message": "..."}], "diagnostics_json": "[...]"}
```

CLI examples:

```bash
cobol2rust doctor --json-output reports/doctor.json
cobol2rust preprocess --config cobol2rust.toml --output reports/program.expanded.cbl program.cbl
cobol2rust check --strict input.cbl
cobol2rust check --config cobol2rust.toml --strict --json-output reports/check.json program.cbl
cobol2rust convert --dialect ibm_zos --output-dir rust_src input.cbl
cobol2rust convert --dialect ibm_zos --output-dir generated/program --json-output reports/convert.json program.cbl
cobol2rust batch check --source-dir cobol --config cobol2rust.toml --summary reports/batch-check.json --no-progress
cobol2rust batch convert --source-dir cobol --output-dir generated --file-map-config cobol2rust.toml --summary reports/batch.json --verify-build
cobol2rust advisor program.cbl --output reports/advisor.json
cobol2rust batch advisor --source-dir cobol --summary reports/advisor-summary.json --no-progress --strict
cobol2rust graph dot program.cbl --output reports/dependencies.dot
cobol2rust graph html program.cbl --output reports/dependencies.html
cobol2rust oracle run --repo-root . --json-output reports/oracle.json
cobol2rust golden record program.cbl --golden-dir golden --source-format free --json-output reports/golden-record.json
cobol2rust golden compare generated/program golden/program.gnucobol.stdout
cobol2rust oracle dashboard --report reports/oracle.json --report reports/program-compare.json --output reports/oracle-dashboard.html
```

`batch check` validates a source tree without generating Rust and writes a
per-file diagnostics summary for CI or migration triage.

`batch convert` accepts `[file_map]` and `[[dd]]` entries from `cobol2rust.toml`
and writes a generated `cobol-file-map.json` into each successful project.
It shows a progress bar by default; use `--no-progress` in CI. Add
`--verify-build` to run `cargo check --offline` in each generated project and
record the build result in the batch summary.

`doctor` checks whether Cargo, maturin, GnuCOBOL (`cobc`), and Docker are
available. It accepts maturin either on `PATH` or importable through Python
module execution (`py -m maturin`). It reports readiness for generated Rust
builds, Python package builds, oracle validation, and the Docker fallback
workflow.

`advisor` reports capability-matrix-backed findings with `capability_id`, `status`,
affected paragraphs, and rewrite/validation guidance for migration hazards such as
`ALTER`, `NEXT SENTENCE`, and partial `SEARCH ALL` forms.
Use `batch advisor` or `cobol_converter.refactoring_advice_for_sources(...)` to
scan a source tree or in-memory source mapping. The codebase report includes
`total_files`, `files_with_findings`, `total_findings`, per-file findings, and a
feature rollup listing affected files and paragraphs. Add `--strict` to fail CI when findings are present.

Convenience Docker image:

```bash
docker build -f docker/python-toolkit/Dockerfile -t cobol-converter-python:local .
docker run --rm -it cobol-converter-python:local cobol2rust-run-sample
```

End-to-end onboarding script:

```text
docs/python-migration-video-script.md
```

VS Code/LSP scaffold:

```bash
cd vscode/cobol2rust
# Open this folder in VS Code and run the Extension Development Host.
```

The extension starts `cobol2rust-lsp`, publishes diagnostics for COBOL files,
and provides a generated Rust preview command.

Pytest regression helpers are available from `cobol_converter.pytest_helpers`:

```python
from cobol_converter.pytest_helpers import assert_generated_project_matches_golden


def test_program_matches_reference():
    assert_generated_project_matches_golden(
        "generated/program",
        "golden/program.gnucobol.stdout",
    )
```

Pass `runner=` to inject a subprocess-compatible command runner for dry-run or
unit-style pytest coverage without invoking Cargo.
