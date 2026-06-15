mod args;
mod constants;
mod error;
mod record;

use args::*;
use clap::{CommandFactory, Parser, ValueEnum};
use cobol_packed::{
    from_packed, from_packed_lossless, nibble_iter, to_packed, to_packed_lossless,
    to_packed_with_sign, PackedConfig, PackedError, SignMode,
};
use constants::*;
use error::CliError;
pub use error::ExitCode;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum OutputFormat {
    Table,
    Json,
    Jsonl,
    Csv,
    Audit,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CliSignMode {
    Pfd,
    Nopfd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FieldMode {
    Canonical,
    Lossless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InputEncoding {
    Binary,
    Hex,
    Csv,
    Jsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum OnError {
    Fail,
    SkipRecord,
    EmitErrorRow,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum VerificationScope {
    Field,
    Record,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum EvidenceMode {
    Minimal,
    Full,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum EvidenceArgv {
    Redacted,
    Raw,
    Omit,
}

#[derive(Debug, Serialize, Deserialize)]
struct Schema {
    version: u8,
    record_length: Option<usize>,
    input_encoding: InputEncoding,
    #[serde(default = "default_on_error")]
    on_error: OnError,
    #[serde(default)]
    output: Option<OutputFormat>,
    #[serde(default = "default_verification_scope")]
    verification_scope: VerificationScope,
    #[serde(default)]
    fillers: Vec<FillerSpec>,
    #[serde(default)]
    fields: Vec<FieldSpec>,
    #[serde(default)]
    layout_mode: Option<record::LayoutMode>,
    #[serde(default)]
    platform_profile: record::PlatformProfile,
    #[serde(default)]
    layout: Vec<record::RawLayoutItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FieldSpec {
    name: String,
    offset: Option<usize>,
    length: Option<usize>,
    total_digits: u8,
    scale: u8,
    signed: bool,
    sign_mode: CliSignMode,
    mode: FieldMode,
    #[serde(default = "default_required")]
    required: bool,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FillerSpec {
    name: String,
    offset: usize,
    length: usize,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Serialize)]
struct SemanticSchema<'a> {
    version: u8,
    record_length: Option<usize>,
    input_encoding: InputEncoding,
    on_error: OnError,
    output: Option<OutputFormat>,
    verification_scope: VerificationScope,
    fillers: Vec<SemanticFiller<'a>>,
    fields: Vec<SemanticField<'a>>,
}

#[derive(Serialize)]
struct SemanticField<'a> {
    name: &'a str,
    offset: Option<usize>,
    length: Option<usize>,
    total_digits: u8,
    scale: u8,
    signed: bool,
    sign_mode: CliSignMode,
    mode: FieldMode,
    required: bool,
}

#[derive(Serialize)]
struct SemanticFiller<'a> {
    name: &'a str,
    offset: usize,
    length: usize,
}

struct FieldPlan<'a> {
    spec: &'a FieldSpec,
    cfg: PackedConfig,
    expected_len: usize,
}

#[derive(Debug, Clone, Copy)]
struct ProcessingLimits {
    max_records: Option<usize>,
}

struct SchemaHashes {
    file_sha256: String,
    semantic_sha256: String,
}

struct AuditContext<'a> {
    command: &'a str,
    hashes: &'a SchemaHashes,
    input: &'a Path,
    limits: ProcessingLimits,
    failure_sample_limit: usize,
    include_input_hash: bool,
    verification_scope: VerificationScope,
    evidence_mode: EvidenceMode,
    evidence_argv: EvidenceArgv,
}

#[derive(Debug, Serialize)]
struct FieldPlanSummary<'a> {
    name: &'a str,
    offset: Option<usize>,
    end_offset: Option<usize>,
    length: usize,
    total_digits: u8,
    scale: u8,
    signed: bool,
    sign_mode: CliSignMode,
    mode: FieldMode,
    required: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum LayoutKind {
    Field,
    Filler,
    SyncSlack,
    Occurs,
    RedefinesBase,
}

#[derive(Debug, Clone, Serialize)]
struct LayoutRangeSummary {
    name: String,
    kind: LayoutKind,
    offset: usize,
    end_offset: usize,
    length: usize,
}

#[derive(Debug, Clone, Serialize)]
struct LayoutGapSummary {
    offset: usize,
    end_offset: usize,
    length: usize,
}

#[derive(Debug, Clone, Serialize)]
struct LayoutOverlapSummary {
    offset: usize,
    end_offset: usize,
    length: usize,
    previous: String,
    current: String,
}

#[derive(Debug, Clone, Serialize)]
struct SchemaCoverageSummary {
    record_length: Option<usize>,
    covered_bytes: usize,
    gap_bytes: Option<usize>,
    full_coverage: Option<bool>,
    overlap_count: usize,
    first_offset: Option<usize>,
    last_end_offset: Option<usize>,
    ranges: Vec<LayoutRangeSummary>,
    gaps: Vec<LayoutGapSummary>,
    overlaps: Vec<LayoutOverlapSummary>,
}

#[derive(Debug, Serialize)]
struct DecodedField {
    version: u8,
    record_index: Option<usize>,
    field: String,
    offset: Option<usize>,
    raw_hex: String,
    raw_byte_len: usize,
    raw_hex_truncated: bool,
    value: Option<String>,
    sign_nibble: Option<String>,
    sign_class: Option<String>,
    valid: bool,
    error_code: Option<&'static str>,
    message: Option<String>,
    recoverable: bool,
}

#[derive(Debug, Serialize)]
struct AuditReport {
    version: u8,
    tool: String,
    tool_version: String,
    command: String,
    evidence_mode: EvidenceMode,
    schema_hash: String,
    schema_file_sha256: String,
    schema_field_count: usize,
    schema_filler_count: usize,
    schema_record_length: Option<usize>,
    schema_input_encoding: InputEncoding,
    verification_scope: VerificationScope,
    record_coverage: SchemaCoverageSummary,
    input_path: String,
    input_size_bytes: u64,
    input_sha256: Option<String>,
    runtime: Option<RuntimeEvidence>,
    record_limit: Option<usize>,
    failure_sample_limit: usize,
    records_seen: usize,
    records_valid: usize,
    records_invalid: usize,
    fields_seen: usize,
    fields_valid: usize,
    fields_invalid: usize,
    status: AuditStatus,
    field_byte_for_byte_verified: Option<bool>,
    record_byte_for_byte_verified: Option<bool>,
    byte_for_byte_verified: Option<bool>,
    negative_zero_count: usize,
    non_preferred_sign_count: usize,
    sign_distribution: BTreeMap<String, usize>,
    error_distribution: BTreeMap<String, usize>,
    field_profiles: BTreeMap<String, FieldAuditSummary>,
    failure_samples: Vec<DecodedField>,
}

#[derive(Debug, Serialize)]
struct RuntimeEvidence {
    #[serde(skip_serializing_if = "Option::is_none")]
    argv: Option<Vec<String>>,
    argv_redacted: bool,
    cwd: String,
    os: &'static str,
    arch: &'static str,
    family: &'static str,
    exe_suffix: &'static str,
    generated_unix_seconds: u64,
}

#[derive(Debug, Serialize, Default)]
struct FieldAuditSummary {
    fields_seen: usize,
    fields_valid: usize,
    fields_invalid: usize,
    min_value: Option<String>,
    max_value: Option<String>,
    negative_zero_count: usize,
    non_preferred_sign_count: usize,
    sign_distribution: BTreeMap<String, usize>,
    error_distribution: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct AuditSummaryCsv<'a> {
    version: u8,
    tool: &'a str,
    tool_version: &'a str,
    command: &'a str,
    status: AuditStatus,
    evidence_mode: EvidenceMode,
    schema_hash: &'a str,
    schema_file_sha256: &'a str,
    schema_field_count: usize,
    schema_filler_count: usize,
    schema_record_length: Option<usize>,
    schema_input_encoding: InputEncoding,
    verification_scope: VerificationScope,
    record_full_coverage: Option<bool>,
    record_overlap_count: usize,
    record_gap_count: usize,
    input_path: &'a str,
    input_size_bytes: u64,
    input_sha256: Option<&'a str>,
    record_limit: Option<usize>,
    records_seen: usize,
    records_valid: usize,
    records_invalid: usize,
    fields_seen: usize,
    fields_valid: usize,
    fields_invalid: usize,
    field_byte_for_byte_verified: Option<bool>,
    record_byte_for_byte_verified: Option<bool>,
    byte_for_byte_verified: Option<bool>,
    negative_zero_count: usize,
    non_preferred_sign_count: usize,
    distinct_error_codes: usize,
    distinct_fields_seen: usize,
    failure_sample_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AuditStatus {
    Passed,
    Failed,
    Empty,
}

enum RowSink {
    None,
    Jsonl,
    Csv(Box<csv::Writer<io::Stdout>>),
    Table { wrote_header: bool },
    Buffer(Vec<DecodedField>),
}

impl RowSink {
    fn for_output(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Json => RowSink::Buffer(Vec::new()),
            OutputFormat::Jsonl => RowSink::Jsonl,
            OutputFormat::Csv => RowSink::Csv(Box::new(csv::Writer::from_writer(io::stdout()))),
            OutputFormat::Table => RowSink::Table {
                wrote_header: false,
            },
            OutputFormat::Audit => RowSink::None,
        }
    }

    fn emit(&mut self, row: DecodedField) -> Result<(), CliError> {
        match self {
            RowSink::None => Ok(()),
            RowSink::Jsonl => {
                println!("{}", serde_json::to_string(&row)?);
                Ok(())
            }
            RowSink::Csv(writer) => writer
                .serialize(row)
                .map_err(|err| CliError::data("E_CSV", err.to_string())),
            RowSink::Table { wrote_header } => {
                if !*wrote_header {
                    println!("record\tfield\toffset\tvalid\tvalue\tsign\traw_hex\terror");
                    *wrote_header = true;
                }
                print_table_row(&row);
                Ok(())
            }
            RowSink::Buffer(rows) => {
                if rows.len() >= MAX_BUFFERED_ROWS {
                    return Err(CliError::data(
                        "E_OUTPUT_LIMIT",
                        format!("json output buffers at most {MAX_BUFFERED_ROWS} rows; use jsonl or csv for streaming output"),
                    ));
                }
                rows.push(row);
                Ok(())
            }
        }
    }

    fn finish(mut self, format: OutputFormat) -> Result<(), CliError> {
        match &mut self {
            RowSink::Csv(writer) => {
                writer.flush()?;
                Ok(())
            }
            RowSink::Buffer(rows) => render_json(rows),
            RowSink::Table { wrote_header } if !*wrote_header && format == OutputFormat::Table => {
                println!("record\tfield\toffset\tvalid\tvalue\tsign\traw_hex\terror");
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

fn default_on_error() -> OnError {
    OnError::Fail
}
fn default_verification_scope() -> VerificationScope {
    VerificationScope::Field
}
fn default_required() -> bool {
    true
}

impl CliSignMode {
    fn to_core(self) -> SignMode {
        match self {
            CliSignMode::Pfd => SignMode::Pfd,
            CliSignMode::Nopfd => SignMode::Nopfd,
        }
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OutputFormat::Table => "table",
            OutputFormat::Json => "json",
            OutputFormat::Jsonl => "jsonl",
            OutputFormat::Csv => "csv",
            OutputFormat::Audit => "audit",
        })
    }
}

impl fmt::Display for CliSignMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CliSignMode::Pfd => "pfd",
            CliSignMode::Nopfd => "nopfd",
        })
    }
}

impl fmt::Display for InputEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InputEncoding::Binary => "binary",
            InputEncoding::Hex => "hex",
            InputEncoding::Csv => "csv",
            InputEncoding::Jsonl => "jsonl",
        })
    }
}

