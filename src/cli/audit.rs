use super::*;

pub(super) struct AuditContext<'a> {
    pub(super) command: &'a str,
    pub(super) hashes: &'a SchemaHashes,
    pub(super) input: &'a Path,
    pub(super) limits: ProcessingLimits,
    pub(super) failure_sample_limit: usize,
    pub(super) include_input_hash: bool,
    pub(super) verification_scope: VerificationScope,
    pub(super) evidence_mode: EvidenceMode,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DecodedField {
    pub(super) version: u8,
    pub(super) record_index: Option<usize>,
    pub(super) field: String,
    pub(super) offset: Option<usize>,
    pub(super) raw_hex: String,
    pub(super) raw_byte_len: usize,
    pub(super) raw_hex_truncated: bool,
    pub(super) value: Option<String>,
    pub(super) sign_nibble: Option<String>,
    pub(super) sign_class: Option<String>,
    pub(super) valid: bool,
    pub(super) error_code: Option<&'static str>,
    pub(super) error_docs_url: Option<&'static str>,
    pub(super) message: Option<String>,
    pub(super) recoverable: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum AuditStatus {
    Passed,
    Failed,
    Empty,
}

pub(super) fn new_audit(schema: &Schema, ctx: AuditContext<'_>) -> Result<AuditReport, CliError> {
    let input_size_bytes = if is_stdin_path(ctx.input) {
        0
    } else {
        fs::metadata(ctx.input)?.len()
    };
    let record_coverage = schema_coverage_summary(schema)?;
    Ok(AuditReport {
        version: OUTPUT_VERSION,
        tool: TOOL_NAME.to_string(),
        tool_version: TOOL_VERSION.to_string(),
        command: ctx.command.to_string(),
        evidence_mode: ctx.evidence_mode,
        schema_hash: ctx.hashes.semantic_sha256.clone(),
        schema_file_sha256: ctx.hashes.file_sha256.clone(),
        schema_field_count: schema.fields.len(),
        schema_filler_count: schema.fillers.len(),
        schema_record_length: schema.record_length,
        schema_input_encoding: schema.input_encoding,
        verification_scope: ctx.verification_scope,
        record_coverage,
        input_path: ctx.input.display().to_string(),
        input_size_bytes,
        input_sha256: if ctx.include_input_hash && !is_stdin_path(ctx.input) {
            Some(sha256_file(ctx.input)?)
        } else {
            None
        },
        runtime: runtime_evidence(ctx.evidence_mode)?,
        elapsed_ms: 0,
        bytes_per_sec: 0.0,
        records_per_sec: 0.0,
        fields_per_sec: 0.0,
        started_at: Instant::now(),
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

pub(super) fn finalize_audit(audit: &mut AuditReport) {
    audit.status = if audit.records_seen == 0 || audit.fields_seen == 0 {
        AuditStatus::Empty
    } else if audit.records_invalid == 0 && audit.fields_invalid == 0 {
        AuditStatus::Passed
    } else {
        AuditStatus::Failed
    };
}

pub(super) fn finalize_audit_metrics(audit: &mut AuditReport) {
    let elapsed = audit.started_at.elapsed();
    audit.elapsed_ms = elapsed.as_millis();
    let seconds = elapsed.as_secs_f64();
    if seconds > 0.0 {
        audit.bytes_per_sec = audit.input_size_bytes as f64 / seconds;
        audit.records_per_sec = audit.records_seen as f64 / seconds;
        audit.fields_per_sec = audit.fields_seen as f64 / seconds;
    }
}

pub(super) fn report_progress(enabled: bool, audit: &AuditReport) {
    if enabled {
        eprintln!(
            "processed {} records, {} fields in {} ms",
            audit.records_seen, audit.fields_seen, audit.elapsed_ms
        );
    }
}

pub(super) fn fail_if_empty(enabled: bool, audit: &AuditReport) -> Result<(), CliError> {
    if enabled && matches!(audit.status, AuditStatus::Empty) {
        Err(CliError::data(
            "E_EMPTY",
            "no records were processed; remove --fail-on-empty to allow empty inputs",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn finalize_verify_audit(audit: &mut AuditReport) {
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

fn runtime_evidence(mode: EvidenceMode) -> Result<Option<RuntimeEvidence>, CliError> {
    if mode != EvidenceMode::Full {
        return Ok(None);
    }
    let cwd = env::current_dir()?.display().to_string();
    let generated_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| CliError::internal(format!("system clock is before UNIX epoch: {err}")))?
        .as_secs();
    Ok(Some(RuntimeEvidence {
        argv: env::args().collect(),
        cwd,
        os: env::consts::OS,
        arch: env::consts::ARCH,
        family: env::consts::FAMILY,
        exe_suffix: env::consts::EXE_SUFFIX,
        generated_unix_seconds,
    }))
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

#[derive(Debug, Serialize)]
pub(super) struct AuditReport {
    pub(super) version: u8,
    pub(super) tool: String,
    pub(super) tool_version: String,
    pub(super) command: String,
    pub(super) evidence_mode: EvidenceMode,
    pub(super) schema_hash: String,
    pub(super) schema_file_sha256: String,
    pub(super) schema_field_count: usize,
    pub(super) schema_filler_count: usize,
    pub(super) schema_record_length: Option<usize>,
    pub(super) schema_input_encoding: InputEncoding,
    pub(super) verification_scope: VerificationScope,
    pub(super) record_coverage: SchemaCoverageSummary,
    pub(super) input_path: String,
    pub(super) input_size_bytes: u64,
    pub(super) input_sha256: Option<String>,
    pub(super) runtime: Option<RuntimeEvidence>,
    pub(super) elapsed_ms: u128,
    pub(super) bytes_per_sec: f64,
    pub(super) records_per_sec: f64,
    pub(super) fields_per_sec: f64,
    #[serde(skip)]
    pub(super) started_at: Instant,
    pub(super) record_limit: Option<usize>,
    pub(super) failure_sample_limit: usize,
    pub(super) records_seen: usize,
    pub(super) records_valid: usize,
    pub(super) records_invalid: usize,
    pub(super) fields_seen: usize,
    pub(super) fields_valid: usize,
    pub(super) fields_invalid: usize,
    pub(super) status: AuditStatus,
    pub(super) field_byte_for_byte_verified: Option<bool>,
    pub(super) record_byte_for_byte_verified: Option<bool>,
    pub(super) byte_for_byte_verified: Option<bool>,
    pub(super) negative_zero_count: usize,
    pub(super) non_preferred_sign_count: usize,
    pub(super) sign_distribution: BTreeMap<String, usize>,
    pub(super) error_distribution: BTreeMap<String, usize>,
    pub(super) field_profiles: BTreeMap<String, FieldAuditSummary>,
    pub(super) failure_samples: Vec<DecodedField>,
}

pub(super) fn record_audit(audit: &mut AuditReport, row: &DecodedField) {
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
            error_docs_url: self.error_docs_url,
            message: self.message.clone(),
            recoverable: self.recoverable,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeEvidence {
    pub(super) argv: Vec<String>,
    pub(super) cwd: String,
    pub(super) os: &'static str,
    pub(super) arch: &'static str,
    pub(super) family: &'static str,
    pub(super) exe_suffix: &'static str,
    pub(super) generated_unix_seconds: u64,
}

#[derive(Debug, Serialize, Default)]
pub(super) struct FieldAuditSummary {
    pub(super) fields_seen: usize,
    pub(super) fields_valid: usize,
    pub(super) fields_invalid: usize,
    pub(super) min_value: Option<String>,
    pub(super) max_value: Option<String>,
    pub(super) negative_zero_count: usize,
    pub(super) non_preferred_sign_count: usize,
    pub(super) sign_distribution: BTreeMap<String, usize>,
    pub(super) error_distribution: BTreeMap<String, usize>,
}
