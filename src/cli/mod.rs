mod args;
mod audit;
mod batch;
mod constants;
mod copybook;
mod ebcdic;
mod ebcdic_tables;
mod encoding_catalog;
mod error;
mod field_decode;
mod mixed_dbcs;
mod mixed_dbcs_tables;
mod render;
mod schema;
mod schema_compare;
mod schema_emit;

use args::*;
use audit::*;
use batch::*;
use clap::{CommandFactory, Parser, ValueEnum};
use cobol_packed::{
    from_packed, from_packed_lossless, nibble_iter, to_packed, to_packed_lossless,
    to_packed_with_sign, LosslessDecimal, PackedConfig, PackedError, SignMode,
};
use constants::*;
use error::CliError;
pub use error::ExitCode;
use field_decode::{decode_named_field, decode_plan_field};
use render::*;
use rust_decimal::Decimal;
use schema::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum OutputFormat {
    Table,
    Json,
    Jsonl,
    Csv,
    Audit,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum EncodingListFormat {
    Table,
    Json,
    Jsonl,
    Csv,
}

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
enum FieldType {
    PackedDecimal,
    DisplayText,
    MixedDbcsText,
    ZonedDecimal,
    Binary,
    RawBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TextEncoding {
    Cp037,
    Cp273,
    Cp277,
    Cp278,
    Cp280,
    Cp284,
    Cp285,
    Cp290,
    Cp297,
    Cp420,
    Cp423,
    Cp424,
    Cp500,
    Cp833,
    Cp838,
    Cp870,
    Cp871,
    Cp875,
    Cp880,
    Cp905,
    Cp924,
    Cp930,
    Cp933,
    Cp935,
    Cp937,
    Cp939,
    Cp1025,
    Cp1026,
    Cp1047,
    Cp1140,
    Cp1141,
    Cp1142,
    Cp1143,
    Cp1144,
    Cp1145,
    Cp1146,
    Cp1147,
    Cp1148,
    Cp1149,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InputEncoding {
    Binary,
    Hex,
    Csv,
    Jsonl,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
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

impl fmt::Display for EncodingListFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EncodingListFormat::Table => "table",
            EncodingListFormat::Json => "json",
            EncodingListFormat::Jsonl => "jsonl",
            EncodingListFormat::Csv => "csv",
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

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FieldType::PackedDecimal => "packed-decimal",
            FieldType::DisplayText => "display-text",
            FieldType::MixedDbcsText => "mixed-dbcs-text",
            FieldType::ZonedDecimal => "zoned-decimal",
            FieldType::Binary => "binary",
            FieldType::RawBytes => "raw-bytes",
        })
    }
}

impl fmt::Display for TextEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TextEncoding::Cp037 => "cp037",
            TextEncoding::Cp273 => "cp273",
            TextEncoding::Cp277 => "cp277",
            TextEncoding::Cp278 => "cp278",
            TextEncoding::Cp280 => "cp280",
            TextEncoding::Cp284 => "cp284",
            TextEncoding::Cp285 => "cp285",
            TextEncoding::Cp290 => "cp290",
            TextEncoding::Cp297 => "cp297",
            TextEncoding::Cp420 => "cp420",
            TextEncoding::Cp423 => "cp423",
            TextEncoding::Cp424 => "cp424",
            TextEncoding::Cp500 => "cp500",
            TextEncoding::Cp833 => "cp833",
            TextEncoding::Cp838 => "cp838",
            TextEncoding::Cp870 => "cp870",
            TextEncoding::Cp871 => "cp871",
            TextEncoding::Cp875 => "cp875",
            TextEncoding::Cp880 => "cp880",
            TextEncoding::Cp905 => "cp905",
            TextEncoding::Cp924 => "cp924",
            TextEncoding::Cp930 => "cp930",
            TextEncoding::Cp933 => "cp933",
            TextEncoding::Cp935 => "cp935",
            TextEncoding::Cp937 => "cp937",
            TextEncoding::Cp939 => "cp939",
            TextEncoding::Cp1025 => "cp1025",
            TextEncoding::Cp1026 => "cp1026",
            TextEncoding::Cp1047 => "cp1047",
            TextEncoding::Cp1140 => "cp1140",
            TextEncoding::Cp1141 => "cp1141",
            TextEncoding::Cp1142 => "cp1142",
            TextEncoding::Cp1143 => "cp1143",
            TextEncoding::Cp1144 => "cp1144",
            TextEncoding::Cp1145 => "cp1145",
            TextEncoding::Cp1146 => "cp1146",
            TextEncoding::Cp1147 => "cp1147",
            TextEncoding::Cp1148 => "cp1148",
            TextEncoding::Cp1149 => "cp1149",
        })
    }
}