impl fmt::Display for OnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OnError::Fail => "fail",
            OnError::SkipRecord => "skip-record",
            OnError::EmitErrorRow => "emit-error-row",
        })
    }
}

impl fmt::Display for VerificationScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            VerificationScope::Field => "field",
            VerificationScope::Record => "record",
        })
    }
}

impl fmt::Display for EvidenceMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EvidenceMode::Minimal => "minimal",
            EvidenceMode::Full => "full",
        })
    }
}

impl fmt::Display for AuditStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AuditStatus::Passed => "passed",
            AuditStatus::Failed => "failed",
            AuditStatus::Empty => "empty",
        })
    }
}

pub fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Decode(args) => decode_field(args, false),
        Command::Inspect(args) => decode_field(args, true),
        Command::Encode(args) => encode_field(args),
        Command::Batch { command } => match command {
            BatchCommand::Decode(args) => batch_decode(args),
            BatchCommand::Verify(args) => batch_verify(args),
        },
        Command::Schema { command } => match command {
            SchemaCommand::Check(args) => schema_check(args),
            SchemaCommand::EmitRust(args) => schema_emit_rust(args),
            SchemaCommand::FromCopybook(args) => schema_from_copybook(args),
            SchemaCommand::Compare(args) => schema_compare(args),
        },
        Command::Profile(args) => profile(args),
        Command::Completions(args) => generate_completions(args),
        Command::Man => generate_man_page(),
    }
}

fn generate_completions(args: CompletionsArgs) -> Result<(), CliError> {
    let mut command = Cli::command();
    let name = command.get_name().to_string();
    clap_complete::generate(args.shell, &mut command, name, &mut io::stdout());
    Ok(())
}

fn generate_man_page() -> Result<(), CliError> {
    let command = Cli::command();
    let mut buffer = Vec::new();
    clap_mangen::Man::new(command).render(&mut buffer)?;
    io::stdout().write_all(&buffer)?;
    Ok(())
}

fn decode_field(args: FieldDecodeArgs, inspect: bool) -> Result<(), CliError> {
    let cfg = cfg_from_shape(&args.shape)?;
    let bytes = read_field_input(
        args.hex.as_deref(),
        args.file.as_ref(),
        args.stdin,
        args.offset,
        cfg.byte_len(),
    )?;
    let sign_mode = args.shape.sign_mode.to_core();
    let field = decode_named_field(
        None,
        "value",
        None,
        &bytes,
        &cfg,
        sign_mode,
        FieldMode::Lossless,
    );

    if inspect {
        render_inspect(&bytes, &field, args.output)
    } else if !field.valid {
        Err(CliError::data(
            field.error_code.unwrap_or("E_DATA"),
            field.message.unwrap_or_else(|| "decode failed".to_string()),
        ))
    } else {
        render_fields(&[field], args.output)
    }
}

fn encode_field(args: EncodeArgs) -> Result<(), CliError> {
    let cfg = cfg_from_shape(&args.shape)?;
    let value = Decimal::from_str(&args.value)
        .map_err(|err| CliError::data("E_DECIMAL", format!("invalid decimal value: {err}")))?;
    let bytes = match args.sign_nibble {
        Some(raw) => {
            let nib = parse_sign_nibble(&raw)?;
            to_packed_with_sign(&value, &cfg, nib).map_err(map_packed_error)?
        }
        None => to_packed(&value, &cfg).map_err(map_packed_error)?,
    };
    let row = serde_json::json!({
        "version": OUTPUT_VERSION,
        "value": value.to_string(),
        "raw_hex": to_hex(&bytes),
        "byte_len": bytes.len(),
    });
    render_value(&row, args.output)
}

fn schema_check(args: SchemaArgs) -> Result<(), CliError> {
    let (schema, hashes) = load_schema(&args.schema)?;
    if schema.version == 2 {
        let summary =
            record::schema_check_value_v2(&schema, &hashes.semantic_sha256, &hashes.file_sha256)?;
        return render_value(&summary, args.output);
    }
    let fields = schema_field_summaries(&schema)?;
    let coverage = schema_coverage_summary(&schema)?;
    let summary = serde_json::json!({
        "version": OUTPUT_VERSION,
        "schema_version": schema.version,
        "schema_hash": hashes.semantic_sha256,
        "schema_file_sha256": hashes.file_sha256,
        "field_count": schema.fields.len(),
        "filler_count": schema.fillers.len(),
        "record_length": schema.record_length,
        "input_encoding": schema.input_encoding,
        "on_error": schema.on_error,
        "verification_scope": schema.verification_scope,
        "coverage": coverage,
        "fields": fields,
        "fillers": schema.fillers,
        "valid": true,
    });
    render_value(&summary, args.output)
}

fn schema_emit_rust(args: EmitRustArgs) -> Result<(), CliError> {
    let (schema, _) = load_schema(&args.schema)?;
    record::emit_rust(&schema, &args.output)?;
    let summary = serde_json::json!({
        "version": OUTPUT_VERSION,
        "schema_version": schema.version,
        "output": args.output.display().to_string(),
        "valid": true,
    });
    render_value(&summary, OutputFormat::Table)
}

fn schema_from_copybook(args: CopybookArgs) -> Result<(), CliError> {
    let text = fs::read_to_string(&args.input)?;
    let schema = copybook_to_schema_json(
        &text,
        args.record_length,
        &args.input_encoding,
        &args.codepage,
        &args.endian,
    )?;
    let bytes = serde_json::to_vec_pretty(&schema)
        .map_err(|err| CliError::internal(format!("failed to serialize schema: {err}")))?;
    fs::write(&args.output, bytes)?;
    let summary = serde_json::json!({
        "version": OUTPUT_VERSION,
        "schema_version": 2,
        "input": args.input.display().to_string(),
        "output": args.output.display().to_string(),
        "field_count": schema.get("layout").and_then(serde_json::Value::as_array).map_or(0, Vec::len),
        "valid": true,
    });
    render_value(&summary, OutputFormat::Table)
}

fn schema_compare(args: SchemaCompareArgs) -> Result<(), CliError> {
    let (left, left_hashes) = load_schema(&args.left)?;
    let (right, right_hashes) = load_schema(&args.right)?;
    let left_fields = compare_field_map(&left)?;
    let right_fields = compare_field_map(&right)?;
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for (path, right_field) in &right_fields {
        match left_fields.get(path) {
            Some(left_field) if left_field != right_field => changed.push(serde_json::json!({
                "path": path,
                "left": left_field,
                "right": right_field,
            })),
            None => added.push(right_field.clone()),
            _ => {}
        }
    }
    for (path, left_field) in &left_fields {
        if !right_fields.contains_key(path) {
            removed.push(left_field.clone());
        }
    }
    let summary = serde_json::json!({
        "version": OUTPUT_VERSION,
        "left_schema_hash": left_hashes.semantic_sha256,
        "right_schema_hash": right_hashes.semantic_sha256,
        "same_semantics": left_hashes.semantic_sha256 == right_hashes.semantic_sha256,
        "added": added,
        "removed": removed,
        "changed": changed,
    });
    render_value(&summary, args.output)
}

fn compare_field_map(schema: &Schema) -> Result<BTreeMap<String, serde_json::Value>, CliError> {
    if schema.version == 2 {
        return record::compare_fields_v2(schema);
    }
    let mut fields = BTreeMap::new();
    for field in &schema.fields {
        let plan = plan_field(field)?;
        fields.insert(
            field.name.clone(),
            serde_json::json!({
                "path": field.name,
                "name": field.name,
                "offset": field.offset,
                "length": field.length.unwrap_or(plan.expected_len),
                "required": field.required,
                "codec": {
                    "field_type": "packed-decimal",
                    "total_digits": field.total_digits,
                    "scale": field.scale,
                    "signed": field.signed,
                    "sign_mode": field.sign_mode,
                    "mode": field.mode,
                },
            }),
        );
    }
    Ok(fields)
}

fn copybook_to_schema_json(
    text: &str,
    record_length: Option<usize>,
    input_encoding: &str,
    codepage: &str,
    endian: &str,
) -> Result<serde_json::Value, CliError> {
    let input_encoding = match input_encoding {
        "binary" | "hex" => input_encoding,
        _ => {
            return Err(CliError::config(
                "E_SCHEMA",
                "copybook import supports input_encoding binary or hex",
            ))
        }
    };
    let endian = match endian {
        "big" | "little" => endian,
        _ => return Err(CliError::config("E_SCHEMA", "endian must be big or little")),
    };
    let mut layout = Vec::new();
    for (line_index, raw_line) in text.lines().enumerate() {
        let Some(line) = normalize_copybook_line(raw_line) else {
            continue;
        };
        let line_no = line_index + 1;
        let upper = line.to_ascii_uppercase();
        for unsupported in [
            " REDEFINES ",
            " OCCURS ",
            " DEPENDING ",
            " SYNCHRONIZED",
            " SYNC",
            " JUSTIFIED",
            " SIGN ",
        ] {
            if upper.contains(unsupported) {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "copybook line {line_no} uses unsupported clause {}; model it directly in schema v2",
                        unsupported.trim()
                    ),
                ));
            }
        }
        let tokens: Vec<&str> = upper.split_whitespace().collect();
        if tokens.len() < 2 || !tokens[0].bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let name = tokens[1].trim_end_matches('.');
        if !upper.contains(" PIC ") && !upper.contains(" PICTURE ") {
            continue;
        }
        validate_field_name(name)?;
        let pic = extract_pic(&tokens, line_no)?;
        let pic_shape = parse_pic_shape(pic, line_no)?;
        let is_filler = name == "FILLER";
        let item = copybook_item_json(name, &pic_shape, is_filler, &upper, codepage, endian)?;
        layout.push(item);
    }
    if layout.is_empty() {
        return Err(CliError::config(
            "E_SCHEMA",
            "copybook import found no supported PIC fields",
        ));
    }
    let schema = serde_json::json!({
        "version": 2,
        "layout_mode": "sequential",
        "record_length": record_length,
        "input_encoding": input_encoding,
        "platform_profile": "ibm-z-os",
        "on_error": "fail",
        "output": "jsonl",
        "layout": layout,
    });
    let parsed: Schema = serde_json::from_value(schema.clone()).map_err(|err| {
        CliError::config(
            "E_SCHEMA",
            format!("generated schema from copybook is invalid: {err}"),
        )
    })?;
    validate_schema(&parsed)?;
    Ok(schema)
}

