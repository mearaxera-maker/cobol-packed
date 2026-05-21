use super::*;

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
    elapsed_ms: u128,
    bytes_per_sec: f64,
    records_per_sec: f64,
    fields_per_sec: f64,
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

pub(super) enum RowSink {
    None,
    Jsonl,
    Csv(Box<csv::Writer<io::Stdout>>),
    Table {
        wrote_header: bool,
        quiet: bool,
    },
    Buffer {
        rows: Vec<DecodedField>,
        max_rows: Option<usize>,
    },
}

impl RowSink {
    pub(super) fn for_output(format: OutputFormat, limits: ProcessingLimits) -> Self {
        match format {
            OutputFormat::Json => RowSink::Buffer {
                rows: Vec::new(),
                max_rows: limits.max_buffered_rows,
            },
            OutputFormat::Jsonl => RowSink::Jsonl,
            OutputFormat::Csv => RowSink::Csv(Box::new(csv::Writer::from_writer(io::stdout()))),
            OutputFormat::Table => RowSink::Table {
                wrote_header: false,
                quiet: limits.quiet,
            },
            OutputFormat::Audit => RowSink::None,
        }
    }

    pub(super) fn emit(&mut self, row: DecodedField) -> Result<(), CliError> {
        match self {
            RowSink::None => Ok(()),
            RowSink::Jsonl => {
                println!("{}", serde_json::to_string(&row)?);
                Ok(())
            }
            RowSink::Csv(writer) => writer
                .serialize(row)
                .map_err(|err| CliError::data("E_CSV", err.to_string())),
            RowSink::Table {
                wrote_header,
                quiet,
            } => {
                if !*wrote_header && !*quiet {
                    println!("record\tfield\toffset\tvalid\tvalue\tsign\traw_hex\terror");
                    *wrote_header = true;
                }
                print_table_row(&row);
                Ok(())
            }
            RowSink::Buffer { rows, max_rows } => {
                if max_rows.is_some_and(|limit| rows.len() >= limit) {
                    return Err(CliError::data(
                        "E_OUTPUT_LIMIT",
                        "json output row cap reached; use --max-rows 0 for unbounded JSON or a streaming format",
                    ));
                }
                rows.push(row);
                Ok(())
            }
        }
    }

    pub(super) fn finish(mut self, format: OutputFormat) -> Result<(), CliError> {
        match &mut self {
            RowSink::Csv(writer) => {
                writer.flush()?;
                Ok(())
            }
            RowSink::Buffer { rows, .. } => render_json(rows),
            RowSink::Table {
                wrote_header,
                quiet,
            } if !*wrote_header && !*quiet && format == OutputFormat::Table => {
                println!("record\tfield\toffset\tvalid\tvalue\tsign\traw_hex\terror");
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

pub(super) fn render_fields(rows: &[DecodedField], format: OutputFormat) -> Result<(), CliError> {
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
        table_cell(&row.field),
        row.offset
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string()),
        row.valid,
        table_cell(row.value.as_deref().unwrap_or("-")),
        table_cell(row.sign_nibble.as_deref().unwrap_or("-")),
        table_cell(&row.raw_hex),
        row.error_code.unwrap_or("-"),
    );
}

fn table_cell(value: &str) -> String {
    if value
        .chars()
        .any(|ch| matches!(ch, '\t' | '\n' | '\r' | '"'))
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub(super) fn render_inspect(
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
        "error_docs_url": field.error_docs_url,
        "message": field.message,
    });
    render_value(&value, format)
}

pub(super) fn render_value(
    value: &serde_json::Value,
    format: OutputFormat,
) -> Result<(), CliError> {
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

pub(super) fn render_json<T: Serialize + ?Sized>(value: &T) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub(super) fn render_audit(audit: &AuditReport) -> Result<(), CliError> {
    render_json(audit)
}

pub(super) fn render_audit_with_format(
    audit: &AuditReport,
    format: OutputFormat,
) -> Result<(), CliError> {
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
            println!("elapsed_ms: {}", audit.elapsed_ms);
            println!("bytes_per_sec: {}", audit.bytes_per_sec);
            println!("records_per_sec: {}", audit.records_per_sec);
            println!("fields_per_sec: {}", audit.fields_per_sec);
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

pub(super) fn render_audit_csv(audit: &AuditReport) -> Result<(), CliError> {
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
        elapsed_ms: audit.elapsed_ms,
        bytes_per_sec: audit.bytes_per_sec,
        records_per_sec: audit.records_per_sec,
        fields_per_sec: audit.fields_per_sec,
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
