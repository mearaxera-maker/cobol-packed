use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Schema {
    pub(super) version: u8,
    pub(super) record_length: Option<usize>,
    pub(super) input_encoding: InputEncoding,
    #[serde(default = "default_on_error")]
    pub(super) on_error: OnError,
    #[serde(default)]
    pub(super) output: Option<OutputFormat>,
    #[serde(default = "default_verification_scope")]
    pub(super) verification_scope: VerificationScope,
    #[serde(default)]
    pub(super) fillers: Vec<FillerSpec>,
    pub(super) fields: Vec<FieldSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FieldSpec {
    pub(super) name: String,
    #[serde(default = "default_field_type")]
    pub(super) field_type: FieldType,
    pub(super) offset: Option<usize>,
    pub(super) length: Option<usize>,
    #[serde(default)]
    pub(super) total_digits: Option<u8>,
    #[serde(default)]
    pub(super) scale: Option<u8>,
    #[serde(default)]
    pub(super) signed: Option<bool>,
    #[serde(default = "default_sign_mode")]
    pub(super) sign_mode: CliSignMode,
    #[serde(default = "default_field_mode")]
    pub(super) mode: FieldMode,
    #[serde(default)]
    pub(super) encoding: Option<TextEncoding>,
    #[serde(default = "default_required")]
    pub(super) required: bool,
    #[serde(default)]
    pub(super) description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FillerSpec {
    pub(super) name: String,
    pub(super) offset: usize,
    pub(super) length: usize,
    #[serde(default)]
    pub(super) description: Option<String>,
}

pub(super) struct FieldPlan<'a> {
    pub(super) spec: &'a FieldSpec,
    pub(super) kind: FieldPlanKind,
    pub(super) expected_len: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum FieldPlanKind {
    PackedDecimal(PackedConfig),
    DisplayText(TextEncoding),
    MixedDbcsText(TextEncoding),
    ZonedDecimal(PackedConfig),
    Binary { signed: bool, scale: u8 },
    RawBytes,
}

pub(super) struct SchemaHashes {
    pub(super) file_sha256: String,
    pub(super) semantic_sha256: String,
}

#[derive(Serialize)]
pub(super) struct SemanticSchema<'a> {
    pub(super) version: u8,
    pub(super) record_length: Option<usize>,
    pub(super) input_encoding: InputEncoding,
    pub(super) on_error: OnError,
    pub(super) verification_scope: VerificationScope,
    pub(super) fillers: Vec<SemanticFiller<'a>>,
    pub(super) fields: Vec<SemanticField<'a>>,
}

#[derive(Serialize)]
pub(super) struct SemanticField<'a> {
    pub(super) name: &'a str,
    pub(super) field_type: FieldType,
    pub(super) offset: Option<usize>,
    pub(super) length: Option<usize>,
    pub(super) total_digits: Option<u8>,
    pub(super) scale: Option<u8>,
    pub(super) signed: Option<bool>,
    pub(super) sign_mode: CliSignMode,
    pub(super) mode: FieldMode,
    pub(super) encoding: Option<TextEncoding>,
    pub(super) required: bool,
}