fn normalize_copybook_line(line: &str) -> Option<String> {
    if line.trim().is_empty() {
        return None;
    }
    if line.len() > 6 {
        let bytes = line.as_bytes();
        if matches!(bytes.get(6), Some(b'*' | b'/')) {
            return None;
        }
        if bytes
            .get(..6)
            .is_some_and(|prefix| prefix.iter().all(u8::is_ascii_digit))
        {
            return Some(line[6..].trim().trim_end_matches('.').to_string());
        }
    }
    let trimmed = line.trim();
    if trimmed.starts_with('*') || trimmed.starts_with('/') {
        None
    } else {
        Some(trimmed.trim_end_matches('.').to_string())
    }
}

fn extract_pic<'a>(tokens: &'a [&str], line_no: usize) -> Result<&'a str, CliError> {
    for (idx, token) in tokens.iter().enumerate() {
        if matches!(*token, "PIC" | "PICTURE") {
            return tokens.get(idx + 1).copied().ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("copybook line {line_no} missing PIC body"),
                )
            });
        }
    }
    Err(CliError::config(
        "E_SCHEMA",
        format!("copybook line {line_no} missing PIC clause"),
    ))
}

struct PicShape {
    class: PicClass,
    total_digits: u8,
    scale: u8,
    signed: bool,
    length: usize,
}

enum PicClass {
    Numeric,
    Alphanumeric,
}

fn parse_pic_shape(pic: &str, line_no: usize) -> Result<PicShape, CliError> {
    let mut body = pic.trim().trim_end_matches('.').to_ascii_uppercase();
    let signed = body.starts_with('S');
    if signed {
        body.remove(0);
    }
    if body.contains('X') || body.contains('A') {
        let length = parse_pic_repeated_class(&body, line_no, 'X')
            .or_else(|_| parse_pic_repeated_class(&body, line_no, 'A'))?;
        return Ok(PicShape {
            class: PicClass::Alphanumeric,
            total_digits: 0,
            scale: 0,
            signed: false,
            length,
        });
    }
    let chars: Vec<char> = body.chars().collect();
    let mut idx = 0usize;
    let mut total = 0usize;
    let mut scale = 0usize;
    let mut in_scale = false;
    while idx < chars.len() {
        match chars[idx] {
            '9' => {
                let (count, next) = parse_pic_repeat(&chars, idx + 1, line_no)?;
                total = total.saturating_add(count);
                if in_scale {
                    scale = scale.saturating_add(count);
                }
                idx = next;
            }
            'V' => {
                if in_scale {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!("copybook line {line_no} has multiple V scale markers"),
                    ));
                }
                in_scale = true;
                idx += 1;
            }
            _ => {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("copybook line {line_no} has unsupported PIC body {pic}"),
                ))
            }
        }
    }
    let total_digits = u8::try_from(total).map_err(|_| {
        CliError::config(
            "E_SCHEMA",
            format!("copybook line {line_no} total digits exceed 18"),
        )
    })?;
    let scale = u8::try_from(scale).map_err(|_| {
        CliError::config(
            "E_SCHEMA",
            format!("copybook line {line_no} scale exceeds supported range"),
        )
    })?;
    if total_digits == 0 || total_digits > 18 || scale > total_digits {
        return Err(CliError::config(
            "E_SCHEMA",
            format!(
                "copybook line {line_no} numeric PIC must have 1..=18 digits and scale <= digits"
            ),
        ));
    }
    Ok(PicShape {
        class: PicClass::Numeric,
        total_digits,
        scale,
        signed,
        length: usize::from(total_digits),
    })
}

fn parse_pic_repeated_class(body: &str, line_no: usize, class: char) -> Result<usize, CliError> {
    let chars: Vec<char> = body.chars().collect();
    let mut idx = 0usize;
    let mut length = 0usize;
    while idx < chars.len() {
        if chars[idx] != class {
            return Err(CliError::config(
                "E_SCHEMA",
                format!("copybook line {line_no} has mixed or unsupported alphanumeric PIC"),
            ));
        }
        let (count, next) = parse_pic_repeat(&chars, idx + 1, line_no)?;
        length = length.saturating_add(count);
        idx = next;
    }
    if length == 0 {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("copybook line {line_no} has zero-length PIC"),
        ));
    }
    Ok(length)
}

fn parse_pic_repeat(
    chars: &[char],
    idx: usize,
    line_no: usize,
) -> Result<(usize, usize), CliError> {
    if chars.get(idx) != Some(&'(') {
        return Ok((1, idx));
    }
    let mut end = idx + 1;
    let mut digits = String::new();
    while let Some(ch) = chars.get(end) {
        if *ch == ')' {
            let count = digits.parse::<usize>().map_err(|_| {
                CliError::config(
                    "E_SCHEMA",
                    format!("copybook line {line_no} has invalid PIC repeat"),
                )
            })?;
            return Ok((count, end + 1));
        }
        if !ch.is_ascii_digit() {
            return Err(CliError::config(
                "E_SCHEMA",
                format!("copybook line {line_no} has invalid PIC repeat"),
            ));
        }
        digits.push(*ch);
        end += 1;
    }
    Err(CliError::config(
        "E_SCHEMA",
        format!("copybook line {line_no} has unterminated PIC repeat"),
    ))
}

fn copybook_item_json(
    name: &str,
    shape: &PicShape,
    is_filler: bool,
    line: &str,
    codepage: &str,
    endian: &str,
) -> Result<serde_json::Value, CliError> {
    if is_filler {
        let length = match shape.class {
            PicClass::Alphanumeric => shape.length,
            PicClass::Numeric if line.contains("COMP-3") || line.contains("PACKED-DECIMAL") => {
                (usize::from(shape.total_digits) + 2) / 2
            }
            PicClass::Numeric if line.contains("COMP") || line.contains("BINARY") => {
                match shape.total_digits {
                    1..=4 => 2,
                    5..=9 => 4,
                    10..=18 => 8,
                    _ => {
                        return Err(CliError::config(
                            "E_SCHEMA",
                            "binary total_digits must be in 1..=18",
                        ))
                    }
                }
            }
            PicClass::Numeric => shape.length,
        };
        return Ok(serde_json::json!({
            "kind": "filler",
            "name": name,
            "length": length,
        }));
    }
    match shape.class {
        PicClass::Alphanumeric => Ok(serde_json::json!({
            "kind": "field",
            "name": name,
            "length": shape.length,
            "field_type": "alphanumeric",
            "encoding": "ebcdic",
            "codepage": codepage,
        })),
        PicClass::Numeric if line.contains("COMP-3") || line.contains("PACKED-DECIMAL") => {
            Ok(serde_json::json!({
                "kind": "field",
                "name": name,
                "field_type": "packed-decimal",
                "total_digits": shape.total_digits,
                "scale": shape.scale,
                "signed": shape.signed,
                "sign_mode": "pfd",
                "mode": "lossless",
            }))
        }
        PicClass::Numeric if line.contains("COMP") || line.contains("BINARY") => {
            Ok(serde_json::json!({
                "kind": "field",
                "name": name,
                "field_type": "binary",
                "total_digits": shape.total_digits,
                "scale": shape.scale,
                "signed": shape.signed,
                "endian": endian,
            }))
        }
        PicClass::Numeric => Ok(serde_json::json!({
            "kind": "field",
            "name": name,
            "field_type": "zoned-decimal",
            "total_digits": shape.total_digits,
            "scale": shape.scale,
            "signed": shape.signed,
            "encoding": "ebcdic",
            "codepage": codepage,
            "sign_policy": "preferred",
        })),
    }
}

fn batch_decode(args: BatchInputArgs) -> Result<(), CliError> {
    let (schema, hashes) = load_schema(&args.schema)?;
    let output = args.output.or(schema.output).unwrap_or(OutputFormat::Jsonl);
    if args.coverage_report {
        return Err(CliError::config(
            "E_CONFIG",
            "--coverage-report is only valid with batch verify",
        ));
    }
    let limits = limits_from_args(&args)?;
    let verification_scope = verification_scope_from_args(&schema, &args)?;
    let mut audit = new_audit(
        &schema,
        AuditContext {
            command: "batch decode",
            hashes: &hashes,
            input: &args.input,
            limits,
            failure_sample_limit: args.sample_failures,
            include_input_hash: output == OutputFormat::Audit,
            verification_scope,
            evidence_mode: args.evidence_mode,
            evidence_argv: args.evidence_argv,
        },
    )?;
    let mut sink = RowSink::for_output(output);
    process_records(&schema, &args.input, false, limits, &mut audit, &mut sink)?;
    finalize_audit(&mut audit);
    match output {
        OutputFormat::Audit => render_audit(&audit),
        _ => sink.finish(output),
    }
}

fn batch_verify(args: BatchInputArgs) -> Result<(), CliError> {
    let (schema, hashes) = load_schema(&args.schema)?;
    let output = args.output.or(schema.output).unwrap_or(OutputFormat::Audit);
    if args.coverage_report && output != OutputFormat::Audit {
        return Err(CliError::config(
            "E_CONFIG",
            "--coverage-report requires audit output because coverage is emitted in the audit report",
        ));
    }
    let limits = limits_from_args(&args)?;
    let verification_scope = verification_scope_from_args(&schema, &args)?;
    let mut audit = new_audit(
        &schema,
        AuditContext {
            command: "batch verify",
            hashes: &hashes,
            input: &args.input,
            limits,
            failure_sample_limit: args.sample_failures,
            include_input_hash: true,
            verification_scope,
            evidence_mode: args.evidence_mode,
            evidence_argv: args.evidence_argv,
        },
    )?;
    let mut sink = RowSink::for_output(output);
    process_records(&schema, &args.input, true, limits, &mut audit, &mut sink)?;
    finalize_audit(&mut audit);
    finalize_verify_audit(&mut audit);
    match output {
        OutputFormat::Audit => render_audit(&audit),
        _ => sink.finish(output),
    }?;
    if matches!(audit.status, AuditStatus::Failed) {
        return Err(CliError::data(
            "E_VERIFY",
            "batch verify failed; see output or audit report for failing records",
        ));
    }
    Ok(())
}

fn profile(args: BatchInputArgs) -> Result<(), CliError> {
    let (schema, hashes) = load_schema(&args.schema)?;
    let output = args.output.or(schema.output).unwrap_or(OutputFormat::Audit);
    if args.coverage_report {
        return Err(CliError::config(
            "E_CONFIG",
            "--coverage-report is only valid with batch verify",
        ));
    }
    let limits = limits_from_args(&args)?;
    let verification_scope = verification_scope_from_args(&schema, &args)?;
    let mut audit = new_audit(
        &schema,
        AuditContext {
            command: "profile",
            hashes: &hashes,
            input: &args.input,
            limits,
            failure_sample_limit: args.sample_failures,
            include_input_hash: true,
            verification_scope,
            evidence_mode: args.evidence_mode,
            evidence_argv: args.evidence_argv,
        },
    )?;
    let mut sink = RowSink::None;
    process_records(&schema, &args.input, false, limits, &mut audit, &mut sink)?;
    finalize_audit(&mut audit);
    render_audit_with_format(&audit, output)
}

