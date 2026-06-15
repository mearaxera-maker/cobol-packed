# COBOL-To-Rust Converter Preview

`cobol2rust` is the first compiler-shaped layer beside the existing
`cobol-packed` forensic record CLI. It is intentionally conservative: generated
Rust is runtime-backed, and unsupported COBOL produces a migration report rather
than best-effort code.

This is a preview, not a production COBOL compiler. The goal of this layer is
to make unsupported behavior explicit while the real compiler front end grows.

## Command

```text
cargo run --features converter --bin cobol2rust -- convert \
  --input program.cbl \
  --copybook-dir copybooks \
  --out generated-rust \
  --dialect ibm \
  --source-format fixed
```

## Pipeline

```text
COBOL source
  -> cobol-source: fixed/free normalization and limited COPY expansion
  -> cobol-syntax: lexer, lossless token tree, typed AST
  -> cobol-sema: symbols, Data Division facts, statement lowering
  -> cobol-ir: storage, paragraph, statement, and diagnostic model
  -> cobol-codegen-rust: standalone Rust project generation
  -> cobol-runtime: runtime support for the generated subset
```

The generated project contains:

- `Cargo.toml`
- `src/main.rs`
- `src/data.rs`
- `src/files.rs`
- `src/program.rs`
- `migration-report.json`

## Current Supported Subset

- Source formats: fixed, free, and auto detection for `>>SOURCE FORMAT FREE`.
- COPY expansion: local copybooks with recursion and depth limits; `COPY
  REPLACING` is rejected instead of ignored.
- Data Division: elementary/group declaration discovery for simple storage.
  Hard layout clauses such as `REDEFINES`, `OCCURS`, `USAGE`, `COMP-*`,
  `VALUE`, `SIGN`, `SYNC`, `RENAMES`, and index clauses are blocked until the
  converter has a real Data Division memory model.
- Procedure Division: paragraphs, `DISPLAY`, `MOVE`, basic arithmetic
  statements, simple `PERFORM`, `GO TO`, and `STOP RUN`.
- Diagnostics: unsupported statements and not-yet-codegenerated IR produce hard
  errors before Rust is emitted.
- Symbol checks: unresolved data references and unresolved paragraph targets
  block generation.
- Generated projects vendor `cobol-runtime` under `vendor/cobol-runtime` so the
  output can compile without referring back to the converter workspace.

## Explicit Boundaries

The converter preview does not yet claim full COBOL language compatibility.
These constructs are blocked or reported until the corresponding semantic layer
is implemented:

- EXEC SQL, CICS, and DLI.
- COPY REPLACING.
- FILE SECTION, FILE-CONTROL, SELECT/FD/SD, and unbound file adapters.
- Data clauses that require byte layout or initialization semantics.
- SORT/MERGE and report writer.
- ALTER and dynamic CALL.
- Full IF/EVALUATE lowering.
- PERFORM THRU lowering.
- IBM/Micro Focus dialect edge cases that require oracle traces.

## Validation

Local validation currently covers:

- workspace tests for source normalization, parsing, semantic lowering, and
  backend smoke generation;
- `cobol2rust` integration tests for successful generation and unsupported
  EXEC SQL blocking;
- generated hello-world Rust project compile/run smoke checks.

GnuCOBOL oracle comparison is the next CI layer: supported fixtures should run
through both GnuCOBOL and generated Rust, then compare stdout, file output,
status, and selected memory traces.