impl TextEncoding {
    fn from_label(raw: &str) -> Option<Self> {
        let mut label = raw.to_ascii_lowercase();
        label.retain(|ch| !matches!(ch, '-' | '_' | ' '));
        let stripped = label
            .strip_prefix("ccsid")
            .or_else(|| label.strip_prefix("ibm"))
            .or_else(|| label.strip_prefix("cp"))
            .unwrap_or(&label);
        let normalized = match stripped {
            "xedbcdickoreanextended" | "xebcdickoreanextended" => "833",
            "ibmthai" => "838",
            "20273" => "273",
            "20277" => "277",
            "20278" => "278",
            "20280" => "280",
            "20284" => "284",
            "20285" => "285",
            "20290" => "290",
            "20297" => "297",
            "20420" => "420",
            "20423" => "423",
            "20424" => "424",
            "20833" => "833",
            "20838" => "838",
            "20871" => "871",
            "20880" => "880",
            "20905" => "905",
            "20924" | "00924" => "924",
            "50930" => "930",
            "50933" => "933",
            "50935" => "935",
            "50937" => "937",
            "50939" => "939",
            "21025" => "1025",
            "01047" => "1047",
            "01140" => "1140",
            "01141" => "1141",
            "01142" => "1142",
            "01143" => "1143",
            "01144" => "1144",
            "01145" => "1145",
            "01146" => "1146",
            "01147" => "1147",
            "01148" => "1148",
            "01149" => "1149",
            other => other.trim_start_matches('0'),
        };
        match normalized {
            "37" => Some(TextEncoding::Cp037),
            "273" => Some(TextEncoding::Cp273),
            "277" => Some(TextEncoding::Cp277),
            "278" => Some(TextEncoding::Cp278),
            "280" => Some(TextEncoding::Cp280),
            "284" => Some(TextEncoding::Cp284),
            "285" => Some(TextEncoding::Cp285),
            "290" => Some(TextEncoding::Cp290),
            "297" => Some(TextEncoding::Cp297),
            "420" => Some(TextEncoding::Cp420),
            "423" => Some(TextEncoding::Cp423),
            "424" => Some(TextEncoding::Cp424),
            "500" => Some(TextEncoding::Cp500),
            "833" => Some(TextEncoding::Cp833),
            "838" => Some(TextEncoding::Cp838),
            "870" => Some(TextEncoding::Cp870),
            "871" => Some(TextEncoding::Cp871),
            "875" => Some(TextEncoding::Cp875),
            "880" => Some(TextEncoding::Cp880),
            "905" => Some(TextEncoding::Cp905),
            "924" => Some(TextEncoding::Cp924),
            "930" => Some(TextEncoding::Cp930),
            "933" => Some(TextEncoding::Cp933),
            "935" => Some(TextEncoding::Cp935),
            "937" => Some(TextEncoding::Cp937),
            "939" => Some(TextEncoding::Cp939),
            "1025" => Some(TextEncoding::Cp1025),
            "1026" => Some(TextEncoding::Cp1026),
            "1047" => Some(TextEncoding::Cp1047),
            "1140" => Some(TextEncoding::Cp1140),
            "1141" => Some(TextEncoding::Cp1141),
            "1142" => Some(TextEncoding::Cp1142),
            "1143" => Some(TextEncoding::Cp1143),
            "1144" => Some(TextEncoding::Cp1144),
            "1145" => Some(TextEncoding::Cp1145),
            "1146" => Some(TextEncoding::Cp1146),
            "1147" => Some(TextEncoding::Cp1147),
            "1148" => Some(TextEncoding::Cp1148),
            "1149" => Some(TextEncoding::Cp1149),
            _ => None,
        }
    }

    fn is_mixed_dbcs(self) -> bool {
        matches!(
            self,
            TextEncoding::Cp930
                | TextEncoding::Cp933
                | TextEncoding::Cp935
                | TextEncoding::Cp937
                | TextEncoding::Cp939
        )
    }
}

impl<'de> Deserialize<'de> for TextEncoding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        TextEncoding::from_label(&raw)
            .ok_or_else(|| serde::de::Error::custom(format!("unsupported EBCDIC encoding {raw}")))
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
            SchemaCommand::FromCopybook(args) => copybook::from_copybook(args),
            SchemaCommand::EmitRust(args) => schema_emit::emit_rust(args),
            SchemaCommand::Compare(args) => schema_compare::compare(args),
        },
        Command::Encodings { command } => match command {
            EncodingsCommand::List(args) => list_encodings(args),
        },
        Command::Profile(args) => profile(args),
        Command::Completions(args) => generate_completions(args),
        Command::Man => generate_man_page(),
    }
}

fn list_encodings(args: EncodingListArgs) -> Result<(), CliError> {
    let entries = encoding_catalog::all();
    match args.output {
        EncodingListFormat::Table => {
            println!("encoding\tfield_type\tbyte_model\taliases\tnotes");
            for entry in entries {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    entry.encoding,
                    entry.field_type,
                    entry.byte_model,
                    entry.aliases.join(","),
                    entry.notes
                );
            }
            Ok(())
        }
        EncodingListFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&entries)?);
            Ok(())
        }
        EncodingListFormat::Jsonl => {
            for entry in entries {
                println!("{}", serde_json::to_string(&entry)?);
            }
            Ok(())
        }
        EncodingListFormat::Csv => {
            #[derive(Serialize)]
            struct EncodingCsvRow<'a> {
                encoding: &'a str,
                field_type: &'a str,
                byte_model: &'a str,
                aliases: String,
                notes: &'a str,
            }

            let mut writer = csv::Writer::from_writer(io::stdout());
            for entry in entries {
                writer.serialize(EncodingCsvRow {
                    encoding: &entry.encoding,
                    field_type: entry.field_type,
                    byte_model: entry.byte_model,
                    aliases: entry.aliases.join(","),
                    notes: entry.notes,
                })?;
            }
            writer.flush()?;
            Ok(())
        }
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

impl DecodedField {
    fn valid(
        record_index: Option<usize>,
        field: &str,
        offset: Option<usize>,
        bytes: &[u8],
        value: String,
        sign_nibble: Option<String>,
        sign_class: Option<String>,
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
            value: Some(value),
            sign_nibble,
            sign_class,
            valid: true,
            error_code: None,
            error_docs_url: None,
            message: None,
            recoverable: false,
        }
    }

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
            error_docs_url: Some(ERROR_DOCS_BASE_URL),
            message: Some(message.into()),
            recoverable: true,
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
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02X}");
    }
    out
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