fn process_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    if schema.version == 2 {
        return record::process_records_v2(schema, input, verify, limits, audit, sink);
    }
    match schema.input_encoding {
        InputEncoding::Binary => {
            process_binary_records(schema, input, verify, limits, audit, sink)?
        }
        InputEncoding::Hex => process_hex_records(schema, input, verify, limits, audit, sink)?,
        InputEncoding::Csv => process_csv_records(schema, input, verify, limits, audit, sink)?,
        InputEncoding::Jsonl => process_jsonl_records(schema, input, verify, limits, audit, sink)?,
    }
    Ok(())
}

fn process_binary_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let record_len = schema
        .record_length
        .ok_or_else(|| CliError::config("E_SCHEMA", "binary schemas require record_length"))?;
    let mut reader = BufReader::new(fs::File::open(input)?);
    let mut record = vec![0u8; record_len];
    let mut idx = 0usize;
    loop {
        if record_limit_reached(idx, limits) {
            break;
        }
        let mut read = 0usize;
        while read < record_len {
            let n = reader.read(&mut record[read..])?;
            if n == 0 {
                break;
            }
            read += n;
        }
        if read == 0 {
            break;
        }
        if read != record_len {
            let err = DecodedField::error(
                Some(idx),
                "<record>",
                Some(read),
                &record[..read],
                "E_RECORD_LENGTH",
                format!("truncated record: expected {record_len} bytes, got {read}"),
            );
            handle_record_error(schema, audit, sink, err)?;
            break;
        }
        process_record_bytes(schema, idx, &record, verify, audit, sink)?;
        record.fill(0);
        idx += 1;
    }
    Ok(())
}

fn process_hex_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let mut reader = BufReader::new(fs::File::open(input)?);
    let max_record_len = schema.record_length.unwrap_or(MAX_RECORD_BYTES);
    let mut line = String::new();
    let mut idx = 0usize;
    while read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? != 0 {
        if line.trim().is_empty() {
            continue;
        }
        if record_limit_reached(idx, limits) {
            break;
        }
        let record = match parse_hex_with_limit(&line, Some(max_record_len)) {
            Ok(record) => record,
            Err(err) => {
                let row = DecodedField::error(
                    Some(idx),
                    "<record>",
                    None,
                    line.as_bytes(),
                    err.code,
                    err.message,
                );
                handle_record_error(schema, audit, sink, row)?;
                idx += 1;
                continue;
            }
        };
        process_record_bytes(schema, idx, &record, verify, audit, sink)?;
        idx += 1;
    }
    Ok(())
}

fn process_csv_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let mut reader = BufReader::new(fs::File::open(input)?);
    let mut line = String::new();
    if read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? == 0 {
        return Ok(());
    }
    let headers = parse_csv_line(&line)?;
    let mut idx = 0usize;
    while read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? != 0 {
        if line.trim().is_empty() {
            continue;
        }
        if record_limit_reached(idx, limits) {
            break;
        }
        let record = match parse_csv_line(&line) {
            Ok(record) => record,
            Err(err) => {
                let row = DecodedField::error(
                    Some(idx),
                    "<record>",
                    None,
                    line.as_bytes(),
                    err.code,
                    err.message,
                );
                handle_record_error(schema, audit, sink, row)?;
                idx += 1;
                continue;
            }
        };
        if record.len() != headers.len() {
            let row = DecodedField::error(
                Some(idx),
                "<record>",
                None,
                line.as_bytes(),
                "E_CSV",
                format!(
                    "expected {} CSV fields, got {}",
                    headers.len(),
                    record.len()
                ),
            );
            handle_record_error(schema, audit, sink, row)?;
            idx += 1;
            continue;
        }
        let mut fields = BTreeMap::new();
        for (header, value) in headers.iter().zip(record.iter()) {
            fields.insert(header.to_string(), value.to_string());
        }
        process_named_hex_fields(schema, idx, &fields, &BTreeMap::new(), verify, audit, sink)?;
        idx += 1;
    }
    Ok(())
}

fn process_jsonl_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let mut reader = BufReader::new(fs::File::open(input)?);
    let mut line = String::new();
    let mut idx = 0usize;
    while read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? != 0 {
        if line.trim().is_empty() {
            continue;
        }
        if record_limit_reached(idx, limits) {
            break;
        }
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(err) => {
                let row = DecodedField::error(
                    Some(idx),
                    "<record>",
                    None,
                    line.as_bytes(),
                    "E_JSON",
                    err.to_string(),
                );
                handle_record_error(schema, audit, sink, row)?;
                idx += 1;
                continue;
            }
        };
        let Some(obj) = value.as_object() else {
            let row = DecodedField::error(
                Some(idx),
                "<record>",
                None,
                line.as_bytes(),
                "E_JSON",
                format!("record {idx} is not a JSON object"),
            );
            handle_record_error(schema, audit, sink, row)?;
            idx += 1;
            continue;
        };
        let mut fields = BTreeMap::new();
        let mut type_errors = BTreeMap::new();
        for (key, value) in obj {
            if let Some(s) = value.as_str() {
                fields.insert(key.clone(), s.to_string());
            } else if schema.fields.iter().any(|field| field.name == *key) {
                type_errors.insert(key.clone(), serde_json::to_string(value)?);
            }
        }
        process_named_hex_fields(schema, idx, &fields, &type_errors, verify, audit, sink)?;
        idx += 1;
    }
    Ok(())
}

fn process_record_bytes(
    schema: &Schema,
    idx: usize,
    record: &[u8],
    verify: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    if let Some(expected) = schema.record_length {
        if record.len() != expected {
            let err = DecodedField::error(
                Some(idx),
                "<record>",
                Some(record.len()),
                record,
                "E_RECORD_LENGTH",
                format!(
                    "record length mismatch: expected {expected} bytes, got {}",
                    record.len()
                ),
            );
            handle_record_error(schema, audit, sink, err)?;
            return Ok(());
        }
    }
    if schema.on_error != OnError::EmitErrorRow {
        return process_record_bytes_atomic(schema, idx, record, verify, audit, sink);
    }
    audit.records_seen += 1;
    let before_invalid = audit.fields_invalid;
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        let offset = plan.spec.offset.ok_or_else(|| {
            CliError::config(
                "E_SCHEMA",
                format!(
                    "field {} requires offset for fixed-width input",
                    plan.spec.name
                ),
            )
        })?;
        let length = plan.spec.length.unwrap_or(plan.expected_len);
        if offset
            .checked_add(length)
            .map_or(true, |end| end > record.len())
        {
            let err = DecodedField::error(
                Some(idx),
                &plan.spec.name,
                Some(offset),
                &[],
                "E_OFFSET",
                "field extends past record boundary",
            );
            if !handle_row_error(schema, audit, sink, err)? {
                break;
            }
            continue;
        }
        let bytes = &record[offset..offset + length];
        let row = decode_plan_field(Some(idx), &plan, Some(offset), bytes, verify);
        if !handle_decoded_row(schema, audit, sink, row)? {
            break;
        }
    }
    if audit.fields_invalid == before_invalid {
        audit.records_valid += 1;
    } else {
        audit.records_invalid += 1;
    }
    Ok(())
}

fn process_record_bytes_atomic(
    schema: &Schema,
    idx: usize,
    record: &[u8],
    verify: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    audit.records_seen += 1;
    let mut pending = Vec::new();
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        let offset = plan.spec.offset.ok_or_else(|| {
            CliError::config(
                "E_SCHEMA",
                format!(
                    "field {} requires offset for fixed-width input",
                    plan.spec.name
                ),
            )
        })?;
        let length = plan.spec.length.unwrap_or(plan.expected_len);
        if offset
            .checked_add(length)
            .map_or(true, |end| end > record.len())
        {
            let err = DecodedField::error(
                Some(idx),
                &plan.spec.name,
                Some(offset),
                &[],
                "E_OFFSET",
                "field extends past record boundary",
            );
            record_audit(audit, &err);
            audit.records_invalid += 1;
            return match schema.on_error {
                OnError::Fail => Err(CliError::data(
                    err.error_code.unwrap_or("E_DATA"),
                    err.message.unwrap_or_else(|| "data error".to_string()),
                )),
                OnError::SkipRecord => Ok(()),
                OnError::EmitErrorRow => Err(CliError::internal(
                    "emit-error-row reached atomic fixed-width processor",
                )),
            };
        }
        let bytes = &record[offset..offset + length];
        let row = decode_plan_field(Some(idx), &plan, Some(offset), bytes, verify);
        if !row.valid {
            let code = row.error_code.unwrap_or("E_DATA");
            let message = row
                .message
                .clone()
                .unwrap_or_else(|| "data error".to_string());
            record_audit(audit, &row);
            audit.records_invalid += 1;
            return match schema.on_error {
                OnError::Fail => Err(CliError::data(code, message)),
                OnError::SkipRecord => Ok(()),
                OnError::EmitErrorRow => Err(CliError::internal(
                    "emit-error-row reached atomic fixed-width processor",
                )),
            };
        }
        pending.push(row);
    }
    for row in pending {
        record_audit(audit, &row);
        sink.emit(row)?;
    }
    audit.records_valid += 1;
    Ok(())
}

fn process_named_hex_fields(
    schema: &Schema,
    idx: usize,
    fields: &BTreeMap<String, String>,
    type_errors: &BTreeMap<String, String>,
    verify: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    if schema.on_error != OnError::EmitErrorRow {
        return process_named_hex_fields_atomic(
            schema,
            idx,
            fields,
            type_errors,
            verify,
            audit,
            sink,
        );
    }
    audit.records_seen += 1;
    let before_invalid = audit.fields_invalid;
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        if let Some(raw) = type_errors.get(&plan.spec.name) {
            let err = DecodedField::error(
                Some(idx),
                &plan.spec.name,
                None,
                raw.as_bytes(),
                "E_JSON_TYPE",
                "JSONL field must be a string containing hex bytes",
            );
            if !handle_row_error(schema, audit, sink, err)? {
                break;
            }
            continue;
        }
        let Some(raw) = fields.get(&plan.spec.name) else {
            if plan.spec.required {
                let err = DecodedField::error(
                    Some(idx),
                    &plan.spec.name,
                    None,
                    &[],
                    "E_REQUIRED",
                    "required field is missing",
                );
                if !handle_row_error(schema, audit, sink, err)? {
                    break;
                }
            }
            continue;
        };
        let bytes = match parse_hex_with_limit(raw, Some(plan.expected_len)) {
            Ok(bytes) => bytes,
            Err(err) => {
                let row = DecodedField::error(
                    Some(idx),
                    &plan.spec.name,
                    None,
                    &[],
                    err.code,
                    err.message,
                );
                if !handle_row_error(schema, audit, sink, row)? {
                    break;
                }
                continue;
            }
        };
        let row = decode_plan_field(Some(idx), &plan, None, &bytes, verify);
        if !handle_decoded_row(schema, audit, sink, row)? {
            break;
        }
    }
    if audit.fields_invalid == before_invalid {
        audit.records_valid += 1;
    } else {
        audit.records_invalid += 1;
    }
    Ok(())
}

