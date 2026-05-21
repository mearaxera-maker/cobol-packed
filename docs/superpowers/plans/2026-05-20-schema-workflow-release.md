# Schema Workflow Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add HostLens schema workflow commands for limited copybook import, Rust schema scaffolding, and semantic schema comparison.

**Architecture:** Keep `src/cli/mod.rs` as dispatch glue. Add three focused CLI modules: `copybook.rs` parses a safe copybook subset into existing schema v2 structs, `schema_emit.rs` renders Rust source from validated schemas, and `schema_compare.rs` produces semantic diffs. Reuse `schema.rs` validation so generated and hand-written schemas follow the same runtime rules.

**Tech Stack:** Rust 2021, Clap subcommands, Serde JSON/TOML, existing HostLens CLI/error/render patterns, no new runtime dependencies for this release.

---

### Task 1: CLI Argument Surface

**Files:**
- Modify: `src/cli/args.rs`
- Modify: `src/cli/mod.rs`
- Test: `tests/cli_smoke.rs`

- [ ] **Step 1: Write failing help tests**

Add this test near the schema CLI tests in `tests/cli_smoke.rs`:

```rust
#[test]
fn schema_workflow_commands_are_listed_in_help() {
    let mut cmd = hostlens();
    cmd.args(["schema", "--help"]);
    cmd.assert()
        .success()
        .stdout(contains("check"))
        .stdout(contains("from-copybook"))
        .stdout(contains("emit-rust"))
        .stdout(contains("compare"));
}
```

- [ ] **Step 2: Run the test to verify RED**

Run:

```text
cargo test --features cli --test cli_smoke schema_workflow_commands_are_listed_in_help
```

Expected: fail because the new subcommands do not exist.

- [ ] **Step 3: Add CLI structs**

In `src/cli/args.rs`, import the extra enums:

```rust
use super::{
    CliSignMode, CompareFailOn, CompareOutputFormat, DiagnosticFormat, EncodingListFormat,
    EvidenceMode, InputEncoding, OnError, OutputFormat, RustDerive, RustVisibility,
    VerificationScope,
};
```

Extend `SchemaCommand`:

```rust
#[derive(Subcommand)]
pub(super) enum SchemaCommand {
    Check(SchemaArgs),
    FromCopybook(CopybookArgs),
    EmitRust(EmitRustArgs),
    Compare(CompareArgs),
}
```

Add argument structs:

```rust
#[derive(Args)]
pub(super) struct CopybookArgs {
    #[arg(long)]
    pub(super) copybook: PathBuf,
    #[arg(long)]
    pub(super) record_name: Option<String>,
    #[arg(long)]
    pub(super) encoding: String,
    #[arg(long, value_enum, default_value_t = InputEncoding::Binary)]
    pub(super) input_encoding: InputEncoding,
    #[arg(long, value_enum, default_value_t = OnError::EmitErrorRow)]
    pub(super) on_error: OnError,
    #[arg(long, value_enum, default_value_t = VerificationScope::Field)]
    pub(super) verification_scope: VerificationScope,
    #[arg(long)]
    pub(super) output: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = DiagnosticFormat::Text)]
    pub(super) diagnostics: DiagnosticFormat,
    #[arg(long)]
    pub(super) strict: bool,
    #[arg(long, conflicts_with = "drop_fillers")]
    pub(super) include_fillers: bool,
    #[arg(long, conflicts_with = "include_fillers")]
    pub(super) drop_fillers: bool,
}

#[derive(Args)]
pub(super) struct EmitRustArgs {
    #[arg(long)]
    pub(super) schema: PathBuf,
    #[arg(long)]
    pub(super) struct_name: String,
    #[arg(long)]
    pub(super) module_name: Option<String>,
    #[arg(long)]
    pub(super) output: Option<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    pub(super) derive: Vec<RustDerive>,
    #[arg(long)]
    pub(super) raw_slices: bool,
    #[arg(long, value_enum, default_value_t = RustVisibility::Pub)]
    pub(super) visibility: RustVisibility,
}

#[derive(Args)]
pub(super) struct CompareArgs {
    #[arg(long)]
    pub(super) old: PathBuf,
    #[arg(long)]
    pub(super) new: PathBuf,
    #[arg(long, value_enum, default_value_t = CompareOutputFormat::Table)]
    pub(super) output: CompareOutputFormat,
    #[arg(long, value_enum, default_value_t = CompareFailOn::Any)]
    pub(super) fail_on: CompareFailOn,
    #[arg(long)]
    pub(super) ignore_order: bool,
    #[arg(long)]
    pub(super) show_unchanged: bool,
}
```