#[derive(Serialize)]
pub(super) struct SemanticFiller<'a> {
    pub(super) name: &'a str,
    pub(super) offset: usize,
    pub(super) length: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct FieldPlanSummary<'a> {
    pub(super) name: &'a str,
    pub(super) field_type: FieldType,
    pub(super) offset: Option<usize>,
    pub(super) end_offset: Option<usize>,
    pub(super) length: usize,
    pub(super) total_digits: Option<u8>,
    pub(super) scale: Option<u8>,
    pub(super) signed: Option<bool>,
    pub(super) sign_mode: CliSignMode,
    pub(super) mode: FieldMode,
    pub(super) encoding: Option<TextEncoding>,
    pub(super) required: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum LayoutKind {
    Field,
    Filler,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct LayoutRangeSummary {
    pub(super) name: String,
    pub(super) kind: LayoutKind,
    pub(super) offset: usize,
    pub(super) end_offset: usize,
    pub(super) length: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct LayoutGapSummary {
    pub(super) offset: usize,
    pub(super) end_offset: usize,
    pub(super) length: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct LayoutOverlapSummary {
    pub(super) offset: usize,
    pub(super) end_offset: usize,
    pub(super) length: usize,
    pub(super) previous: String,
    pub(super) current: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct SchemaCoverageSummary {
    pub(super) record_length: Option<usize>,
    pub(super) covered_bytes: usize,
    pub(super) gap_bytes: Option<usize>,
    pub(super) full_coverage: Option<bool>,
    pub(super) overlap_count: usize,
    pub(super) first_offset: Option<usize>,
    pub(super) last_end_offset: Option<usize>,
    pub(super) ranges: Vec<LayoutRangeSummary>,
    pub(super) gaps: Vec<LayoutGapSummary>,
    pub(super) overlaps: Vec<LayoutOverlapSummary>,
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
fn default_field_type() -> FieldType {
    FieldType::PackedDecimal
}
fn default_sign_mode() -> CliSignMode {
    CliSignMode::Pfd
}
fn default_field_mode() -> FieldMode {
    FieldMode::Lossless
}

pub(super) fn load_schema(path: &Path) -> Result<(Schema, SchemaHashes), CliError> {
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
        Some("json") => serde_json::from_str(text)
            .map_err(|err| CliError::config("E_SCHEMA", format!("invalid JSON schema: {err}")))?,
        Some("toml") => toml::from_str(text)
            .map_err(|err| CliError::config("E_SCHEMA", format!("invalid TOML schema: {err}")))?,
        _ => {
            return Err(CliError::config(
                "E_SCHEMA",
                "schema extension must be .json or .toml",
            ))
        }
    };
    validate_schema(&schema)?;
    let canonical = serde_json::to_vec(&semantic_schema(&schema)).map_err(|err| {
        CliError::internal(format!("failed to canonicalize validated schema: {err}"))
    })?;
    Ok((
        schema,
        SchemaHashes {
            file_sha256,
            semantic_sha256: sha256_hex(&canonical),
        },
    ))
}

pub(super) fn semantic_schema(schema: &Schema) -> SemanticSchema<'_> {
    SemanticSchema {
        version: schema.version,
        record_length: schema.record_length,
        input_encoding: schema.input_encoding,
        on_error: schema.on_error,
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
                field_type: field.field_type,
                offset: field.offset,
                length: field.length,
                total_digits: field.total_digits,
                scale: field.scale,
                signed: field.signed,
                sign_mode: field.sign_mode,
                mode: field.mode,
                encoding: field.encoding,
                required: field.required,
            })
            .collect(),
    }
}

pub(super) fn validate_schema(schema: &Schema) -> Result<(), CliError> {
    if !matches!(schema.version, 1 | 2) {
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
    if schema.fields.len() > MAX_SCHEMA_FIELDS {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("schema supports at most {MAX_SCHEMA_FIELDS} fields"),
        ));
    }
    if schema.fields.len().saturating_add(schema.fillers.len()) > MAX_SCHEMA_FIELDS {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("schema supports at most {MAX_SCHEMA_FIELDS} fields plus fillers"),
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
        if record_length == 0 {
            return Err(CliError::config(
                "E_SCHEMA",
                "record_length must be greater than zero",
            ));
        }
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
            if matches!(plan.kind, FieldPlanKind::PackedDecimal(_)) && length != expected {
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

pub(super) fn validate_field_name(name: &str) -> Result<(), CliError> {
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

pub(super) fn plan_field(spec: &FieldSpec) -> Result<FieldPlan<'_>, CliError> {
    let (kind, expected_len) = match spec.field_type {
        FieldType::PackedDecimal => {
            if spec.encoding.is_some() {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "field {} encoding is only valid for display-text",
                        spec.name
                    ),
                ));
            }
            let total_digits = spec.total_digits.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("field {} requires total_digits", spec.name),
                )
            })?;
            let signed = spec.signed.ok_or_else(|| {
                CliError::config("E_SCHEMA", format!("field {} requires signed", spec.name))
            })?;
            let cfg = PackedConfig::new(total_digits, spec.scale.unwrap_or(0), signed)
                .map_err(map_packed_error)?;
            (FieldPlanKind::PackedDecimal(cfg), cfg.byte_len())
        }
        FieldType::DisplayText => {
            let encoding = spec.encoding.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("field {} display-text requires encoding", spec.name),
                )
            })?;
            if encoding.is_mixed_dbcs() {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "field {} encoding {encoding} requires field_type mixed-dbcs-text",
                        spec.name
                    ),
                ));
            }
            let length = spec.length.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("field {} display-text requires length", spec.name),
                )
            })?;
            if length == 0 {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("field {} length must be greater than zero", spec.name),
                ));
            }
            (FieldPlanKind::DisplayText(encoding), length)
        }
        FieldType::MixedDbcsText => {
            let encoding = spec.encoding.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("field {} mixed-dbcs-text requires encoding", spec.name),
                )
            })?;
            if !encoding.is_mixed_dbcs() {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "field {} mixed-dbcs-text requires a mixed DBCS encoding",
                        spec.name
                    ),
                ));
            }
            let length = spec.length.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("field {} mixed-dbcs-text requires length", spec.name),
                )
            })?;
            if length == 0 {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("field {} length must be greater than zero", spec.name),
                ));
            }
            (FieldPlanKind::MixedDbcsText(encoding), length)
        }
        FieldType::ZonedDecimal => {
            if spec.encoding.is_some() {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "field {} encoding is only valid for display-text",
                        spec.name
                    ),
                ));
            }
            let total_digits = spec.total_digits.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("field {} requires total_digits", spec.name),
                )
            })?;
            let signed = spec.signed.ok_or_else(|| {
                CliError::config("E_SCHEMA", format!("field {} requires signed", spec.name))
            })?;
            let cfg = PackedConfig::new(total_digits, spec.scale.unwrap_or(0), signed)
                .map_err(map_packed_error)?;
            let expected = total_digits as usize;
            if spec.length.is_some_and(|length| length != expected) {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "field {} length must equal total_digits for zoned-decimal",
                        spec.name
                    ),
                ));
            }
            (FieldPlanKind::ZonedDecimal(cfg), expected)
        }
        FieldType::Binary => {
            if spec.encoding.is_some() {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "field {} encoding is only valid for display-text",
                        spec.name
                    ),
                ));
            }
            let length = spec.length.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("field {} binary requires length", spec.name),
                )
            })?;
            if !matches!(length, 1 | 2 | 4 | 8) {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("field {} binary length must be 1, 2, 4, or 8", spec.name),
                ));
            }
            (
                FieldPlanKind::Binary {
                    signed: spec.signed.unwrap_or(true),
                    scale: spec.scale.unwrap_or(0),
                },
                length,
            )
        }
        FieldType::RawBytes => {
            if spec.encoding.is_some() {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "field {} encoding is only valid for display-text",
                        spec.name
                    ),
                ));
            }
            let length = spec.length.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("field {} raw-bytes requires length", spec.name),
                )
            })?;
            if length == 0 {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("field {} length must be greater than zero", spec.name),
                ));
            }
            (FieldPlanKind::RawBytes, length)
        }
    };
    Ok(FieldPlan {
        spec,
        kind,
        expected_len,
    })
}

pub(super) fn schema_field_summaries(
    schema: &Schema,
) -> Result<Vec<FieldPlanSummary<'_>>, CliError> {
    let mut out = Vec::with_capacity(schema.fields.len());
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        let length = spec.length.unwrap_or(plan.expected_len);
        let end_offset = spec.offset.and_then(|offset| offset.checked_add(length));
        out.push(FieldPlanSummary {
            name: &spec.name,
            field_type: spec.field_type,
            offset: spec.offset,
            end_offset,
            length,
            total_digits: spec.total_digits,
            scale: spec.scale,
            signed: spec.signed,
            sign_mode: spec.sign_mode,
            mode: spec.mode,
            encoding: spec.encoding,
            required: spec.required,
        });
    }
    Ok(out)
}

pub(super) fn schema_coverage_summary(schema: &Schema) -> Result<SchemaCoverageSummary, CliError> {
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