fn process_named_hex_fields_atomic(
    schema: &Schema,
    idx: usize,
    fields: &BTreeMap<String, String>,
    type_errors: &BTreeMap<String, String>,
    verify: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    audit.records_seen += 1;
    let mut pending = Vec::new();
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        if let Some(raw) = type_errors.get(&plan.spec.name) {
            let err = DecodedField::error(
                Some(idx),
                &plan.spec.name,
                None,
                raw.as_bytes(),
                "E_JSON_TYPE",
                "JSONL field must be a string containing hex bytes",
            );
            let code = err.error_code.unwrap_or("E_DATA");
            let message = err
                .message
                .clone()
                .unwrap_or_else(|| "data error".to_string());
            record_audit(audit, &err);
            audit.records_invalid += 1;
            return match schema.on_error {
                OnError::Fail => Err(CliError::data(code, message)),
                OnError::SkipRecord => Ok(()),
                OnError::EmitErrorRow => Err(CliError::internal(
                    "emit-error-row reached atomic named-field processor",
                )),
            };
        }
        let Some(raw) = fields.get(&plan.spec.name) else {
            if plan.spec.required {
                let err = DecodedField::error(
                    Some(idx),
                    &plan.spec.name,
                    None,
                    &[],
                    "E_REQUIRED",
                    "required field is missing",
                );
                record_audit(audit, &err);
                audit.records_invalid += 1;
                return match schema.on_error {
                    OnError::Fail => Err(CliError::data(
                        err.error_code.unwrap_or("E_DATA"),
                        err.message.unwrap_or_else(|| "data error".to_string()),
                    )),
                    OnError::SkipRecord => Ok(()),
                    OnError::EmitErrorRow => Err(CliError::internal(
                        "emit-error-row reached atomic named-field processor",
                    )),
                };
            }
            continue;
        };
        let bytes = match parse_hex_with_limit(raw, Some(plan.expected_len)) {
            Ok(bytes) => bytes,
            Err(err) => {
                let row = DecodedField::error(
                    Some(idx),
                    &plan.spec.name,
                    None,
                    &[],
                    err.code,
                    err.message,
                );
                let code = row.error_code.unwrap_or("E_DATA");
                let message = row
                    .message
                    .clone()
                    .unwrap_or_else(|| "data error".to_string());
                record_audit(audit, &row);
                audit.records_invalid += 1;
                return match schema.on_error {
                    OnError::Fail => Err(CliError::data(code, message)),
                    OnError::SkipRecord => Ok(()),
                    OnError::EmitErrorRow => Err(CliError::internal(
                        "emit-error-row reached atomic named-field processor",
                    )),
                };
            }
        };
        let row = decode_plan_field(Some(idx), &plan, None, &bytes, verify);
        if !row.valid {
            let code = row.error_code.unwrap_or("E_DATA");
            let message = row
                .message
                .clone()
                .unwrap_or_else(|| "data error".to_string());
            record_audit(audit, &row);
            audit.records_invalid += 1;
            return match schema.on_error {
                OnError::Fail => Err(CliError::data(code, message)),
                OnError::SkipRecord => Ok(()),
                OnError::EmitErrorRow => Err(CliError::internal(
                    "emit-error-row reached atomic named-field processor",
                )),
            };
        }
        pending.push(row);
    }
    for row in pending {
        record_audit(audit, &row);
        sink.emit(row)?;
    }
    audit.records_valid += 1;
    Ok(())
}

fn decode_plan_field(
    record_index: Option<usize>,
    plan: &FieldPlan<'_>,
    offset: Option<usize>,
    bytes: &[u8],
    verify: bool,
) -> DecodedField {
    let mut row = decode_named_field(
        record_index,
        &plan.spec.name,
        offset,
        bytes,
        &plan.cfg,
        plan.spec.sign_mode.to_core(),
        plan.spec.mode,
    );
    if verify && row.valid {
        let verified = match from_packed_lossless(bytes, &plan.cfg, plan.spec.sign_mode.to_core()) {
            Ok(loss) => to_packed_lossless(&loss, &plan.cfg)
                .map(|rebuilt| rebuilt == bytes)
                .unwrap_or(false),
            Err(_) => false,
        };
        if !verified {
            row.valid = false;
            row.error_code = Some("E_VERIFY");
            row.message = Some("lossless re-encode did not match original bytes".to_string());
            row.recoverable = true;
        }
    }
    row
}

fn decode_named_field(
    record_index: Option<usize>,
    name: &str,
    offset: Option<usize>,
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
    mode: FieldMode,
) -> DecodedField {
    let sign_nibble = bytes.last().map(|b| b & 0x0F);
    let sign_class = sign_nibble.map(|n| classify_sign(n, cfg.is_signed()).to_string());
    let result = match mode {
        FieldMode::Canonical => from_packed(bytes, cfg, sign_mode),
        FieldMode::Lossless => from_packed_lossless(bytes, cfg, sign_mode).map(|loss| loss.value),
    };
    match result {
        Ok(value) => {
            let (raw_hex, raw_hex_truncated) = raw_hex_for_output(bytes);
            DecodedField {
                version: OUTPUT_VERSION,
                record_index,
                field: name.to_string(),
                offset,
                raw_hex,
                raw_byte_len: bytes.len(),
                raw_hex_truncated,
                value: Some(value.to_string()),
                sign_nibble: sign_nibble.map(|n| format!("0x{n:X}")),
                sign_class,
                valid: true,
                error_code: None,
                message: None,
                recoverable: false,
            }
        }
        Err(err) => DecodedField::packed_error(record_index, name, offset, bytes, err),
    }
}

impl DecodedField {
    fn packed_error(
        record_index: Option<usize>,
        field: &str,
        offset: Option<usize>,
        bytes: &[u8],
        err: PackedError,
    ) -> Self {
        let code = packed_error_code(&err);
        Self::error(record_index, field, offset, bytes, code, err.to_string())
    }

    fn error(
        record_index: Option<usize>,
        field: &str,
        offset: Option<usize>,
        bytes: &[u8],
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        let (raw_hex, raw_hex_truncated) = raw_hex_for_output(bytes);
        Self {
            version: OUTPUT_VERSION,
            record_index,
            field: field.to_string(),
            offset,
            raw_hex,
            raw_byte_len: bytes.len(),
            raw_hex_truncated,
            value: None,
            sign_nibble: bytes.last().map(|b| format!("0x{:X}", b & 0x0F)),
            sign_class: bytes
                .last()
                .map(|b| classify_sign(b & 0x0F, true).to_string()),
            valid: false,
            error_code: Some(code),
            message: Some(message.into()),
            recoverable: true,
        }
    }
}

fn record_audit(audit: &mut AuditReport, row: &DecodedField) {
    audit.fields_seen += 1;
    let profile = audit.field_profiles.entry(row.field.clone()).or_default();
    profile.fields_seen += 1;
    if row.valid {
        audit.fields_valid += 1;
        profile.fields_valid += 1;
        if let Some(value) = row.value.as_deref() {
            update_profile_range(profile, value);
        }
    } else {
        audit.fields_invalid += 1;
        profile.fields_invalid += 1;
        if let Some(code) = row.error_code {
            *audit
                .error_distribution
                .entry(code.to_string())
                .or_insert(0) += 1;
            *profile
                .error_distribution
                .entry(code.to_string())
                .or_insert(0) += 1;
        }
        if audit.failure_samples.len() < audit.failure_sample_limit {
            audit.failure_samples.push(row.clone_for_sample());
        }
    }
    if let Some(sign) = &row.sign_nibble {
        *audit.sign_distribution.entry(sign.clone()).or_insert(0) += 1;
        *profile.sign_distribution.entry(sign.clone()).or_insert(0) += 1;
        if matches!(sign.as_str(), "0xA" | "0xB" | "0xE") {
            audit.non_preferred_sign_count += 1;
            profile.non_preferred_sign_count += 1;
        }
        if row.value.as_deref().is_some_and(decimal_text_is_zero)
            && matches!(sign.as_str(), "0xB" | "0xD")
        {
            audit.negative_zero_count += 1;
            profile.negative_zero_count += 1;
        }
    }
}

fn decimal_text_is_zero(value_text: &str) -> bool {
    Decimal::from_str(value_text).is_ok_and(|value| value == Decimal::new(0, 0))
}

fn update_profile_range(profile: &mut FieldAuditSummary, value_text: &str) {
    let Ok(value) = Decimal::from_str(value_text) else {
        return;
    };
    let update_min = profile
        .min_value
        .as_deref()
        .and_then(|current| Decimal::from_str(current).ok())
        .map_or(true, |current| value < current);
    if update_min {
        profile.min_value = Some(value.to_string());
    }
    let update_max = profile
        .max_value
        .as_deref()
        .and_then(|current| Decimal::from_str(current).ok())
        .map_or(true, |current| value > current);
    if update_max {
        profile.max_value = Some(value.to_string());
    }
}

impl DecodedField {
    fn clone_for_sample(&self) -> Self {
        Self {
            version: self.version,
            record_index: self.record_index,
            field: self.field.clone(),
            offset: self.offset,
            raw_hex: self.raw_hex.clone(),
            raw_byte_len: self.raw_byte_len,
            raw_hex_truncated: self.raw_hex_truncated,
            value: self.value.clone(),
            sign_nibble: self.sign_nibble.clone(),
            sign_class: self.sign_class.clone(),
            valid: self.valid,
            error_code: self.error_code,
            message: self.message.clone(),
            recoverable: self.recoverable,
        }
    }
}

fn handle_row_error(
    schema: &Schema,
    audit: &mut AuditReport,
    sink: &mut RowSink,
    err: DecodedField,
) -> Result<bool, CliError> {
    record_audit(audit, &err);
    match schema.on_error {
        OnError::Fail => Err(CliError::data(
            err.error_code.unwrap_or("E_DATA"),
            err.message.unwrap_or_else(|| "data error".to_string()),
        )),
        OnError::SkipRecord => Ok(false),
        OnError::EmitErrorRow => {
            sink.emit(err)?;
            Ok(true)
        }
    }
}

fn handle_record_error(
    schema: &Schema,
    audit: &mut AuditReport,
    sink: &mut RowSink,
    err: DecodedField,
) -> Result<bool, CliError> {
    audit.records_seen += 1;
    audit.records_invalid += 1;
    handle_row_error(schema, audit, sink, err)
}

fn handle_decoded_row(
    schema: &Schema,
    audit: &mut AuditReport,
    sink: &mut RowSink,
    row: DecodedField,
) -> Result<bool, CliError> {
    if row.valid {
        record_audit(audit, &row);
        sink.emit(row)?;
        return Ok(true);
    }
    match schema.on_error {
        OnError::Fail => {
            let code = row.error_code.unwrap_or("E_DATA");
            let message = row
                .message
                .clone()
                .unwrap_or_else(|| "data error".to_string());
            record_audit(audit, &row);
            Err(CliError::data(code, message))
        }
        OnError::SkipRecord => {
            record_audit(audit, &row);
            Ok(false)
        }
        OnError::EmitErrorRow => {
            record_audit(audit, &row);
            sink.emit(row)?;
            Ok(true)
        }
    }
}