- [ ] **Step 4: Add value enums**

In `src/cli/mod.rs`, add:

```rust
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum DiagnosticFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum CompareOutputFormat {
    Table,
    Json,
    Jsonl,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum CompareFailOn {
    Warning,
    Breaking,
    Any,
    Never,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum RustDerive {
    Debug,
    Clone,
    Serde,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum RustVisibility {
    Pub,
    PubCrate,
    Private,
}
```

Add `ValueEnum` to `InputEncoding`, `OnError`, and `VerificationScope` derives if missing. `VerificationScope` already has it.

- [ ] **Step 5: Add dispatch stubs**

In `src/cli/mod.rs`, extend schema dispatch:

```rust
Command::Schema { command } => match command {
    SchemaCommand::Check(args) => schema_check(args),
    SchemaCommand::FromCopybook(args) => copybook::from_copybook(args),
    SchemaCommand::EmitRust(args) => schema_emit::emit_rust(args),
    SchemaCommand::Compare(args) => schema_compare::compare(args),
},
```

Create minimal modules with stub functions returning `CliError::internal("not implemented")`:

```rust
mod copybook;
mod schema_compare;
mod schema_emit;
```

- [ ] **Step 6: Run help test to verify GREEN**

Run:

```text
cargo test --features cli --test cli_smoke schema_workflow_commands_are_listed_in_help
```

Expected: pass.

### Task 2: Copybook Generation Happy Path

**Files:**
- Create: `src/cli/copybook.rs`
- Modify: `src/cli/schema.rs`
- Test: `tests/cli_smoke.rs`

- [ ] **Step 1: Write failing generation test**

Add:

```rust
#[test]
fn schema_from_copybook_generates_schema_v2_for_mixed_record() {
    let dir = tempfile::tempdir().unwrap();
    let copybook = dir.path().join("customer.cpy");
    fs::write(
        &copybook,
        r#"
       01 CUSTOMER-REC.
          05 ACCOUNT-ID     PIC X(4).
          05 AMOUNT         PIC S9(5)V99 COMP-3.
          05 TAX            PIC S9(3)V99.
          05 SEQUENCE-NO    PIC 9(9) COMP.
          05 FILLER         PIC X(2).
        "#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "from-copybook",
        "--copybook",
        copybook.to_str().unwrap(),
        "--encoding",
        "cp037",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["version"], 2);
    assert_eq!(json["record_length"], 19);
    assert_eq!(json["fields"][0]["name"], "account_id");
    assert_eq!(json["fields"][0]["field_type"], "display-text");
    assert_eq!(json["fields"][0]["offset"], 0);
    assert_eq!(json["fields"][1]["name"], "amount");
    assert_eq!(json["fields"][1]["field_type"], "packed-decimal");
    assert_eq!(json["fields"][1]["length"], 4);
    assert_eq!(json["fields"][2]["field_type"], "zoned-decimal");
    assert_eq!(json["fields"][2]["offset"], 8);
    assert_eq!(json["fields"][3]["field_type"], "binary");
    assert_eq!(json["fields"][3]["length"], 4);
    assert_eq!(json["fillers"][0]["offset"], 17);
    assert_eq!(json["fillers"][0]["length"], 2);
}
```

