# COBOL-To-Rust Converter

`cobol2rust` is the first compiler-shaped layer beside the existing
`cobol-packed` forensic record CLI. It is intentionally conservative: generated
Rust is runtime-backed, and unsupported COBOL produces a migration report rather
than best-effort code.

This is still not a full mainframe rehosting platform. The goal of this layer is
to execute the supported procedural batch subset exactly, report unsupported
behavior precisely, and leave platform-specific behavior such as JCL file
disposition, VSAM catalogs, and EBCDIC dataset conversion to the runtime
platform layer.

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
  -> cobol-source: fixed/free normalization, COPY expansion, COPY REPLACING
  -> cobol-syntax: lexer, lossless token tree, typed AST
  -> cobol-sema: symbols, PIC facts, storage areas, value categories, resolved references, CFG, statement lowering
  -> cobol-record: shared byte layout facts, coverage, and codec helpers
  -> cobol-ir: byte storage, record plan, paragraph, control-flow, and diagnostics
  -> cobol-codegen-rust: standalone Rust project generation
  -> cobol-platform: structured runtime file binding configuration
  -> cobol-runtime: runtime support for the generated subset
```

The generated project contains:

- `Cargo.toml`
- `src/main.rs`
- `src/data.rs`
- `src/files.rs`
- `src/program.rs`
- `vendor/cobol-dialect`
- `vendor/cobol-platform`
- `vendor/cobol-record`
- `vendor/cobol-runtime`
- `vendor/cobol-vm`
- `migration-report.json`

Generated programs can load file bindings from structured runtime config:

```text
cargo run -- --runtime-config cobol-runtime.json
```

`cobol-runtime.json` currently supports fixed-length sequential files only:

```json
{
  "files": [
    {
      "name": "INFILE",
      "path": "input.dat",
      "organization": "sequential",
      "record_format": { "kind": "fixed", "record_len": 80 },
      "disposition": "old",
      "encoding": "ascii"
    }
  ]
}
```

The `name` is the generated COBOL file name, not a JCL DD catalog entry. The
legacy flat `--file-map path` behavior remains supported. When no runtime flag
is provided, generated programs check `cobol-runtime.json` first, then
`cobol-file-map.json`.

Runtime config v1 validates before execution: duplicate normalized file names,
empty/padded names, empty paths, non-ASCII encodings, non-fixed record formats,
and indexed/relative/VSAM organizations fail closed. Disposition is enforced at
`OPEN`: `old`/`shr` require existing input or I-O use, `new` requires output
create/truncate use, and `mod` requires extend/append use.

## Current Supported Subset

The canonical feature inventory is `migration-capability-matrix.json`; see
`docs/migration-capability-matrix.md` for status definitions and current counts.
The list below is a narrative overview and must not be treated as the source of
truth when tests or implementation move.

- Source formats: fixed, free, and auto detection for `>>SOURCE FORMAT FREE`.
- COPY expansion: local copybooks with recursion and depth limits; simple
  pseudo-text `COPY ... REPLACING ==old== BY ==new==` pairs are applied
  outside string literals, case-insensitively for COBOL words, and without
  substring replacement inside larger identifiers.
- Data Division: groups, elementary items, PIC parsing, VALUE initialization,
  legal repeated FILLER storage, fixed OCCURS sizing, scoped REDEFINES overlay
  metadata, COMP/COMP-3/COMP-5/floats, DISPLAY/alphanumeric byte lengths,
  Working-Storage/Linkage/File Section area tagging, 88-level condition names,
  qualified references, and SYNC offset planning are lowered into byte-backed
  storage IR.
- Procedure Division: paragraphs, `DISPLAY`, `MOVE`, basic arithmetic
  statements, `PERFORM` variants, `GO TO`, `IF`, `EVALUATE`, serial `SEARCH`,
  narrow executable `SEARCH ALL`, static `CALL`, linked dynamic `CALL`, `GOBACK`,
  and `STOP RUN`.
- Procedure AST V1/V2: supported scoped branches for file verbs,
  procedure-based `SORT`/`RETURN`, `INSPECT`/`EXAMINE`, `STRING`/`UNSTRING`,
  `COMPUTE ... ON SIZE ERROR`, `EVALUATE`, and `SEARCH` are token-slice parsed
  and lowered as typed imperative lists instead of reparsed raw strings. Top-level
  Procedure Division sentences use the same token-slice parser, and the migration
  report emits sentence-scoped procedure CFG blocks.
- Reference IR: unqualified names, qualified `A OF B`, condition names, basic
  subscripts, and reference modification are resolved in semantic analysis;
  unsupported forms block from structured diagnostics instead of being flattened
  into strings.
- Condition IR: `IF` conditions are parsed into relation, class-test, sign-test,
  condition-name, `NOT`, `AND`, and `OR` trees with COBOL precedence. Basic
  abbreviated relations such as `A > B AND < C` are expanded against the last
  explicit subject before semantic resolution. Condition-name evaluation and
  `SET condition TO TRUE` use declared views so REDEFINES and ODO aliasing do
  not change the condition's declared parent layout.
- Value semantics: fields carry compiler-level categories such as group,
  alphanumeric, numeric DISPLAY, numeric edited, packed decimal, binary,
  native binary, float, and condition name. MOVE/arithmetic category mismatches
  fail before Rust is emitted.
- Diagnostics: unsupported statements and not-yet-codegenerated IR produce hard
  errors before Rust is emitted.
- Symbol checks: unresolved data references and unresolved paragraph targets
  block generation.
- Generated projects vendor `cobol-runtime`, `cobol-record`, `cobol-platform`,
  `cobol-dialect`, and `cobol-vm`. Generated
  storage is byte-backed for supported display/alphanumeric procedure access.
  Generated `DataView` accessors use shared codec helpers for packed decimal,
  binary, and IBM float fields, and `DISPLAY` can route through those typed
  accessors when no OCCURS/REDEFINES/dynamic layout ambiguity is present.
  Fixed scalar COMP-3 fields also support strict generated `MOVE` and
  `COMPUTE` writes through the VM packed-decimal encoder, including
  `ON SIZE ERROR` protection for out-of-range compute results. Procedure-based
  `SORT` can order fixed scalar COMP-3 keys numerically rather than by packed
  storage bytes.
- Multi-program runtime support includes structural `LINKAGE`, scalar
  conversion temporaries for `CALL USING`, EXTERNAL storage sharing,
  COMMON/INITIAL lifecycle vectors, `PROGRAM-STATUS` for dynamic-call failure,
  and OS-backed fixed-length sequential files through generated file maps or
  structured runtime platform config.
- Procedure-generation guardrails reject unsupported statements, `NEXT
  SENTENCE`, and unlowered `SEARCH ALL` / `MOVE CORRESPONDING` fallback paths
  before Rust is emitted. File I/O statements lower to typed semantic IR only;
  the legacy raw file-I/O IR variants have been removed.

## Explicit Boundaries

Feature status and blocker evidence are tracked in
`migration-capability-matrix.json`. This section is a readable boundary summary.

The converter does not claim full COBOL language or platform compatibility.
These constructs are blocked or reported until the corresponding semantic layer
is implemented:

- EXEC SQL, CICS, and DLI.
- LINE SEQUENTIAL, indexed, relative, VSAM, variable-length files, and FD
  records with ODO.
- `OPEN I-O`, `REWRITE`, `DELETE`, and statement-specific `INVALID KEY`
  branches are supported only for the current fixed-length sequential slice.
- Dynamic `ASSIGN TO identifier`; current OS file mapping is configured at
  runtime through generated file maps, structured runtime platform config for
  static fixed sequential files, or literal ASSIGN fallback.
- EBCDIC/binary runtime dataset translation is not implemented in v1 runtime
  config; non-ASCII `encoding` values parse but fail validation.
- Multiple-key and non-equality `SEARCH ALL` forms.
- True separately loaded dynamic CALL targets; current dynamic CALL dispatches
  among programs linked into the generated binary.
- MOVE/arithmetic cases that require dialect-specific edited/national/DBCS
  semantics not yet implemented.
- Data clauses not yet modeled: RENAMES, SIGN SEPARATE, JUSTIFIED, BLANK
  WHEN ZERO, GLOBAL, POINTER, and PROCEDURE-POINTER.
- Procedure-based in-memory `SORT` with fixed-length `SD` records is supported
  for the current single-key callback slice; physical `USING` / `GIVING`,
  external sort utilities, `MERGE`, multiple keys, and custom collation remain
  out of scope.
- `ALTER` and computed `GO TO DEPENDING ON` are supported in the current VM
  paragraph-resolution model; overlay-aware branch targets remain future work.
- `NEXT SENTENCE` targets are represented in the sentence-scoped CFG report, but
  execution remains fail-closed until generated VM lowering has complete
  period-scope semantics; it must not be emitted as a generated VM `Noop`.
- Figurative `ALL`, collating-sequence comparisons, and dialect-specific
  implicit-subject edge cases that still need oracle traces.
- IBM/Micro Focus dialect edge cases that require oracle traces.

## Validation

Local validation currently covers:

- workspace tests for source normalization, COPY REPLACING, parsing, semantic
  storage lowering, CFG metadata, and backend smoke generation;
- `cobol2rust` integration tests for successful generation, generated project
  compile checks, unsupported EXEC SQL blocking, hostile COBOL blocking,
  semantic reference evidence, category mismatch blocking, file I/O, lifecycle,
  EXTERNAL, SEARCH ALL, ODO READ INTO, and dynamic CALL behavior;
- generated hello-world and Data Division Rust project compile smoke checks;
- optional GnuCOBOL oracle fixtures for a very small observable-behavior slice.
  Most converter smoke assertions are still implementation/regression tests, not
  independent migration oracles.

See `docs/installation.md` for local verification scripts and
`docs/oracle_validation.md` for the oracle fixture model.