fn cfg_from_shape(args: &FieldShapeArgs) -> Result<PackedConfig, CliError> {
    if args.signed == args.unsigned {
        return Err(CliError::config(
            "E_SCHEMA",
            "choose exactly one of --signed or --unsigned",
        ));
    }
    let signed = if args.unsigned { false } else { args.signed };
    PackedConfig::new(args.digits, args.scale, signed).map_err(map_packed_error)
}

fn read_field_input(
    hex: Option<&str>,
    file: Option<&PathBuf>,
    stdin: bool,
    offset: u64,
    expected_len: usize,
) -> Result<Vec<u8>, CliError> {
    if expected_len > MAX_SINGLE_FIELD_BYTES {
        return Err(CliError::internal("field length exceeds compiled maximum"));
    }
    let source_count =
        usize::from(hex.is_some()) + usize::from(file.is_some()) + usize::from(stdin);
    if source_count != 1 {
        return Err(CliError::config(
            "E_INPUT",
            "choose exactly one of --hex, --file, or --stdin",
        ));
    }
    let bytes = if let Some(raw) = hex {
        parse_hex_with_limit(raw, Some(expected_len))?
    } else if let Some(path) = file {
        let mut f = fs::File::open(path)?;
        if offset > 0 {
            f.seek(SeekFrom::Start(offset))?;
        }
        let mut buf = vec![0u8; expected_len];
        f.read_exact(&mut buf)?;
        buf
    } else if stdin {
        let mut input = String::new();
        let max_read = (expected_len.saturating_mul(8).saturating_add(128)) as u64;
        io::stdin().take(max_read + 1).read_to_string(&mut input)?;
        if input.len() as u64 > max_read {
            return Err(CliError::data(
                "E_LENGTH",
                format!("stdin field input exceeds {max_read} bytes"),
            ));
        }
        parse_hex_with_limit(&input, Some(expected_len))?
    } else {
        return Err(CliError::config(
            "E_INPUT",
            "choose exactly one of --hex, --file, or --stdin",
        ));
    };
    if bytes.len() != expected_len {
        return Err(CliError::data(
            "E_LENGTH",
            format!("expected {expected_len} bytes, got {}", bytes.len()),
        ));
    }
    Ok(bytes)
}

fn load_schema(path: &Path) -> Result<(Schema, SchemaHashes), CliError> {
    let meta = fs::metadata(path)?;
    if meta.len() > MAX_SCHEMA_BYTES {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("schema exceeds {MAX_SCHEMA_BYTES} bytes"),
        ));
    }
    let raw = fs::read(path)?;
    let file_sha256 = sha256_hex(&raw);
    let text = std::str::from_utf8(&raw)
        .map_err(|err| CliError::config("E_SCHEMA", format!("schema is not UTF-8: {err}")))?;
    let schema: Schema = match path.extension().and_then(|s| s.to_str()) {
        Some("json") => serde_json::from_str(text).map_err(|err| {
            CliError::config(
                "E_SCHEMA",
                format!(
                    "expected JSON schema at {} but parse failed: {err}",
                    path.display()
                ),
            )
        })?,
        Some("toml") => toml::from_str(text).map_err(|err| {
            CliError::config(
                "E_SCHEMA",
                format!(
                    "expected TOML schema at {} but parse failed: {err}",
                    path.display()
                ),
            )
        })?,
        _ => {
            return Err(CliError::config(
                "E_SCHEMA",
                "schema extension must be .json or .toml",
            ))
        }
    };
    validate_schema(&schema)?;
    let canonical = semantic_schema_bytes(&schema)?;
    Ok((
        schema,
        SchemaHashes {
            file_sha256,
            semantic_sha256: sha256_hex(&canonical),
        },
    ))
}

fn semantic_schema_bytes(schema: &Schema) -> Result<Vec<u8>, CliError> {
    if schema.version == 2 {
        return record::semantic_schema_bytes_v2(schema);
    }
    let semantic = SemanticSchema {
        version: schema.version,
        record_length: schema.record_length,
        input_encoding: schema.input_encoding,
        on_error: schema.on_error,
        output: schema.output,
        verification_scope: schema.verification_scope,
        fillers: schema
            .fillers
            .iter()
            .map(|filler| SemanticFiller {
                name: &filler.name,
                offset: filler.offset,
                length: filler.length,
            })
            .collect(),
        fields: schema
            .fields
            .iter()
            .map(|field| SemanticField {
                name: &field.name,
                offset: field.offset,
                length: field.length,
                total_digits: field.total_digits,
                scale: field.scale,
                signed: field.signed,
                sign_mode: field.sign_mode,
                mode: field.mode,
                required: field.required,
            })
            .collect(),
    };
    serde_json::to_vec(&semantic).map_err(|err| {
        CliError::internal(format!("failed to canonicalize validated schema: {err}"))
    })
}

fn validate_schema(schema: &Schema) -> Result<(), CliError> {
    if schema.version == 2 {
        return record::validate_v2_schema(schema);
    }
    if schema.version != 1 {
        return Err(CliError::config(
            "E_SCHEMA",
            "schema version must be 1 or 2",
        ));
    }
    if schema.fields.is_empty() {
        return Err(CliError::config(
            "E_SCHEMA",
            "schema requires at least one field",
        ));
    }
    if schema.fields.len().saturating_add(schema.fillers.len()) > MAX_SCHEMA_FIELDS {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("schema supports at most {MAX_SCHEMA_FIELDS} fields plus fillers combined"),
        ));
    }
    let fixed_width = matches!(
        schema.input_encoding,
        InputEncoding::Binary | InputEncoding::Hex
    );
    if matches!(
        schema.input_encoding,
        InputEncoding::Csv | InputEncoding::Jsonl
    ) && schema.record_length.is_some()
    {
        return Err(CliError::config(
            "E_SCHEMA",
            format!(
                "record_length is only valid for fixed-width binary or hex input, not {}",
                schema.input_encoding
            ),
        ));
    }
    if !fixed_width && !schema.fillers.is_empty() {
        return Err(CliError::config(
            "E_SCHEMA",
            "fillers are only valid for fixed-width binary or hex input",
        ));
    }
    if !fixed_width && schema.verification_scope == VerificationScope::Record {
        return Err(CliError::config(
            "E_SCHEMA",
            "record verification scope requires fixed-width binary or hex input",
        ));
    }
    if let Some(record_length) = schema.record_length {
        if record_length > MAX_RECORD_BYTES {
            return Err(CliError::config(
                "E_SCHEMA",
                format!("record_length exceeds {MAX_RECORD_BYTES} bytes"),
            ));
        }
    }
    let mut names = BTreeSet::new();
    let mut spans = Vec::new();
    for field in &schema.fields {
        let _ = &field.description;
        validate_field_name(&field.name)?;
        if !names.insert(field.name.clone()) {
            return Err(CliError::config(
                "E_SCHEMA",
                format!("duplicate field name {}", field.name),
            ));
        }
        let plan = plan_field(field)?;
        let expected = plan.expected_len;
        if let Some(length) = field.length {
            if length != expected {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "field {} length {length} does not match expected packed length {expected}",
                        field.name
                    ),
                ));
            }
        }
        if fixed_width {
            let offset = field.offset.ok_or_else(|| {
                CliError::config("E_SCHEMA", format!("field {} requires offset", field.name))
            })?;
            let length = field.length.unwrap_or(expected);
            let end = offset.checked_add(length).ok_or_else(|| {
                CliError::config("E_SCHEMA", format!("field {} offset overflows", field.name))
            })?;
            let record_length = schema.record_length.ok_or_else(|| {
                CliError::config("E_SCHEMA", "fixed-width schemas require record_length")
            })?;
            if end > record_length {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("field {} extends past record_length", field.name),
                ));
            }
            spans.push((offset, end, field.name.clone()));
        } else if field.offset.is_some() {
            return Err(CliError::config(
                "E_SCHEMA",
                format!(
                    "field {} uses offset but {} input is name-based",
                    field.name, schema.input_encoding
                ),
            ));
        }
    }
    if fixed_width {
        let record_length = schema.record_length.ok_or_else(|| {
            CliError::config("E_SCHEMA", "fixed-width schemas require record_length")
        })?;
        for filler in &schema.fillers {
            let _ = &filler.description;
            validate_field_name(&filler.name)?;
            if !names.insert(filler.name.clone()) {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("duplicate field or filler name {}", filler.name),
                ));
            }
            if filler.length == 0 {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("filler {} length must be greater than zero", filler.name),
                ));
            }
            let end = filler.offset.checked_add(filler.length).ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("filler {} offset overflows", filler.name),
                )
            })?;
            if end > record_length {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("filler {} extends past record_length", filler.name),
                ));
            }
            spans.push((filler.offset, end, filler.name.clone()));
        }
    }
    spans.sort_by_key(|(start, _, _)| *start);
    for pair in spans.windows(2) {
        let (_, prev_end, prev_name) = &pair[0];
        let (next_start, _, next_name) = &pair[1];
        if prev_end > next_start {
            return Err(CliError::config(
                "E_SCHEMA",
                format!("fields {prev_name} and {next_name} overlap"),
            ));
        }
    }
    if fixed_width && schema.verification_scope == VerificationScope::Record {
        let coverage = schema_coverage_summary(schema)?;
        if coverage.full_coverage != Some(true) {
            return Err(CliError::config(
                "E_SCHEMA",
                "record verification scope requires fields and fillers to cover every record byte",
            ));
        }
    }
    Ok(())
}

fn validate_field_name(name: &str) -> Result<(), CliError> {
    if name.is_empty() || name == "<record>" || name.len() > 128 {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("invalid field name {name:?}"),
        ));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("field name {name:?} must use only ASCII letters, numbers, dot, underscore, or hyphen"),
        ));
    }
    Ok(())
}

fn plan_field(spec: &FieldSpec) -> Result<FieldPlan<'_>, CliError> {
    let cfg =
        PackedConfig::new(spec.total_digits, spec.scale, spec.signed).map_err(map_packed_error)?;
    Ok(FieldPlan {
        spec,
        expected_len: cfg.byte_len(),
        cfg,
    })
}

fn limits_from_args(args: &BatchInputArgs) -> Result<ProcessingLimits, CliError> {
    if matches!(args.max_records, Some(0)) {
        return Err(CliError::config(
            "E_CONFIG",
            "--max-records must be greater than zero",
        ));
    }
    if args.sample_failures > MAX_FAILURE_SAMPLE_LIMIT {
        return Err(CliError::config(
            "E_CONFIG",
            format!("--sample-failures must be at most {MAX_FAILURE_SAMPLE_LIMIT}"),
        ));
    }
    Ok(ProcessingLimits {
        max_records: args.max_records,
    })
}