- [ ] **Step 2: Run the test to verify RED**

Run:

```text
cargo test --features cli --test cli_smoke schema_from_copybook_generates_schema_v2_for_mixed_record
```

Expected: fail because `from-copybook` stub returns not implemented.

- [ ] **Step 3: Make schema structs constructible**

In `src/cli/schema.rs`, keep fields `pub(super)` and add constructor helpers only if direct construction from `copybook.rs` becomes noisy:

```rust
pub(super) fn validate_generated_schema(schema: &Schema) -> Result<(), CliError> {
    validate_schema(schema)
}
```

- [ ] **Step 4: Implement copybook parser skeleton**

Create `src/cli/copybook.rs` with:

```rust
use super::*;

#[derive(Debug)]
struct CopybookItem {
    level: u8,
    name: String,
    pic: Option<String>,
    usage: Usage,
    line: usize,
    source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Usage {
    Display,
    PackedDecimal,
    Binary,
}

#[derive(Debug)]
struct Picture {
    kind: PictureKind,
    total_digits: Option<u8>,
    scale: u8,
    signed: bool,
    display_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PictureKind {
    Text,
    Numeric,
}
```

Implement `from_copybook(args: CopybookArgs) -> Result<(), CliError>`:

1. parse `args.encoding` through `TextEncoding::from_label`;
2. reject mixed DBCS encodings;
3. read file as UTF-8;
4. parse declarations;
5. select record;
6. map items to `Schema`;
7. call `validate_schema`;
8. serialize pretty JSON to stdout or `args.output`.

- [ ] **Step 5: Implement declaration parsing**

Implement:

```rust
fn declarations(text: &str) -> Vec<(usize, String)> {
    // trim comments, collect lines until '.', preserve starting line
}

fn parse_item(line: usize, declaration: &str) -> Result<CopybookItem, CliError> {
    // split whitespace, first token level, second name, find PIC/PICTURE,
    // detect usage tokens
}
```

Reject unsupported tokens by checking uppercased declaration for:

```rust
[" REDEFINES ", " OCCURS ", " SYNCHRONIZED", " SYNC", " JUSTIFIED",
 " SIGN IS SEPARATE", " BLANK WHEN ZERO", " COPY "]
```

Reject level `66`, `77`, and `88`.

- [ ] **Step 6: Implement picture parser**

Implement:

```rust
fn parse_picture(raw: &str) -> Result<Picture, String>
```

Rules:

- remove spaces and trailing period;
- `X(n)` and `A(n)` produce text length `n`;
- repeated `X`/`A` produce text length count;
- optional leading `S` sets `signed`;
- split on `V`;
- count `9(n)` and repeated `9`;
- scale is right side of `V`;
- total digits is left plus right;
- reject edited characters.

- [ ] **Step 7: Implement layout mapping**

For each elementary item after the selected level 01, compute `let keep_fillers = !args.drop_fillers;`. `--include-fillers` is accepted for explicitness but does not need special handling because fillers are included by default.

- skip group items with no `PIC`;
- for text `FILLER`, create `FillerSpec` when fillers are included;
- text non-filler creates `FieldSpec { field_type: DisplayText, encoding: Some(encoding), length: Some(display_len) }`;
- display numeric creates `ZonedDecimal`;
- packed usage creates `PackedDecimal`;
- binary usage creates `Binary` with IBM width by digit count.

Use:

```rust
fn packed_len(digits: u8) -> usize { (digits as usize + 2) / 2 }
fn binary_len(digits: u8) -> Result<usize, CliError> {
    match digits {
        1..=4 => Ok(2),
        5..=9 => Ok(4),
        10..=18 => Ok(8),
        _ => Err(CliError::config("E_SCHEMA", "binary COMP supports 1..=18 digits")),
    }
}
```

- [ ] **Step 8: Run test to verify GREEN**

Run:

```text
cargo test --features cli --test cli_smoke schema_from_copybook_generates_schema_v2_for_mixed_record
```

Expected: pass.

### Task 3: Copybook Diagnostics And Record Selection

**Files:**
- Modify: `src/cli/copybook.rs`
- Test: `tests/cli_smoke.rs`

- [ ] **Step 1: Write failing unsupported construct tests**

Add:

```rust
#[test]
fn schema_from_copybook_rejects_unsupported_constructs_with_line_numbers() {
    let cases = [
        ("REDEFINES", "          05 ALT-AMOUNT REDEFINES AMOUNT PIC X(4)."),
        ("OCCURS", "          05 ITEM PIC X(2) OCCURS 3 TIMES."),
        ("SYNC", "          05 BIN-FIELD PIC 9(4) COMP SYNC."),
        ("level 88", "          88 ACTIVE VALUE 'Y'."),
    ];

    for (expected, line) in cases {
        let dir = tempfile::tempdir().unwrap();
        let copybook = dir.path().join("bad.cpy");
        fs::write(
            &copybook,
            format!("       01 BAD-REC.\n          05 AMOUNT PIC 9(4).\n{line}\n"),
        )
        .unwrap();
        let mut cmd = hostlens();
        cmd.args([
            "schema",
            "from-copybook",
            "--copybook",
            copybook.to_str().unwrap(),
            "--encoding",
            "cp037",
        ]);
        cmd.assert()
            .code(2)
            .stderr(contains(expected))
            .stderr(contains("line"));
    }
}
```

- [ ] **Step 2: Run test to verify RED**

Run:

```text
cargo test --features cli --test cli_smoke schema_from_copybook_rejects_unsupported_constructs_with_line_numbers
```

Expected: fail until diagnostics are stable.

- [ ] **Step 3: Implement diagnostics**

Add:

```rust
#[derive(Debug, Serialize)]
struct CopybookDiagnostic {
    severity: &'static str,
    code: &'static str,
    line: usize,
    field_name: Option<String>,
    message: String,
    help: String,
    source: String,
}
```

When returning a blocking diagnostic, render text or JSON to stderr according to `args.diagnostics`, then return `CliError::config("E_SCHEMA", "...")`.

- [ ] **Step 4: Add multiple-record selection test**

Add:

```rust
#[test]
fn schema_from_copybook_requires_record_name_for_multiple_01_records() {
    let dir = tempfile::tempdir().unwrap();
    let copybook = dir.path().join("multi.cpy");
    fs::write(
        &copybook,
        "       01 FIRST-REC.\n          05 A PIC X(1).\n       01 SECOND-REC.\n          05 B PIC X(1).\n",
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "from-copybook",
        "--copybook",
        copybook.to_str().unwrap(),
        "--encoding",
        "cp037",
    ]);
    cmd.assert().code(2).stderr(contains("--record-name"));
}
```

- [ ] **Step 5: Implement record selection**

Track all level 01 declarations. If more than one exists and `record_name` is absent, return `E_SCHEMA`. If `record_name` is present, select declarations after that 01 until the next level 01.

- [ ] **Step 6: Run diagnostics tests**

Run:

```text
cargo test --features cli --test cli_smoke schema_from_copybook_rejects_unsupported_constructs_with_line_numbers schema_from_copybook_requires_record_name_for_multiple_01_records
```

If Cargo rejects multiple filters, run the full CLI smoke suite.

### Task 4: Rust Schema Emission

**Files:**
- Create: `src/cli/schema_emit.rs`
- Modify: `src/cli/mod.rs`
- Test: `tests/cli_smoke.rs`

- [ ] **Step 1: Write failing emit-rust test**

Add:

```rust
#[test]
fn schema_emit_rust_generates_struct_and_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 7,
          "input_encoding": "binary",
          "fields": [{
            "name": "account-id",
            "field_type": "display-text",
            "offset": 0,
            "length": 4,
            "encoding": "cp037"
          }, {
            "name": "amount",
            "field_type": "packed-decimal",
            "offset": 4,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true
          }]
        }"#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "emit-rust",
        "--schema",
        schema.to_str().unwrap(),
        "--struct-name",
        "CustomerRecord",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("pub const RECORD_LEN: usize = 7;"))
        .stdout(contains("pub const ACCOUNT_ID_OFFSET: usize = 0;"))
        .stdout(contains("pub struct CustomerRecord"))
        .stdout(contains("pub account_id: String"))
        .stdout(contains("pub amount: String"))
        .stdout(predicates::str::contains("repr(C)").not())
        .stdout(predicates::str::contains("unsafe").not());
}
```

- [ ] **Step 2: Run test to verify RED**

Run:

```text
cargo test --features cli --test cli_smoke schema_emit_rust_generates_struct_and_offsets
```

Expected: fail because `emit-rust` is a stub.

- [ ] **Step 3: Implement `schema_emit.rs`**

Implement:

```rust
pub(super) fn emit_rust(args: EmitRustArgs) -> Result<(), CliError> {
    let (schema, _) = load_schema(&args.schema)?;
    let source = render_rust(&schema, &args)?;
    if let Some(path) = args.output {
        fs::write(path, source)?;
    } else {
        print!("{source}");
    }
    Ok(())
}
```

Add helpers:

- `validate_type_name(name: &str) -> Result<(), CliError>`;
- `rust_field_ident(name: &str) -> String`;
- `rust_const_ident(name: &str, suffix: &str) -> String`;
- `rust_type(field: &FieldSpec) -> &'static str`;
- `visibility(vis: RustVisibility) -> &'static str`.

- [ ] **Step 4: Run emit-rust test to verify GREEN**

Run:

```text
cargo test --features cli --test cli_smoke schema_emit_rust_generates_struct_and_offsets
```

Expected: pass.

### Task 5: Schema Compare

**Files:**
- Create: `src/cli/schema_compare.rs`
- Modify: `src/cli/mod.rs`
- Test: `tests/cli_smoke.rs`

- [ ] **Step 1: Write failing equality test**

Add:

```rust
#[test]
fn schema_compare_ignores_description_and_output_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.json");
    let new = dir.path().join("new.json");
    fs::write(&old, r#"{"version":2,"record_length":3,"input_encoding":"binary","output":"jsonl","fields":[{"name":"amount","field_type":"packed-decimal","offset":0,"length":3,"total_digits":4,"scale":2,"signed":true,"description":"old"}]}"#).unwrap();
    fs::write(&new, r#"{"version":2,"record_length":3,"input_encoding":"binary","output":"csv","fields":[{"name":"amount","field_type":"packed-decimal","offset":0,"length":3,"total_digits":4,"scale":2,"signed":true,"description":"new"}]}"#).unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "compare",
        "--old",
        old.to_str().unwrap(),
        "--new",
        new.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"changed\": false"));
}
```

- [ ] **Step 2: Write failing diff test**

Add:

```rust
#[test]
fn schema_compare_reports_breaking_field_layout_changes() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.json");
    let new = dir.path().join("new.json");
    fs::write(&old, r#"{"version":2,"record_length":3,"input_encoding":"binary","fields":[{"name":"amount","field_type":"packed-decimal","offset":0,"length":3,"total_digits":4,"scale":2,"signed":true}]}"#).unwrap();
    fs::write(&new, r#"{"version":2,"record_length":4,"input_encoding":"binary","fields":[{"name":"amount","field_type":"packed-decimal","offset":1,"length":3,"total_digits":4,"scale":2,"signed":true}]}"#).unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "compare",
        "--old",
        old.to_str().unwrap(),
        "--new",
        new.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .code(1)
        .stdout(contains("\"changed\": true"))
        .stdout(contains("\"severity\": \"breaking\""))
        .stdout(contains("offset"));
}
```