fn verification_scope_from_args(
    schema: &Schema,
    args: &BatchInputArgs,
) -> Result<VerificationScope, CliError> {
    let scope = if args.strict_record {
        VerificationScope::Record
    } else {
        schema.verification_scope
    };
    if scope == VerificationScope::Record
        && !matches!(
            schema.input_encoding,
            InputEncoding::Binary | InputEncoding::Hex
        )
    {
        return Err(CliError::config(
            "E_SCHEMA",
            "record verification scope requires binary or hex fixed-width input",
        ));
    }
    Ok(scope)
}

fn record_limit_reached(idx: usize, limits: ProcessingLimits) -> bool {
    limits.max_records.is_some_and(|max| idx >= max)
}

fn schema_field_summaries(schema: &Schema) -> Result<Vec<FieldPlanSummary<'_>>, CliError> {
    let mut out = Vec::with_capacity(schema.fields.len());
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        let length = spec.length.unwrap_or(plan.expected_len);
        let end_offset = spec.offset.and_then(|offset| offset.checked_add(length));
        out.push(FieldPlanSummary {
            name: &spec.name,
            offset: spec.offset,
            end_offset,
            length,
            total_digits: spec.total_digits,
            scale: spec.scale,
            signed: spec.signed,
            sign_mode: spec.sign_mode,
            mode: spec.mode,
            required: spec.required,
        });
    }
    Ok(out)
}

fn schema_coverage_summary(schema: &Schema) -> Result<SchemaCoverageSummary, CliError> {
    if schema.version == 2 {
        return record::schema_coverage_summary_v2(schema);
    }
    if !matches!(
        schema.input_encoding,
        InputEncoding::Binary | InputEncoding::Hex
    ) {
        return Ok(SchemaCoverageSummary {
            record_length: schema.record_length,
            covered_bytes: 0,
            gap_bytes: None,
            full_coverage: None,
            overlap_count: 0,
            first_offset: None,
            last_end_offset: None,
            ranges: Vec::new(),
            gaps: Vec::new(),
            overlaps: Vec::new(),
        });
    }

    let mut ranges = Vec::new();
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        let offset = spec.offset.unwrap_or(0);
        let length = spec.length.unwrap_or(plan.expected_len);
        ranges.push(LayoutRangeSummary {
            name: spec.name.clone(),
            kind: LayoutKind::Field,
            offset,
            end_offset: offset.saturating_add(length),
            length,
        });
    }
    for filler in &schema.fillers {
        ranges.push(LayoutRangeSummary {
            name: filler.name.clone(),
            kind: LayoutKind::Filler,
            offset: filler.offset,
            end_offset: filler.offset.saturating_add(filler.length),
            length: filler.length,
        });
    }
    ranges.sort_by_key(|range| (range.offset, range.end_offset, range.name.clone()));

    let first = ranges.first().map(|range| range.offset);
    let last = ranges.iter().map(|range| range.end_offset).max();
    let mut covered = 0usize;
    let mut cursor = 0usize;
    let mut gaps = Vec::new();
    let mut overlaps = Vec::new();
    let mut previous_name = String::new();
    for range in &ranges {
        if range.offset > cursor {
            gaps.push(LayoutGapSummary {
                offset: cursor,
                end_offset: range.offset,
                length: range.offset - cursor,
            });
        } else if range.offset < cursor {
            let overlap_end = cursor.min(range.end_offset);
            if overlap_end > range.offset {
                overlaps.push(LayoutOverlapSummary {
                    offset: range.offset,
                    end_offset: overlap_end,
                    length: overlap_end - range.offset,
                    previous: previous_name.clone(),
                    current: range.name.clone(),
                });
            }
        }
        if range.end_offset > cursor {
            covered = covered.saturating_add(range.end_offset - cursor.max(range.offset));
            cursor = range.end_offset;
            previous_name = range.name.clone();
        }
    }
    if let Some(record_len) = schema.record_length {
        if cursor < record_len {
            gaps.push(LayoutGapSummary {
                offset: cursor,
                end_offset: record_len,
                length: record_len - cursor,
            });
        }
    }
    let gap_bytes = schema
        .record_length
        .map(|_| gaps.iter().map(|gap| gap.length).sum());
    let full_coverage = schema
        .record_length
        .map(|record_len| gaps.is_empty() && overlaps.is_empty() && covered == record_len);
    let overlap_count = overlaps.len();
    Ok(SchemaCoverageSummary {
        record_length: schema.record_length,
        covered_bytes: covered,
        gap_bytes,
        full_coverage,
        overlap_count,
        first_offset: first,
        last_end_offset: last,
        ranges,
        gaps,
        overlaps,
    })
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    out: &mut String,
    max_bytes: usize,
) -> Result<usize, CliError> {
    out.clear();
    let mut total = 0usize;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(total);
        }
        let take = match available.iter().position(|b| *b == b'\n') {
            Some(pos) => pos + 1,
            None => available.len(),
        };
        total = total
            .checked_add(take)
            .ok_or_else(|| CliError::data("E_LENGTH", "line length overflow"))?;
        if total > max_bytes {
            return Err(CliError::data(
                "E_LENGTH",
                format!("input line exceeds {max_bytes} bytes"),
            ));
        }
        let chunk = std::str::from_utf8(&available[..take]).map_err(|err| {
            CliError::data("E_ENCODING", format!("input line is not UTF-8: {err}"))
        })?;
        let ended = chunk.ends_with('\n');
        out.push_str(chunk);
        reader.consume(take);
        if ended {
            return Ok(total);
        }
    }
}

fn parse_hex_with_limit(input: &str, max_bytes: Option<usize>) -> Result<Vec<u8>, CliError> {
    if let Some(max) = max_bytes {
        let text_limit = max.saturating_mul(8).saturating_add(128);
        if input.len() > text_limit {
            return Err(CliError::data(
                "E_LENGTH",
                format!("hex text exceeds {text_limit} bytes for a {max}-byte field"),
            ));
        }
    }
    let mut digits = String::new();
    let mut chars = input.trim().chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '0' && matches!(chars.peek(), Some('x' | 'X')) {
            let _ = chars.next();
            continue;
        }
        if ch.is_ascii_hexdigit() {
            digits.push(ch);
            if let Some(max) = max_bytes {
                if digits.len() / 2 > max {
                    return Err(CliError::data(
                        "E_LENGTH",
                        format!("hex input exceeds {max} bytes"),
                    ));
                }
            }
        } else if ch.is_ascii_whitespace() || matches!(ch, ',' | '_' | '-' | ':') {
            continue;
        } else {
            return Err(CliError::data(
                "E_HEX",
                format!("invalid hex character {ch}"),
            ));
        }
    }
    if digits.len() % 2 != 0 {
        return Err(CliError::data(
            "E_HEX",
            "hex input must contain an even number of digits",
        ));
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    let bytes = digits.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_value(bytes[i])?;
        let lo = hex_value(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn parse_csv_line(line: &str) -> Result<csv::StringRecord, CliError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(line.as_bytes());
    let mut records = reader.records();
    let Some(record) = records.next() else {
        return Ok(csv::StringRecord::new());
    };
    let record = record.map_err(|err| CliError::data("E_CSV", err.to_string()))?;
    if records
        .next()
        .transpose()
        .map_err(|err| CliError::data("E_CSV", err.to_string()))?
        .is_some()
    {
        return Err(CliError::data(
            "E_CSV",
            "CSV records must fit on one physical line",
        ));
    }
    Ok(record)
}

fn hex_value(ch: u8) -> Result<u8, CliError> {
    match ch {
        b'0'..=b'9' => Ok(ch - b'0'),
        b'a'..=b'f' => Ok(ch - b'a' + 10),
        b'A'..=b'F' => Ok(ch - b'A' + 10),
        _ => Err(CliError::data("E_HEX", "invalid hex digit")),
    }
}

fn parse_sign_nibble(input: &str) -> Result<u8, CliError> {
    let trimmed = input.trim();
    let raw = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if raw.len() == 1 {
        return hex_value(raw.as_bytes()[0]);
    }
    let bytes = parse_hex_with_limit(input, Some(1))?;
    match bytes.as_slice() {
        [value] if *value <= 0x0F => Ok(*value),
        [value] if (*value & 0xF0) == 0 => Ok(*value & 0x0F),
        _ => Err(CliError::data(
            "E_SIGN",
            "sign nibble must be a single hex nibble",
        )),
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join("")
}

fn raw_hex_for_output(bytes: &[u8]) -> (String, bool) {
    if bytes.len() <= MAX_ERROR_RAW_BYTES {
        (to_hex(bytes), false)
    } else {
        (to_hex(&bytes[..MAX_ERROR_RAW_BYTES]), true)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex(&hasher.finalize())
}

fn new_audit(schema: &Schema, ctx: AuditContext<'_>) -> Result<AuditReport, CliError> {
    let input_meta = fs::metadata(ctx.input)?;
    let record_coverage = schema_coverage_summary(schema)?;
    let schema_field_count = if schema.version == 2 {
        record::schema_field_count(schema)?
    } else {
        schema.fields.len()
    };
    let schema_filler_count = if schema.version == 2 {
        record::schema_filler_count(schema)?
    } else {
        schema.fillers.len()
    };
    Ok(AuditReport {
        version: OUTPUT_VERSION,
        tool: TOOL_NAME.to_string(),
        tool_version: TOOL_VERSION.to_string(),
        command: ctx.command.to_string(),
        evidence_mode: ctx.evidence_mode,
        schema_hash: ctx.hashes.semantic_sha256.clone(),
        schema_file_sha256: ctx.hashes.file_sha256.clone(),
        schema_field_count,
        schema_filler_count,
        schema_record_length: schema.record_length,
        schema_input_encoding: schema.input_encoding,
        verification_scope: ctx.verification_scope,
        record_coverage,
        input_path: ctx.input.display().to_string(),
        input_size_bytes: input_meta.len(),
        input_sha256: if ctx.include_input_hash {
            Some(sha256_file(ctx.input)?)
        } else {
            None
        },
        runtime: runtime_evidence(ctx.evidence_mode, ctx.evidence_argv)?,
        record_limit: ctx.limits.max_records,
        failure_sample_limit: ctx.failure_sample_limit,
        records_seen: 0,
        records_valid: 0,
        records_invalid: 0,
        fields_seen: 0,
        fields_valid: 0,
        fields_invalid: 0,
        status: AuditStatus::Empty,
        field_byte_for_byte_verified: None,
        record_byte_for_byte_verified: None,
        byte_for_byte_verified: None,
        negative_zero_count: 0,
        non_preferred_sign_count: 0,
        sign_distribution: BTreeMap::new(),
        error_distribution: BTreeMap::new(),
        field_profiles: BTreeMap::new(),
        failure_samples: Vec::new(),
    })
}

fn finalize_audit(audit: &mut AuditReport) {
    audit.status = if audit.records_seen == 0 || audit.fields_seen == 0 {
        AuditStatus::Empty
    } else if audit.records_invalid == 0 && audit.fields_invalid == 0 {
        AuditStatus::Passed
    } else {
        AuditStatus::Failed
    };
}

fn finalize_verify_audit(audit: &mut AuditReport) {
    let field_verified = matches!(audit.status, AuditStatus::Passed);
    audit.field_byte_for_byte_verified = Some(field_verified);
    let record_verified = if audit.verification_scope == VerificationScope::Record {
        Some(
            field_verified
                && audit.record_coverage.full_coverage == Some(true)
                && audit.record_coverage.overlap_count == 0,
        )
    } else {
        None
    };
    audit.record_byte_for_byte_verified = record_verified;
    audit.byte_for_byte_verified = Some(record_verified.unwrap_or(field_verified));
    if audit.byte_for_byte_verified == Some(false) && matches!(audit.status, AuditStatus::Passed) {
        audit.status = AuditStatus::Failed;
    }
}

fn runtime_evidence(
    mode: EvidenceMode,
    argv_mode: EvidenceArgv,
) -> Result<Option<RuntimeEvidence>, CliError> {
    if mode != EvidenceMode::Full {
        return Ok(None);
    }
    let cwd = env::current_dir()?.display().to_string();
    let generated_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| CliError::internal(format!("system clock is before UNIX epoch: {err}")))?
        .as_secs();
    let argv = match argv_mode {
        EvidenceArgv::Redacted => Some(redact_argv(env::args())),
        EvidenceArgv::Raw => Some(env::args().collect()),
        EvidenceArgv::Omit => None,
    };
    Ok(Some(RuntimeEvidence {
        argv,
        argv_redacted: argv_mode == EvidenceArgv::Redacted,
        cwd,
        os: env::consts::OS,
        arch: env::consts::ARCH,
        family: env::consts::FAMILY,
        exe_suffix: env::consts::EXE_SUFFIX,
        generated_unix_seconds,
    }))
}

fn redact_argv(args: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut redacted = Vec::new();
    let mut redact_next_value = false;
    for arg in args {
        if redact_next_value {
            if arg.starts_with('-') {
                redact_next_value = false;
            } else {
                redacted.push("<redacted>".to_string());
                redact_next_value = false;
                continue;
            }
        }
        if let Some((flag, _)) = arg.split_once('=') {
            if flag.starts_with("--") {
                redacted.push(format!("{flag}=<redacted>"));
                continue;
            }
        }
        if arg.starts_with("--") {
            redact_next_value = true;
        }
        redacted.push(arg);
    }
    redacted
}

fn sha256_file(path: &Path) -> Result<String, CliError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

fn render_fields(rows: &[DecodedField], format: OutputFormat) -> Result<(), CliError> {
    match format {
        OutputFormat::Json => render_json(rows),
        OutputFormat::Jsonl => {
            for row in rows {
                println!("{}", serde_json::to_string(row)?);
            }
            Ok(())
        }
        OutputFormat::Csv => {
            let mut writer = csv::Writer::from_writer(io::stdout());
            for row in rows {
                writer
                    .serialize(row)
                    .map_err(|err| CliError::data("E_CSV", err.to_string()))?;
            }
            writer.flush()?;
            Ok(())
        }
        OutputFormat::Audit => render_json(rows),
        OutputFormat::Table => {
            println!("record\tfield\toffset\tvalid\tvalue\tsign\traw_hex\terror");
            for row in rows {
                print_table_row(row);
            }
            Ok(())
        }
    }
}

fn print_table_row(row: &DecodedField) {
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        row.record_index
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string()),
        row.field,
        row.offset
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string()),
        row.valid,
        row.value.as_deref().unwrap_or("-"),
        row.sign_nibble.as_deref().unwrap_or("-"),
        row.raw_hex,
        row.error_code.unwrap_or("-"),
    );
}

fn render_inspect(
    bytes: &[u8],
    field: &DecodedField,
    format: OutputFormat,
) -> Result<(), CliError> {
    let nibbles: Vec<String> = nibble_iter(bytes).map(|n| format!("{n:X}")).collect();
    let value = serde_json::json!({
        "version": OUTPUT_VERSION,
        "raw_hex": to_hex(bytes),
        "byte_len": bytes.len(),
        "nibbles": nibbles,
        "sign_nibble": field.sign_nibble,
        "sign_class": field.sign_class,
        "valid": field.valid,
        "value": field.value,
        "error_code": field.error_code,
        "message": field.message,
    });
    render_value(&value, format)
}

fn render_value(value: &serde_json::Value, format: OutputFormat) -> Result<(), CliError> {
    match format {
        OutputFormat::Json | OutputFormat::Audit => {
            println!("{}", serde_json::to_string_pretty(value)?);
            Ok(())
        }
        OutputFormat::Jsonl => {
            println!("{}", serde_json::to_string(value)?);
            Ok(())
        }
        OutputFormat::Csv => {
            println!("{}", serde_json::to_string(value)?);
            Ok(())
        }
        OutputFormat::Table => {
            if let Some(obj) = value.as_object() {
                for (key, value) in obj {
                    println!("{key}: {}", display_json_value(value));
                }
                Ok(())
            } else {
                println!("{}", serde_json::to_string_pretty(value)?);
                Ok(())
            }
        }
    }
}

fn render_json<T: Serialize + ?Sized>(value: &T) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn render_audit(audit: &AuditReport) -> Result<(), CliError> {
    render_json(audit)
}

fn render_audit_with_format(audit: &AuditReport, format: OutputFormat) -> Result<(), CliError> {
    match format {
        OutputFormat::Table => {
            println!("tool: {}", audit.tool);
            println!("tool_version: {}", audit.tool_version);
            println!("command: {}", audit.command);
            println!("evidence_mode: {}", audit.evidence_mode);
            println!("schema_hash: {}", audit.schema_hash);
            println!("schema_file_sha256: {}", audit.schema_file_sha256);
            println!("input_path: {}", audit.input_path);
            println!("input_size_bytes: {}", audit.input_size_bytes);
            println!("verification_scope: {}", audit.verification_scope);
            println!(
                "record_full_coverage: {}",
                audit
                    .record_coverage
                    .full_coverage
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string())
            );
            println!("record_gap_count: {}", audit.record_coverage.gaps.len());
            println!(
                "record_overlap_count: {}",
                audit.record_coverage.overlap_count
            );
            println!("records_seen: {}", audit.records_seen);
            println!("records_valid: {}", audit.records_valid);
            println!("records_invalid: {}", audit.records_invalid);
            println!("fields_seen: {}", audit.fields_seen);
            println!("fields_valid: {}", audit.fields_valid);
            println!("fields_invalid: {}", audit.fields_invalid);
            println!("status: {}", audit.status);
            if let Some(verified) = audit.field_byte_for_byte_verified {
                println!("field_byte_for_byte_verified: {verified}");
            }
            if let Some(verified) = audit.record_byte_for_byte_verified {
                println!("record_byte_for_byte_verified: {verified}");
            }
            if let Some(verified) = audit.byte_for_byte_verified {
                println!("byte_for_byte_verified: {verified}");
            }
            println!("negative_zero_count: {}", audit.negative_zero_count);
            println!(
                "non_preferred_sign_count: {}",
                audit.non_preferred_sign_count
            );
            println!("distinct_error_codes: {}", audit.error_distribution.len());
            println!("distinct_fields_seen: {}", audit.field_profiles.len());
            Ok(())
        }
        OutputFormat::Json | OutputFormat::Audit => render_audit(audit),
        OutputFormat::Jsonl => {
            println!("{}", serde_json::to_string(audit)?);
            Ok(())
        }
        OutputFormat::Csv => render_audit_csv(audit),
    }
}

fn render_audit_csv(audit: &AuditReport) -> Result<(), CliError> {
    let row = AuditSummaryCsv {
        version: audit.version,
        tool: &audit.tool,
        tool_version: &audit.tool_version,
        command: &audit.command,
        status: audit.status,
        evidence_mode: audit.evidence_mode,
        schema_hash: &audit.schema_hash,
        schema_file_sha256: &audit.schema_file_sha256,
        schema_field_count: audit.schema_field_count,
        schema_filler_count: audit.schema_filler_count,
        schema_record_length: audit.schema_record_length,
        schema_input_encoding: audit.schema_input_encoding,
        verification_scope: audit.verification_scope,
        record_full_coverage: audit.record_coverage.full_coverage,
        record_overlap_count: audit.record_coverage.overlap_count,
        record_gap_count: audit.record_coverage.gaps.len(),
        input_path: &audit.input_path,
        input_size_bytes: audit.input_size_bytes,
        input_sha256: audit.input_sha256.as_deref(),
        record_limit: audit.record_limit,
        records_seen: audit.records_seen,
        records_valid: audit.records_valid,
        records_invalid: audit.records_invalid,
        fields_seen: audit.fields_seen,
        fields_valid: audit.fields_valid,
        fields_invalid: audit.fields_invalid,
        field_byte_for_byte_verified: audit.field_byte_for_byte_verified,
        record_byte_for_byte_verified: audit.record_byte_for_byte_verified,
        byte_for_byte_verified: audit.byte_for_byte_verified,
        negative_zero_count: audit.negative_zero_count,
        non_preferred_sign_count: audit.non_preferred_sign_count,
        distinct_error_codes: audit.error_distribution.len(),
        distinct_fields_seen: audit.field_profiles.len(),
        failure_sample_count: audit.failure_samples.len(),
    };
    let mut writer = csv::Writer::from_writer(io::stdout());
    writer
        .serialize(row)
        .map_err(|err| CliError::data("E_CSV", err.to_string()))?;
    writer.flush()?;
    Ok(())
}

fn display_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

fn classify_sign(nibble: u8, signed: bool) -> &'static str {
    match nibble {
        0xA | 0xC | 0xE | 0xF => {
            if signed {
                "positive"
            } else {
                "unsigned-positive"
            }
        }
        0xB | 0xD => "negative",
        _ => "invalid",
    }
}

fn packed_error_code(err: &PackedError) -> &'static str {
    match err {
        PackedError::InvalidByteLength { .. } => "E_LENGTH",
        PackedError::InvalidTotalDigits(_) => "E_SCHEMA",
        PackedError::ScaleExceedsTotalDigits { .. } | PackedError::ScaleTooLargeForDecimal(_) => {
            "E_SCHEMA"
        }
        PackedError::InvalidDigitNibble { .. } => "E_DIGIT",
        PackedError::InvalidSignNibble { .. } | PackedError::InvalidSignOverride { .. } => "E_SIGN",
        PackedError::NegativeInUnsigned { .. } | PackedError::NegativeUnsigned => "E_SIGN",
        PackedError::InvalidPaddingNibble { .. } => "E_PADDING",
        PackedError::Overflow { .. }
        | PackedError::ArithmeticOverflow { .. }
        | PackedError::AbsoluteOverflow => "E_OVERFLOW",
    }
}

fn map_packed_error(err: PackedError) -> CliError {
    let code = packed_error_code(&err);
    let exit = if code == "E_SCHEMA" {
        ExitCode::Config
    } else {
        ExitCode::Data
    };
    CliError {
        code,
        message: err.to_string(),
        exit,
    }
}