- [ ] **Step 3: Run tests to verify RED**

Run the full CLI smoke suite if multiple filters are inconvenient:

```text
cargo test --features cli --test cli_smoke
```

Expected: compare tests fail while previous tests pass.

- [ ] **Step 4: Implement `schema_compare.rs`**

Implement:

```rust
#[derive(Debug, Serialize)]
struct SchemaCompareReport {
    changed: bool,
    breaking_count: usize,
    warning_count: usize,
    info_count: usize,
    diffs: Vec<SchemaDiff>,
}

#[derive(Debug, Serialize)]
struct SchemaDiff {
    severity: &'static str,
    path: String,
    old: Option<String>,
    new: Option<String>,
    message: String,
}
```

Load both schemas with `load_schema`. Compare:

- version;
- record_length;
- input_encoding;
- on_error;
- verification_scope;
- field map by name;
- filler map by name.

Format values with `serde_json::to_string`.

- [ ] **Step 5: Implement output and exit behavior**

For JSON, pretty-print `SchemaCompareReport`.

For JSONL, print one diff per line.

For table:

```text
severity	path	old	new	message
breaking	fields.amount.offset	0	1	field offset changed
```

Return `Err(CliError::data("E_SCHEMA_DIFF", "..."))` only when the diff meets `args.fail_on`; otherwise return `Ok(())`.

- [ ] **Step 6: Run compare tests to verify GREEN**

Run:

```text
cargo test --features cli --test cli_smoke
```

Expected: all CLI smoke tests pass.

### Task 6: Docs And Developer Map

**Files:**
- Modify: `README.md`
- Modify: `docs/cli.md`
- Modify: `docs/developer-map.md`
- Test: doc tests through cargo

- [ ] **Step 1: Document schema workflow commands**

In `README.md`, add a short "Schema Workflow" section with the three commands and copybook limitations.

In `docs/cli.md`, add detailed usage blocks for:

```text
hostlens schema from-copybook --copybook customer.cpy --encoding cp037
hostlens schema emit-rust --schema customer.schema.json --struct-name CustomerRecord
hostlens schema compare --old old.json --new new.json --output json
```

- [ ] **Step 2: Update developer map**

Add:

```markdown
- `src/cli/copybook.rs`: limited COBOL copybook declaration parser and schema v2 bootstrapper.
- `src/cli/schema_emit.rs`: Rust source generator for schema constants and decoded-value structs.
- `src/cli/schema_compare.rs`: semantic schema diff engine and compare output rendering.
```

- [ ] **Step 3: Run docs verification**

Run:

```text
cargo test --doc --all-features
```

Expected: exit 0.

### Task 7: Final Verification And Packaging

**Files:**
- All touched files
- Generated package artifacts only if user asks to package again

- [ ] **Step 1: Run formatting**

Run:

```text
cargo fmt --all --check
```

Expected: exit 0.

- [ ] **Step 2: Run CLI smoke suite**

Run:

```text
cargo test --features cli --test cli_smoke
```

Expected: all tests pass.

- [ ] **Step 3: Run full tests**

Run:

```text
cargo test --all-features
```

Expected: all tests pass.

- [ ] **Step 4: Run doc tests**

Run:

```text
cargo test --doc --all-features
```

Expected: exit 0.

- [ ] **Step 5: Run clippy**

Run:

```text
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: exit 0.

- [ ] **Step 6: Run package dry checks**

Run:

```text
cargo package --allow-dirty
```

Expected: package verifies. Run `cargo publish --dry-run --allow-dirty` only when network access is allowed.

- [ ] **Step 7: Report remaining boundaries**

Final report must state:

- copybook support is limited bootstrap support;
- `REDEFINES`, `OCCURS`, `SYNC`, level 88, and COPY expansion remain unsupported;
- no SQL/Parquet/Python/Wasm implementation is included in this release;
- no crates.io publish, tag, push, or GitHub release was performed unless explicitly requested later.
