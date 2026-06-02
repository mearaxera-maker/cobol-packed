use super::{
    classify_sign, handle_decoded_row, handle_record_error, map_packed_error, parse_hex_with_limit,
    raw_hex_for_output, to_hex, validate_field_name, CliError, CliSignMode, DecodedField,
    FieldMode, InputEncoding, LayoutGapSummary, LayoutKind, LayoutOverlapSummary,
    LayoutRangeSummary, OnError, OutputFormat, PackedConfig, PackedError, ProcessingLimits,
    RowSink, Schema, SchemaCoverageSummary, VerificationScope, MAX_RECORD_BYTES, OUTPUT_VERSION,
};
use cobol_packed::{from_packed, from_packed_lossless, to_packed_lossless};
use rust_decimal::Decimal;
use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum LayoutMode {
    #[default]
    Declared,
    Sequential,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum PlatformProfile {
    #[default]
    IbmZOs,
    MicroFocus,
    GnuCobol,
    IbmI,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum CobolType {
    PackedDecimal,
    ZonedDecimal,
    Binary,
    NativeBinary,
    IbmFloat32,
    IbmFloat64,
    Alphanumeric,
    Filler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum FieldEncoding {
    Ebcdic,
    Ascii,
    AsciiOverpunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Endian {
    Big,
    Little,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodePage(pub(super) u16);

impl Serialize for CodePage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("cp{:03}", self.0))
    }
}

impl<'de> Deserialize<'de> for CodePage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CodePageVisitor;

        impl<'de> Visitor<'de> for CodePageVisitor {
            type Value = CodePage;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an EBCDIC CCSID such as 37, 500, cp037, or cp1140")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = u16::try_from(value)
                    .map_err(|_| E::custom("codepage must fit in an unsigned 16-bit CCSID"))?;
                validate_codepage(value)
                    .map(|()| CodePage(value))
                    .map_err(E::custom)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let trimmed = value.trim();
                let digits = trimmed
                    .strip_prefix("cp")
                    .or_else(|| trimmed.strip_prefix("CP"))
                    .unwrap_or(trimmed);
                let parsed = digits
                    .parse::<u16>()
                    .map_err(|_| E::custom("codepage must be numeric or cpNNN"))?;
                validate_codepage(parsed)
                    .map(|()| CodePage(parsed))
                    .map_err(E::custom)
            }
        }

        deserializer.deserialize_any(CodePageVisitor)
    }
}

fn validate_codepage(value: u16) -> Result<(), String> {
    match value {
        37 | 500 | 1140 | 1148 => Ok(()),
        _ => Err(format!(
            "unsupported codepage cp{value}; supported codepages are cp037, cp500, cp1140, cp1148"
        )),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SignPolicy {
    #[default]
    Preferred,
    NonPreferred,
    Permissive {
        blank_as_positive: bool,
        zero_nibble_as_positive: bool,
    },
}

impl Serialize for SignPolicy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            SignPolicy::Preferred => serializer.serialize_str("preferred"),
            SignPolicy::NonPreferred => serializer.serialize_str("non-preferred"),
            SignPolicy::Permissive {
                blank_as_positive,
                zero_nibble_as_positive,
            } => {
                let mut state = serializer.serialize_struct("SignPolicy", 3)?;
                state.serialize_field("policy", "permissive")?;
                state.serialize_field("blank_as_positive", blank_as_positive)?;
                state.serialize_field("zero_nibble_as_positive", zero_nibble_as_positive)?;
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for SignPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SignPolicyVisitor;

        impl<'de> Visitor<'de> for SignPolicyVisitor {
            type Value = SignPolicy;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(
                    "preferred, non-preferred, permissive, or a permissive policy object",
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                match value {
                    "preferred" | "pfd" => Ok(SignPolicy::Preferred),
                    "non-preferred" | "nopfd" => Ok(SignPolicy::NonPreferred),
                    "permissive" => Ok(SignPolicy::Permissive {
                        blank_as_positive: false,
                        zero_nibble_as_positive: false,
                    }),
                    _ => Err(E::custom("unknown sign policy")),
                }
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut policy = None::<String>;
                let mut blank_as_positive = false;
                let mut zero_nibble_as_positive = false;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "policy" | "mode" => policy = Some(map.next_value()?),
                        "blank_as_positive" => blank_as_positive = map.next_value()?,
                        "zero_nibble_as_positive" => zero_nibble_as_positive = map.next_value()?,
                        _ => {
                            let _ = map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }
                match policy.as_deref() {
                    Some("preferred") | Some("pfd") => Ok(SignPolicy::Preferred),
                    Some("non-preferred") | Some("nopfd") => Ok(SignPolicy::NonPreferred),
                    Some("permissive") => Ok(SignPolicy::Permissive {
                        blank_as_positive,
                        zero_nibble_as_positive,
                    }),
                    _ => Err(de::Error::custom("sign policy object requires policy")),
                }
            }
        }

        deserializer.deserialize_any(SignPolicyVisitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(super) enum RawLayoutItem {
    Field(RawFieldSpec),
    Filler(RawFillerSpec),
    #[serde(rename = "occurs", alias = "occurs-group")]
    OccursGroup(RawOccursGroup),
    #[serde(rename = "redefines", alias = "redefines-group")]
    RedefinesGroup(RawRedefinesGroup),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawFieldSpec {
    pub(super) name: String,
    #[serde(default)]
    pub(super) offset: Option<usize>,
    #[serde(default, alias = "byte_len")]
    pub(super) length: Option<usize>,
    pub(super) field_type: CobolType,
    #[serde(default)]
    pub(super) total_digits: Option<u8>,
    #[serde(default)]
    pub(super) scale: Option<u8>,
    #[serde(default)]
    pub(super) signed: Option<bool>,
    #[serde(default)]
    pub(super) sign_mode: Option<CliSignMode>,
    #[serde(default)]
    pub(super) sign_policy: Option<SignPolicy>,
    #[serde(default)]
    pub(super) mode: Option<FieldMode>,
    #[serde(default)]
    pub(super) encoding: Option<FieldEncoding>,
    #[serde(default)]
    pub(super) codepage: Option<CodePage>,
    #[serde(default)]
    pub(super) endian: Option<Endian>,
    #[serde(default)]
    pub(super) sync: bool,
    #[serde(default = "super::default_required")]
    pub(super) required: bool,
    #[serde(default)]
    pub(super) description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawFillerSpec {
    pub(super) name: String,
    #[serde(default)]
    pub(super) offset: Option<usize>,
    pub(super) length: usize,
    #[serde(default)]
    pub(super) description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawOccursGroup {
    pub(super) name: String,
    #[serde(default)]
    pub(super) offset: Option<usize>,
    pub(super) counter_field: String,
    pub(super) min_occurs: usize,
    pub(super) max_occurs: usize,
    #[serde(default)]
    pub(super) element_layout: Vec<RawLayoutItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawRedefinesGroup {
    pub(super) name: String,
    #[serde(default)]
    pub(super) offset: Option<usize>,
    #[serde(default)]
    pub(super) base_length: Option<usize>,
    pub(super) variants: Vec<RawRedefinesVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawRedefinesVariant {
    pub(super) name: String,
    #[serde(default)]
    pub(super) selector: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) layout: Vec<RawLayoutItem>,
}

#[derive(Debug, Clone)]
pub(super) struct PlannedSchema {
    pub(super) record_length: Option<usize>,
    pub(super) input_encoding: InputEncoding,
    pub(super) on_error: OnError,
    pub(super) output: Option<OutputFormat>,
    pub(super) verification_scope: VerificationScope,
    pub(super) layout_mode: LayoutMode,
    pub(super) platform_profile: PlatformProfile,
    pub(super) items: Vec<PlannedLayoutItem>,
    pub(super) fields: Vec<PlannedFieldSpec>,
    pub(super) ranges: Vec<LayoutRangeSummary>,
    pub(super) has_occurs: bool,
    pub(super) max_record_len: usize,
    pub(super) odo_header_len: usize,
}

#[derive(Debug, Clone)]
pub(super) enum PlannedLayoutItem {
    Field(PlannedFieldSpec),
    Filler {
        _name: String,
        offset: usize,
        length: usize,
    },
    SyncSlack {
        offset: usize,
        length: usize,
    },
    OccursGroup(PlannedOccursGroup),
    RedefinesGroup(PlannedRedefinesGroup),
}

#[derive(Debug, Clone)]
pub(super) struct PlannedFieldSpec {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) offset: usize,
    pub(super) byte_len: usize,
    pub(super) codec: FieldCodec,
    pub(super) required: bool,
    pub(super) sync: bool,
}

#[derive(Debug, Clone)]
pub(super) struct PlannedOccursGroup {
    pub(super) name: String,
    pub(super) offset: usize,
    pub(super) min_occurs: usize,
    pub(super) max_occurs: usize,
    pub(super) element_byte_len: usize,
    pub(super) counter_field: PlannedFieldSpec,
    pub(super) element_items: Vec<PlannedLayoutItem>,
}

#[derive(Debug, Clone)]
pub(super) struct PlannedRedefinesGroup {
    pub(super) name: String,
    pub(super) offset: usize,
    pub(super) base_length: usize,
    pub(super) variants: Vec<PlannedRedefinesVariant>,
}

#[derive(Debug, Clone)]
pub(super) struct PlannedRedefinesVariant {
    pub(super) _name: String,
    pub(super) selector: Option<PlannedSelector>,
    pub(super) items: Vec<PlannedLayoutItem>,
}

#[derive(Debug, Clone)]
pub(super) struct PlannedSelector {
    pub(super) field: PlannedFieldSpec,
    pub(super) equals: String,
}

#[derive(Debug, Clone)]
pub(super) enum FieldCodec {
    PackedDecimal {
        cfg: PackedConfig,
        sign_mode: CliSignMode,
        mode: FieldMode,
    },
    ZonedDecimal {
        total_digits: u8,
        scale: u8,
        signed: bool,
        encoding: FieldEncoding,
        codepage: Option<CodePage>,
        sign_policy: SignPolicy,
    },
    Binary {
        byte_len: usize,
        endian: Endian,
        signed: bool,
        scale: u8,
    },
    NativeBinary {
        byte_len: usize,
        endian: Endian,
        signed: bool,
        scale: u8,
    },
    IbmFloat32 {
        endian: Endian,
    },
    IbmFloat64 {
        endian: Endian,
    },
    Alphanumeric {
        byte_len: usize,
        encoding: FieldEncoding,
        codepage: Option<CodePage>,
    },
    Bytes {
        byte_len: usize,
    },
}

#[derive(Debug, Clone)]
enum DecodedValue {
    Decimal(Decimal),
    DecimalWithRaw { value: Decimal, raw: Vec<u8> },
    Integer(i64),
    UnsignedInteger(u64),
    FloatWithRaw { value: f64, raw: Vec<u8> },
    Text(String),
    Bytes(Vec<u8>),
    Null,
}

#[derive(Debug, Clone)]
struct CodecOutput {
    value: DecodedValue,
    sign_nibble: Option<String>,
    sign_class: Option<String>,
}

#[derive(Debug, Clone)]
struct DecodeFailure {
    code: &'static str,
    message: String,
}

#[derive(Debug, Clone)]
struct PlanResult {
    items: Vec<PlannedLayoutItem>,
    total_size: usize,
}

#[derive(Debug, Clone)]
struct PlanState {
    fields: Vec<PlannedFieldSpec>,
    ranges: Vec<LayoutRangeSummary>,
    names: BTreeSet<String>,
    has_occurs: bool,
    odo_header_len: usize,
}

pub(super) fn plan_schema(schema: &Schema) -> Result<PlannedSchema, CliError> {
    match schema.version {
        1 => plan_v1_schema(schema),
        2 => plan_v2_schema(schema),
        _ => Err(CliError::config(
            "E_SCHEMA",
            "schema version must be 1 or 2",
        )),
    }
}

pub(super) fn validate_v2_schema(schema: &Schema) -> Result<(), CliError> {
    let plan = plan_schema(schema)?;
    if plan.fields.is_empty() {
        return Err(CliError::config(
            "E_SCHEMA",
            "schema v2 requires at least one decoded field",
        ));
    }
    let coverage = coverage_from_ranges(plan.record_length, &plan.ranges);
    if coverage.overlap_count > 0 {
        let first = coverage
            .overlaps
            .first()
            .map(|overlap| {
                format!(
                    "{} overlaps {} at {}..{}",
                    overlap.current, overlap.previous, overlap.offset, overlap.end_offset
                )
            })
            .unwrap_or_else(|| "layout overlap".to_string());
        return Err(CliError::config("E_SCHEMA", first));
    }
    if plan.verification_scope == VerificationScope::Record && coverage.full_coverage != Some(true)
    {
        return Err(CliError::config(
            "E_SCHEMA",
            "record verification scope requires fields, fillers, sync slack, occurs ranges, and redefines bases to cover every record byte",
        ));
    }
    Ok(())
}

pub(super) fn schema_field_count(schema: &Schema) -> Result<usize, CliError> {
    Ok(plan_schema(schema)?.fields.len())
}

pub(super) fn schema_filler_count(schema: &Schema) -> Result<usize, CliError> {
    if schema.version == 1 {
        return Ok(schema.fillers.len());
    }
    let plan = plan_schema(schema)?;
    Ok(plan
        .ranges
        .iter()
        .filter(|range| matches!(range.kind, LayoutKind::Filler | LayoutKind::SyncSlack))
        .count())
}

pub(super) fn schema_coverage_summary_v2(
    schema: &Schema,
) -> Result<SchemaCoverageSummary, CliError> {
    let plan = plan_schema(schema)?;
    Ok(coverage_from_ranges(plan.record_length, &plan.ranges))
}

pub(super) fn semantic_schema_bytes_v2(schema: &Schema) -> Result<Vec<u8>, CliError> {
    let plan = plan_schema(schema)?;
    let fields: Vec<_> = plan
        .fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "path": field.path,
                "name": field.name,
                "offset": field.offset,
                "byte_len": field.byte_len,
                "sync": field.sync,
                "required": field.required,
                "codec": field.codec.semantic_json(),
            })
        })
        .collect();
    let ranges: Vec<_> = plan
        .ranges
        .iter()
        .map(|range| {
            serde_json::json!({
                "name": range.name,
                "kind": range.kind,
                "offset": range.offset,
                "end_offset": range.end_offset,
                "length": range.length,
            })
        })
        .collect();
    let semantic = serde_json::json!({
        "version": 2,
        "record_length": plan.record_length,
        "input_encoding": plan.input_encoding,
        "on_error": plan.on_error,
        "output": plan.output,
        "verification_scope": plan.verification_scope,
        "layout_mode": plan.layout_mode,
        "platform_profile": plan.platform_profile,
        "fields": fields,
        "ranges": ranges,
    });
    serde_json::to_vec(&semantic).map_err(|err| {
        CliError::internal(format!("failed to canonicalize validated schema v2: {err}"))
    })
}

pub(super) fn schema_check_value_v2(
    schema: &Schema,
    schema_hash: &str,
    schema_file_sha256: &str,
) -> Result<serde_json::Value, CliError> {
    let plan = plan_schema(schema)?;
    let coverage = coverage_from_ranges(plan.record_length, &plan.ranges);
    let fields: Vec<_> = plan
        .fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "name": field.name,
                "path": field.path,
                "offset": field.offset,
                "end_offset": field.offset + field.byte_len,
                "length": field.byte_len,
                "field_type": field.codec.type_name(),
                "required": field.required,
                "sync": field.sync,
            })
        })
        .collect();
    Ok(serde_json::json!({
        "version": OUTPUT_VERSION,
        "schema_version": schema.version,
        "schema_hash": schema_hash,
        "schema_file_sha256": schema_file_sha256,
        "field_count": plan.fields.len(),
        "filler_count": plan.ranges.iter().filter(|range| matches!(range.kind, LayoutKind::Filler | LayoutKind::SyncSlack)).count(),
        "record_length": plan.record_length,
        "input_encoding": plan.input_encoding,
        "on_error": plan.on_error,
        "verification_scope": plan.verification_scope,
        "layout_mode": plan.layout_mode,
        "platform_profile": plan.platform_profile,
        "coverage": coverage,
        "fields": fields,
        "valid": true,
    }))
}

pub(super) fn compare_fields_v2(
    schema: &Schema,
) -> Result<BTreeMap<String, serde_json::Value>, CliError> {
    let plan = plan_schema(schema)?;
    Ok(plan
        .fields
        .iter()
        .map(|field| {
            (
                field.path.clone(),
                serde_json::json!({
                    "path": field.path,
                    "name": field.name,
                    "offset": field.offset,
                    "length": field.byte_len,
                    "required": field.required,
                    "sync": field.sync,
                    "codec": field.codec.semantic_json(),
                }),
            )
        })
        .collect())
}

pub(super) fn process_records_v2(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    audit: &mut super::AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let plan = plan_schema(schema)?;
    if plan.has_occurs && matches!(sink, RowSink::Csv(_)) {
        return Err(CliError::config(
            "E_SCHEMA",
            "CSV output for OCCURS DEPENDING ON records requires a future flatten mode",
        ));
    }
    match plan.input_encoding {
        InputEncoding::Binary => {
            process_binary_records_v2(schema, &plan, input, verify, limits, audit, sink)
        }
        InputEncoding::Hex => {
            process_hex_records_v2(schema, &plan, input, verify, limits, audit, sink)
        }
        InputEncoding::Csv | InputEncoding::Jsonl => Err(CliError::config(
            "E_SCHEMA",
            "schema v2 record layouts currently require binary or hex-line input",
        )),
    }
}

pub(super) fn emit_rust(schema: &Schema, output: &Path) -> Result<(), CliError> {
    let plan = plan_schema(schema)?;
    ensure_emit_rust_supported_items(&plan.items)?;
    let mut text = String::new();
    text.push_str("// Generated by cobol-packed schema emit-rust. Do not edit by hand.\n");
    text.push_str("#![allow(dead_code)]\n\n");
    text.push_str("pub struct Record<'a> {\n    bytes: &'a [u8],\n}\n\n");
    text.push_str("impl<'a> Record<'a> {\n");
    text.push_str("    pub fn new(bytes: &'a [u8]) -> Result<Self, String> {\n");
    if plan.has_occurs {
        text.push_str(&format!(
            "        if bytes.len() < {} {{ return Err(format!(\"record requires at least {} bytes to decode OCCURS counters, got {{}}\", bytes.len())); }}\n",
            plan.odo_header_len, plan.odo_header_len
        ));
        text.push_str("        let record = Self { bytes };\n");
        text.push_str("        let required_len = record.expected_record_len()?;\n");
        text.push_str(
            "        if bytes.len() < required_len { return Err(format!(\"record requires {} bytes after OCCURS evaluation, got {}\", required_len, bytes.len())); }\n",
        );
        text.push_str("        Ok(record)\n    }\n\n");
        emit_generated_expected_record_len(&mut text, &plan)?;
    } else {
        text.push_str(&format!(
            "        if bytes.len() < {} {{ return Err(format!(\"record requires at least {} bytes, got {{}}\", bytes.len())); }}\n",
            plan.max_record_len, plan.max_record_len
        ));
        text.push_str("        Ok(Self { bytes })\n    }\n\n");
    }
    for field in direct_fields_from_items(&plan.items) {
        emit_generated_field_methods(&mut text, field, &rust_ident(&field.path), "self.bytes")?;
    }
    emit_occurs_record_methods(&mut text, &plan.items)?;
    emit_redefines_record_methods(&mut text, &plan.items);
    text.push_str("}\n\n");
    emit_occurs_element_types(&mut text, &plan.items)?;
    emit_redefines_types(&mut text, &plan.items)?;
    emit_generated_helpers(&mut text);
    if text.contains("unsafe") {
        return Err(CliError::internal(
            "generated Rust unexpectedly contains unsafe text",
        ));
    }
    let mut file = fs::File::create(output)?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

fn emit_generated_field_methods(
    text: &mut String,
    field: &PlannedFieldSpec,
    method: &str,
    bytes_expr: &str,
) -> Result<(), CliError> {
    let end = checked_add(field.offset, field.byte_len, "generated field")?;
    text.push_str(&format!(
        "    pub fn {method}_raw(&self) -> Result<&'a [u8], String> {{\n"
    ));
    text.push_str(&format!(
        "        {bytes_expr}.get({}..{}).ok_or_else(|| \"field {} is out of range\".to_string())\n",
        field.offset, end, field.path
    ));
    text.push_str("    }\n\n");
    text.push_str(&format!(
        "    pub fn {method}_hex(&self) -> Result<String, String> {{\n"
    ));
    text.push_str(&format!(
        "        Ok(self.{method}_raw()?.iter().map(|b| format!(\"{{:02X}}\", b)).collect::<Vec<_>>().join(\"\"))\n"
    ));
    text.push_str("    }\n\n");
    text.push_str(&format!(
        "    pub fn {method}(&self) -> Result<{}, String> {{\n",
        generated_return_type(&field.codec)
    ));
    text.push_str(&format!("        let bytes = self.{method}_raw()?;\n"));
    text.push_str(&format!(
        "        {}\n",
        generated_decode_call(&field.codec)?
    ));
    text.push_str("    }\n\n");
    Ok(())
}

fn emit_generated_expected_record_len(
    text: &mut String,
    plan: &PlannedSchema,
) -> Result<(), CliError> {
    let static_len = static_record_len_for_generated(&plan.items).max(plan.odo_header_len);
    text.push_str("    fn expected_record_len(&self) -> Result<usize, String> {\n");
    text.push_str(&format!("        let mut required = {static_len}usize;\n"));
    emit_expected_len_for_items(text, &plan.items)?;
    text.push_str("        Ok(required)\n");
    text.push_str("    }\n\n");
    Ok(())
}

fn emit_expected_len_for_items(
    text: &mut String,
    items: &[PlannedLayoutItem],
) -> Result<(), CliError> {
    for item in items {
        if let PlannedLayoutItem::OccursGroup(group) = item {
            let count_method = occurs_count_method_name(group);
            text.push_str(&format!("        let count = self.{count_method}()?;\n"));
            text.push_str(&format!(
                "        let group_len = count.checked_mul({}).ok_or_else(|| \"OCCURS group {} length overflow\".to_string())?;\n",
                group.element_byte_len, group.name
            ));
            text.push_str(&format!(
                "        let group_end = {}usize.checked_add(group_len).ok_or_else(|| \"OCCURS group {} end offset overflow\".to_string())?;\n",
                group.offset, group.name
            ));
            text.push_str("        required = required.max(group_end);\n");
        }
    }
    Ok(())
}

fn emit_occurs_record_methods(
    text: &mut String,
    items: &[PlannedLayoutItem],
) -> Result<(), CliError> {
    for item in items {
        let PlannedLayoutItem::OccursGroup(group) = item else {
            continue;
        };
        ensure_generated_occurs_element_supported(group)?;
        let method = rust_ident(&group.name);
        let count_method = occurs_count_method_name(group);
        let element_type = occurs_element_type_name(group);
        text.push_str(&format!(
            "    fn {count_method}(&self) -> Result<usize, String> {{\n"
        ));
        let counter_end = checked_add(
            group.counter_field.offset,
            group.counter_field.byte_len,
            "generated counter",
        )?;
        text.push_str(&format!(
            "        let counter_bytes = self.bytes.get({}..{}).ok_or_else(|| \"OCCURS counter {} is out of range\".to_string())?;\n",
            group.counter_field.offset, counter_end, group.counter_field.path
        ));
        text.push_str(&format!(
            "        let counter_value = {{ let bytes = counter_bytes; {} }}?;\n",
            generated_decode_call(&group.counter_field.codec)?
        ));
        text.push_str(&format!(
            "        let count = format!(\"{{}}\", counter_value).parse::<usize>().map_err(|_| \"OCCURS counter {} is not a non-negative integer\".to_string())?;\n",
            group.counter_field.path
        ));
        if group.min_occurs == 0 {
            text.push_str(&format!(
                "        if count > {} {{ return Err(format!(\"OCCURS counter {{}} is outside {}..={}\", count)); }}\n",
                group.max_occurs, group.min_occurs, group.max_occurs
            ));
        } else {
            text.push_str(&format!(
                "        if count < {} || count > {} {{ return Err(format!(\"OCCURS counter {{}} is outside {}..={}\", count)); }}\n",
                group.min_occurs, group.max_occurs, group.min_occurs, group.max_occurs
            ));
        }
        text.push_str("        Ok(count)\n");
        text.push_str("    }\n\n");
        text.push_str(&format!(
            "    pub fn {method}(&self) -> Result<Vec<{element_type}<'a>>, String> {{\n"
        ));
        text.push_str(&format!("        let count = self.{count_method}()?;\n"));
        text.push_str("        let mut values = Vec::with_capacity(count);\n");
        text.push_str("        for occurrence in 0..count {\n");
        text.push_str(&format!(
            "            let delta = occurrence.checked_mul({}).ok_or_else(|| \"OCCURS occurrence offset overflow\".to_string())?;\n",
            group.element_byte_len
        ));
        text.push_str(&format!(
            "            let start = {}usize.checked_add(delta).ok_or_else(|| \"OCCURS occurrence start overflow\".to_string())?;\n",
            group.offset
        ));
        text.push_str(&format!(
            "            let end = start.checked_add({}).ok_or_else(|| \"OCCURS occurrence end overflow\".to_string())?;\n",
            group.element_byte_len
        ));
        text.push_str(&format!(
            "            let bytes = self.bytes.get(start..end).ok_or_else(|| format!(\"OCCURS group {} occurrence {{}} is out of range\", occurrence))?;\n",
            group.name
        ));
        text.push_str(&format!(
            "            values.push({element_type} {{ bytes }});\n"
        ));
        text.push_str("        }\n");
        text.push_str("        Ok(values)\n");
        text.push_str("    }\n\n");
    }
    Ok(())
}

fn emit_redefines_record_methods(text: &mut String, items: &[PlannedLayoutItem]) {
    for item in items {
        if let PlannedLayoutItem::RedefinesGroup(group) = item {
            let method = rust_ident(&group.name);
            let type_name = rust_type_ident(&group.name);
            let end = group.offset.saturating_add(group.base_length);
            text.push_str(&format!(
                "    pub fn {method}(&self) -> Result<{type_name}<'a>, String> {{\n"
            ));
            text.push_str(&format!(
                "        let bytes = self.bytes.get({}..{}).ok_or_else(|| \"REDEFINES group {} is out of range\".to_string())?;\n",
                group.offset, end, group.name
            ));
            text.push_str(&format!(
                "        Ok({type_name} {{ record_bytes: self.bytes, bytes }})\n"
            ));
            text.push_str("    }\n\n");
        }
    }
}

fn emit_redefines_types(text: &mut String, items: &[PlannedLayoutItem]) -> Result<(), CliError> {
    for item in items {
        if let PlannedLayoutItem::RedefinesGroup(group) = item {
            let type_name = rust_type_ident(&group.name);
            text.push_str(&format!(
                "pub struct {type_name}<'a> {{\n    record_bytes: &'a [u8],\n    bytes: &'a [u8],\n}}\n\n"
            ));
            text.push_str(&format!("impl<'a> {type_name}<'a> {{\n"));
            for variant in &group.variants {
                let variant_method = rust_ident(&variant._name);
                let variant_type = rust_type_ident(&format!("{}_{}", group.name, variant._name));
                text.push_str(&format!(
                    "    pub fn {variant_method}(&self) -> {variant_type}<'a> {{\n"
                ));
                text.push_str(&format!(
                    "        {variant_type} {{ record_bytes: self.record_bytes, bytes: self.bytes }}\n"
                ));
                text.push_str("    }\n\n");
            }
            text.push_str(&format!(
                "    pub fn select(&self, variant: &str) -> {type_name}Variant<'a> {{\n"
            ));
            text.push_str("        match variant {\n");
            for variant in &group.variants {
                let variant_type = rust_type_ident(&format!("{}_{}", group.name, variant._name));
                let enum_variant = rust_type_ident(&variant._name);
                text.push_str(&format!(
                    "            {:?} => {type_name}Variant::{enum_variant}({variant_type} {{ record_bytes: self.record_bytes, bytes: self.bytes }}),\n",
                    variant._name
                ));
            }
            text.push_str(&format!(
                "            _ => {type_name}Variant::Unknown({type_name} {{ record_bytes: self.record_bytes, bytes: self.bytes }}),\n"
            ));
            text.push_str("        }\n");
            text.push_str("    }\n\n");
            if group
                .variants
                .iter()
                .any(|variant| variant.selector.is_some())
            {
                text.push_str(&format!(
                    "    pub fn selected(&self) -> Result<{type_name}Variant<'a>, String> {{\n"
                ));
                for variant in &group.variants {
                    let Some(selector) = &variant.selector else {
                        continue;
                    };
                    let selector_end =
                        checked_add(selector.field.offset, selector.field.byte_len, "selector")?;
                    let variant_type =
                        rust_type_ident(&format!("{}_{}", group.name, variant._name));
                    let enum_variant = rust_type_ident(&variant._name);
                    text.push_str(&format!(
                        "        let selector_bytes = self.record_bytes.get({}..{}).ok_or_else(|| \"selector field {} is out of range\".to_string())?;\n",
                        selector.field.offset, selector_end, selector.field.path
                    ));
                    text.push_str(&format!(
                        "        let selector_value = {{ let bytes = selector_bytes; {} }}?;\n",
                        generated_decode_call(&selector.field.codec)?
                    ));
                    text.push_str(&format!(
                        "        if format!(\"{{}}\", selector_value) == {:?} {{\n",
                        selector.equals
                    ));
                    text.push_str(&format!(
                        "            return Ok({type_name}Variant::{enum_variant}({variant_type} {{ record_bytes: self.record_bytes, bytes: self.bytes }}));\n"
                    ));
                    text.push_str("        }\n");
                }
                text.push_str(&format!(
                    "        Ok({type_name}Variant::Unknown({type_name} {{ record_bytes: self.record_bytes, bytes: self.bytes }}))\n"
                ));
                text.push_str("    }\n\n");
            }
            text.push_str("}\n\n");
            text.push_str(&format!("pub enum {type_name}Variant<'a> {{\n"));
            for variant in &group.variants {
                let variant_type = rust_type_ident(&format!("{}_{}", group.name, variant._name));
                text.push_str(&format!(
                    "    {}({variant_type}<'a>),\n",
                    rust_type_ident(&variant._name)
                ));
            }
            text.push_str(&format!("    Unknown({type_name}<'a>),\n"));
            text.push_str("}\n\n");
            for variant in &group.variants {
                let variant_type = rust_type_ident(&format!("{}_{}", group.name, variant._name));
                text.push_str(&format!(
                    "pub struct {variant_type}<'a> {{\n    record_bytes: &'a [u8],\n    bytes: &'a [u8],\n}}\n\n"
                ));
                text.push_str(&format!("impl<'a> {variant_type}<'a> {{\n"));
                for field in fields_from_items(&variant.items) {
                    emit_generated_field_methods(
                        text,
                        field,
                        &rust_ident(&field.name),
                        "self.bytes",
                    )?;
                }
                text.push_str("}\n\n");
            }
        }
    }
    Ok(())
}

fn emit_occurs_element_types(
    text: &mut String,
    items: &[PlannedLayoutItem],
) -> Result<(), CliError> {
    for item in items {
        let PlannedLayoutItem::OccursGroup(group) = item else {
            continue;
        };
        ensure_generated_occurs_element_supported(group)?;
        let element_type = occurs_element_type_name(group);
        text.push_str(&format!(
            "pub struct {element_type}<'a> {{\n    bytes: &'a [u8],\n}}\n\n"
        ));
        text.push_str(&format!("impl<'a> {element_type}<'a> {{\n"));
        for field in direct_fields_from_items(&group.element_items) {
            emit_generated_field_methods(text, field, &rust_ident(&field.name), "self.bytes")?;
        }
        text.push_str("}\n\n");
    }
    Ok(())
}

fn direct_fields_from_items(items: &[PlannedLayoutItem]) -> Vec<&PlannedFieldSpec> {
    let mut fields = Vec::new();
    for item in items {
        if let PlannedLayoutItem::Field(field) = item {
            fields.push(field);
        }
    }
    fields
}

fn fields_from_items(items: &[PlannedLayoutItem]) -> Vec<&PlannedFieldSpec> {
    let mut fields = Vec::new();
    for item in items {
        match item {
            PlannedLayoutItem::Field(field) => fields.push(field),
            PlannedLayoutItem::OccursGroup(group) => {
                fields.extend(fields_from_items(&group.element_items));
            }
            PlannedLayoutItem::RedefinesGroup(group) => {
                for variant in &group.variants {
                    fields.extend(fields_from_items(&variant.items));
                }
            }
            PlannedLayoutItem::Filler { .. } | PlannedLayoutItem::SyncSlack { .. } => {}
        }
    }
    fields
}

fn occurs_count_method_name(group: &PlannedOccursGroup) -> String {
    format!("__occurs_count_{}", rust_ident(&group.name))
}

fn occurs_element_type_name(group: &PlannedOccursGroup) -> String {
    rust_type_ident(&format!("{}_element", group.name))
}

fn static_record_len_for_generated(items: &[PlannedLayoutItem]) -> usize {
    items
        .iter()
        .filter_map(|item| match item {
            PlannedLayoutItem::OccursGroup(_) => None,
            other => Some(item_end_offset(other)),
        })
        .max()
        .unwrap_or(0)
}

fn ensure_generated_occurs_element_supported(group: &PlannedOccursGroup) -> Result<(), CliError> {
    for item in &group.element_items {
        match item {
            PlannedLayoutItem::Field(_)
            | PlannedLayoutItem::Filler { .. }
            | PlannedLayoutItem::SyncSlack { .. } => {}
            PlannedLayoutItem::OccursGroup(_) | PlannedLayoutItem::RedefinesGroup(_) => {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "schema emit-rust currently supports OCCURS elements with scalar fields, fillers, and sync slack only; group {} contains nested groups",
                        group.name
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn ensure_emit_rust_supported_items(items: &[PlannedLayoutItem]) -> Result<(), CliError> {
    for item in items {
        match item {
            PlannedLayoutItem::Field(_)
            | PlannedLayoutItem::Filler { .. }
            | PlannedLayoutItem::SyncSlack { .. } => {}
            PlannedLayoutItem::OccursGroup(group) => {
                ensure_generated_occurs_element_supported(group)?;
            }
            PlannedLayoutItem::RedefinesGroup(group) => {
                for variant in &group.variants {
                    ensure_generated_redefines_variant_supported(group, variant)?;
                }
            }
        }
    }
    Ok(())
}

fn ensure_generated_redefines_variant_supported(
    group: &PlannedRedefinesGroup,
    variant: &PlannedRedefinesVariant,
) -> Result<(), CliError> {
    for item in &variant.items {
        match item {
            PlannedLayoutItem::Field(_)
            | PlannedLayoutItem::Filler { .. }
            | PlannedLayoutItem::SyncSlack { .. } => {}
            PlannedLayoutItem::OccursGroup(_) | PlannedLayoutItem::RedefinesGroup(_) => {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "schema emit-rust currently supports scalar REDEFINES variants only; group {} variant {} contains nested groups",
                        group.name, variant._name
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn generated_return_type(codec: &FieldCodec) -> &'static str {
    match codec {
        FieldCodec::PackedDecimal { .. } | FieldCodec::ZonedDecimal { .. } => {
            "rust_decimal::Decimal"
        }
        FieldCodec::Binary { signed, scale, .. }
        | FieldCodec::NativeBinary { signed, scale, .. } => {
            if *scale > 0 {
                "rust_decimal::Decimal"
            } else if *signed {
                "i64"
            } else {
                "u64"
            }
        }
        FieldCodec::IbmFloat32 { .. } | FieldCodec::IbmFloat64 { .. } => "f64",
        FieldCodec::Alphanumeric { .. } => "String",
        FieldCodec::Bytes { .. } => "Vec<u8>",
    }
}

fn generated_decode_call(codec: &FieldCodec) -> Result<String, CliError> {
    Ok(match codec {
        FieldCodec::PackedDecimal {
            cfg,
            sign_mode,
            mode,
        } => format!(
            "decode_packed_decimal_generated(bytes, {}, {}, {}, \"{}\", \"{}\")",
            cfg.total_digits(),
            cfg.scale(),
            cfg.is_signed(),
            generated_sign_mode(*sign_mode),
            generated_field_mode(*mode)
        ),
        FieldCodec::ZonedDecimal {
            scale,
            signed,
            encoding,
            sign_policy,
            ..
        } => {
            let (policy, blank, zero) = generated_sign_policy(*sign_policy);
            format!(
                "decode_zoned_decimal_generated(bytes, {}, {}, \"{}\", \"{}\", {}, {})",
                scale,
                signed,
                generated_encoding(*encoding),
                policy,
                blank,
                zero
            )
        }
        FieldCodec::Binary {
            endian,
            signed,
            scale,
            ..
        }
        | FieldCodec::NativeBinary {
            endian,
            signed,
            scale,
            ..
        } => {
            if *scale > 0 {
                format!(
                    "decode_binary_decimal_generated(bytes, \"{}\", {}, {})",
                    generated_endian(*endian),
                    signed,
                    scale
                )
            } else if *signed {
                format!(
                    "decode_binary_i64_generated(bytes, \"{}\")",
                    generated_endian(*endian)
                )
            } else {
                format!(
                    "decode_binary_u64_generated(bytes, \"{}\")",
                    generated_endian(*endian)
                )
            }
        }
        FieldCodec::IbmFloat32 { endian } => format!(
            "decode_ibm_float32_generated(bytes, \"{}\")",
            generated_endian(*endian)
        ),
        FieldCodec::IbmFloat64 { endian } => format!(
            "decode_ibm_float64_generated(bytes, \"{}\")",
            generated_endian(*endian)
        ),
        FieldCodec::Alphanumeric {
            encoding, codepage, ..
        } => format!(
            "decode_text_generated(bytes, \"{}\", {})",
            generated_encoding(*encoding),
            codepage.map(|cp| cp.0).unwrap_or(0)
        ),
        FieldCodec::Bytes { .. } => "Ok(bytes.to_vec())".to_string(),
    })
}

fn generated_sign_mode(sign_mode: CliSignMode) -> &'static str {
    match sign_mode {
        CliSignMode::Pfd => "pfd",
        CliSignMode::Nopfd => "nopfd",
    }
}

fn generated_field_mode(mode: FieldMode) -> &'static str {
    match mode {
        FieldMode::Canonical => "canonical",
        FieldMode::Lossless => "lossless",
    }
}

fn generated_encoding(encoding: FieldEncoding) -> &'static str {
    match encoding {
        FieldEncoding::Ebcdic => "ebcdic",
        FieldEncoding::Ascii => "ascii",
        FieldEncoding::AsciiOverpunch => "ascii-overpunch",
    }
}

fn generated_endian(endian: Endian) -> &'static str {
    match endian {
        Endian::Big => "big",
        Endian::Little => "little",
    }
}

fn generated_sign_policy(policy: SignPolicy) -> (&'static str, bool, bool) {
    match policy {
        SignPolicy::Preferred => ("preferred", false, false),
        SignPolicy::NonPreferred => ("non-preferred", false, false),
        SignPolicy::Permissive {
            blank_as_positive,
            zero_nibble_as_positive,
        } => ("permissive", blank_as_positive, zero_nibble_as_positive),
    }
}

fn emit_generated_helpers(text: &mut String) {
    text.push_str(GENERATED_HELPERS);
    text.push_str("const CP037: [u16; 256] = ");
    text.push_str(&format_u16_table(&CP037));
    text.push_str(";\n\nconst CP500: [u16; 256] = ");
    text.push_str(&format_u16_table(&CP500));
    text.push_str(";\n");
}

fn format_u16_table(values: &[u16; 256]) -> String {
    let mut out = String::from("[\n");
    for chunk in values.chunks(8) {
        out.push_str("    ");
        for value in chunk {
            out.push_str(&format!("0x{value:04X}, "));
        }
        out.push('\n');
    }
    out.push(']');
    out
}

const GENERATED_HELPERS: &str = r#"
fn decode_packed_decimal_generated(
    bytes: &[u8],
    total_digits: u8,
    scale: u8,
    signed: bool,
    sign_mode: &str,
    mode: &str,
) -> Result<rust_decimal::Decimal, String> {
    let cfg = cobol_packed::PackedConfig::new(total_digits, scale, signed)
        .map_err(|err| err.to_string())?;
    let sign_mode = match sign_mode {
        "pfd" => cobol_packed::SignMode::Pfd,
        "nopfd" => cobol_packed::SignMode::Nopfd,
        _ => return Err("invalid generated sign mode".to_string()),
    };
    match mode {
        "canonical" => cobol_packed::from_packed(bytes, &cfg, sign_mode)
            .map_err(|err| err.to_string()),
        "lossless" => cobol_packed::from_packed_lossless(bytes, &cfg, sign_mode)
            .map(|value| value.value)
            .map_err(|err| err.to_string()),
        _ => Err("invalid generated packed mode".to_string()),
    }
}

fn decode_zoned_decimal_generated(
    bytes: &[u8],
    scale: u8,
    signed: bool,
    encoding: &str,
    policy: &str,
    blank_as_positive: bool,
    zero_nibble_as_positive: bool,
) -> Result<rust_decimal::Decimal, String> {
    if bytes.is_empty() {
        return Err("zoned decimal field is empty".to_string());
    }
    let (mantissa, negative) = match encoding {
        "ebcdic" => decode_ebcdic_zoned_generated(
            bytes,
            signed,
            policy,
            blank_as_positive,
            zero_nibble_as_positive,
        )?,
        "ascii-overpunch" => decode_ascii_overpunch_generated(bytes, signed)?,
        _ => return Err("zoned decimal requires ebcdic or ascii-overpunch".to_string()),
    };
    let mantissa = if negative {
        mantissa
            .checked_neg()
            .ok_or_else(|| "zoned decimal mantissa overflow".to_string())?
    } else {
        mantissa
    };
    Ok(rust_decimal::Decimal::from_i128_with_scale(
        mantissa,
        u32::from(scale),
    ))
}

fn decode_ebcdic_zoned_generated(
    bytes: &[u8],
    signed: bool,
    policy: &str,
    blank_as_positive: bool,
    zero_nibble_as_positive: bool,
) -> Result<(i128, bool), String> {
    let mut mantissa = 0i128;
    for &byte in &bytes[..bytes.len() - 1] {
        let zone = byte >> 4;
        let digit = byte & 0x0F;
        if zone != 0xF || digit > 9 {
            return Err(format!("invalid EBCDIC zoned digit byte 0x{byte:02X}"));
        }
        push_decimal_digit_generated(&mut mantissa, digit)?;
    }
    let last = bytes[bytes.len() - 1];
    let zone = last >> 4;
    let digit = last & 0x0F;
    if digit > 9 {
        return Err(format!("invalid zoned digit nibble 0x{digit:X}"));
    }
    let negative = match policy {
        "preferred" => match zone {
            0xC | 0xF => false,
            0xD if signed => true,
            0xD => return Err("negative zoned sign in unsigned field".to_string()),
            _ => return Err(format!("invalid preferred zoned sign zone 0x{zone:X}")),
        },
        "non-preferred" => match zone {
            0xA | 0xC | 0xE | 0xF => false,
            0xB | 0xD if signed => true,
            0xB | 0xD => return Err("negative zoned sign in unsigned field".to_string()),
            _ => return Err(format!("invalid non-preferred zoned sign zone 0x{zone:X}")),
        },
        "permissive" => match zone {
            0xA | 0xC | 0xE | 0xF => false,
            0xB | 0xD if signed => true,
            0xB | 0xD => return Err("negative zoned sign in unsigned field".to_string()),
            0x0 if zero_nibble_as_positive => false,
            0x4 if blank_as_positive && digit == 0 => false,
            _ => return Err(format!("invalid permissive zoned sign zone 0x{zone:X}")),
        },
        _ => return Err("invalid generated zoned sign policy".to_string()),
    };
    push_decimal_digit_generated(&mut mantissa, digit)?;
    Ok((mantissa, negative))
}

fn decode_ascii_overpunch_generated(bytes: &[u8], signed: bool) -> Result<(i128, bool), String> {
    let mut mantissa = 0i128;
    for &byte in &bytes[..bytes.len() - 1] {
        if !byte.is_ascii_digit() {
            return Err(format!("invalid ASCII digit byte 0x{byte:02X}"));
        }
        push_decimal_digit_generated(&mut mantissa, byte - b'0')?;
    }
    let last = bytes[bytes.len() - 1];
    let (digit, negative) = match last {
        b'0'..=b'9' => (last - b'0', false),
        b'{' => (0, false),
        b'A'..=b'I' => (last - b'A' + 1, false),
        b'}' if signed => (0, true),
        b'J'..=b'R' if signed => (last - b'J' + 1, true),
        b'}' | b'J'..=b'R' => {
            return Err("negative ASCII overpunch sign in unsigned field".to_string())
        }
        _ => return Err(format!("invalid ASCII overpunch byte 0x{last:02X}")),
    };
    push_decimal_digit_generated(&mut mantissa, digit)?;
    Ok((mantissa, negative))
}

fn push_decimal_digit_generated(mantissa: &mut i128, digit: u8) -> Result<(), String> {
    *mantissa = mantissa
        .checked_mul(10)
        .and_then(|value| value.checked_add(i128::from(digit)))
        .ok_or_else(|| "decimal mantissa overflow".to_string())?;
    Ok(())
}

fn decode_binary_decimal_generated(
    bytes: &[u8],
    endian: &str,
    signed: bool,
    scale: u8,
) -> Result<rust_decimal::Decimal, String> {
    let mantissa = if signed {
        i128::from(decode_binary_i64_generated(bytes, endian)?)
    } else {
        i128::from(decode_binary_u64_generated(bytes, endian)?)
    };
    Ok(rust_decimal::Decimal::from_i128_with_scale(
        mantissa,
        u32::from(scale),
    ))
}

fn decode_binary_i64_generated(bytes: &[u8], endian: &str) -> Result<i64, String> {
    match bytes.len() {
        2 => Ok(i64::from(read_i16_generated(bytes, endian)?)),
        4 => Ok(i64::from(read_i32_generated(bytes, endian)?)),
        8 => read_i64_generated(bytes, endian),
        _ => Err("binary field length must be 2, 4, or 8".to_string()),
    }
}

fn decode_binary_u64_generated(bytes: &[u8], endian: &str) -> Result<u64, String> {
    match bytes.len() {
        2 => Ok(u64::from(read_u16_generated(bytes, endian)?)),
        4 => Ok(u64::from(read_u32_generated(bytes, endian)?)),
        8 => read_u64_generated(bytes, endian),
        _ => Err("binary field length must be 2, 4, or 8".to_string()),
    }
}

fn decode_ibm_float32_generated(bytes: &[u8], endian: &str) -> Result<f64, String> {
    let raw = read_u32_generated(bytes, endian)?;
    ibm_hex_float_to_f64_generated(
        u64::from(raw >> 31),
        u64::from((raw >> 24) & 0x7F),
        u64::from(raw & 0x00FF_FFFF),
        24,
    )
}

fn decode_ibm_float64_generated(bytes: &[u8], endian: &str) -> Result<f64, String> {
    let raw = read_u64_generated(bytes, endian)?;
    ibm_hex_float_to_f64_generated(raw >> 63, (raw >> 56) & 0x7F, raw & 0x00FF_FFFF_FFFF_FFFF, 56)
}

fn ibm_hex_float_to_f64_generated(
    sign: u64,
    exponent: u64,
    mantissa: u64,
    mantissa_bits: i32,
) -> Result<f64, String> {
    if mantissa == 0 {
        return Ok(0.0);
    }
    let sign = if sign == 0 { 1.0 } else { -1.0 };
    let exp = i32::try_from(exponent).map_err(|_| "IBM float exponent overflow".to_string())? - 64;
    let value = sign * ((mantissa as f64) / 2f64.powi(mantissa_bits)) * 16f64.powi(exp);
    if value.is_finite() {
        Ok(value)
    } else {
        Err("IBM hexadecimal float converted to non-finite value".to_string())
    }
}

fn read_u16_generated(bytes: &[u8], endian: &str) -> Result<u16, String> {
    if bytes.len() != 2 {
        return Err("expected 2 bytes".to_string());
    }
    let arr = [bytes[0], bytes[1]];
    Ok(match endian {
        "big" => u16::from_be_bytes(arr),
        "little" => u16::from_le_bytes(arr),
        _ => return Err("invalid generated endian".to_string()),
    })
}

fn read_i16_generated(bytes: &[u8], endian: &str) -> Result<i16, String> {
    Ok(match endian {
        "big" => i16::from_be_bytes(read_u16_generated(bytes, endian)?.to_be_bytes()),
        "little" => i16::from_le_bytes(read_u16_generated(bytes, endian)?.to_le_bytes()),
        _ => return Err("invalid generated endian".to_string()),
    })
}

fn read_u32_generated(bytes: &[u8], endian: &str) -> Result<u32, String> {
    if bytes.len() != 4 {
        return Err("expected 4 bytes".to_string());
    }
    let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
    Ok(match endian {
        "big" => u32::from_be_bytes(arr),
        "little" => u32::from_le_bytes(arr),
        _ => return Err("invalid generated endian".to_string()),
    })
}

fn read_i32_generated(bytes: &[u8], endian: &str) -> Result<i32, String> {
    Ok(match endian {
        "big" => i32::from_be_bytes(read_u32_generated(bytes, endian)?.to_be_bytes()),
        "little" => i32::from_le_bytes(read_u32_generated(bytes, endian)?.to_le_bytes()),
        _ => return Err("invalid generated endian".to_string()),
    })
}

fn read_u64_generated(bytes: &[u8], endian: &str) -> Result<u64, String> {
    if bytes.len() != 8 {
        return Err("expected 8 bytes".to_string());
    }
    let arr = [
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
    ];
    Ok(match endian {
        "big" => u64::from_be_bytes(arr),
        "little" => u64::from_le_bytes(arr),
        _ => return Err("invalid generated endian".to_string()),
    })
}

fn read_i64_generated(bytes: &[u8], endian: &str) -> Result<i64, String> {
    Ok(match endian {
        "big" => i64::from_be_bytes(read_u64_generated(bytes, endian)?.to_be_bytes()),
        "little" => i64::from_le_bytes(read_u64_generated(bytes, endian)?.to_le_bytes()),
        _ => return Err("invalid generated endian".to_string()),
    })
}

fn decode_text_generated(bytes: &[u8], encoding: &str, codepage: u16) -> Result<String, String> {
    match encoding {
        "ascii" => {
            if !bytes.is_ascii() {
                return Err("ASCII text contains non-ASCII bytes".to_string());
            }
            let text = std::str::from_utf8(bytes)
                .map_err(|err| format!("ASCII text is not valid UTF-8: {err}"))?;
            if text.chars().any(char::is_control) {
                return Err("ASCII text contains rejected control bytes".to_string());
            }
            Ok(text.to_string())
        }
        "ebcdic" => decode_ebcdic_text_generated(bytes, codepage),
        _ => Err("unsupported generated text encoding".to_string()),
    }
}

fn decode_ebcdic_text_generated(bytes: &[u8], codepage: u16) -> Result<String, String> {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        let code = ebcdic_codepoint_generated(byte, codepage);
        let ch = char::from_u32(u32::from(code))
            .ok_or_else(|| format!("unsupported EBCDIC byte 0x{byte:02X}"))?;
        if ch.is_control() {
            return Err(format!("unsupported or control EBCDIC byte 0x{byte:02X}"));
        }
        out.push(ch);
    }
    Ok(out)
}

fn ebcdic_codepoint_generated(byte: u8, codepage: u16) -> u16 {
    if byte == 0x9F && matches!(codepage, 1140 | 1148) {
        return 0x20AC;
    }
    let table = match codepage {
        37 | 1140 => &CP037,
        500 | 1148 => &CP500,
        _ => &CP037,
    };
    table[usize::from(byte)]
}

"#;

fn plan_v1_schema(schema: &Schema) -> Result<PlannedSchema, CliError> {
    let mut state = PlanState {
        fields: Vec::new(),
        ranges: Vec::new(),
        names: BTreeSet::new(),
        has_occurs: false,
        odo_header_len: 0,
    };
    for field in &schema.fields {
        let cfg = PackedConfig::new(field.total_digits, field.scale, field.signed)
            .map_err(map_packed_error)?;
        let byte_len = field.length.unwrap_or_else(|| cfg.byte_len());
        let offset = field.offset.unwrap_or(0);
        let codec = FieldCodec::PackedDecimal {
            cfg,
            sign_mode: field.sign_mode,
            mode: field.mode,
        };
        let planned = PlannedFieldSpec {
            name: field.name.clone(),
            path: field.name.clone(),
            offset,
            byte_len,
            codec,
            required: field.required,
            sync: false,
        };
        state.fields.push(planned.clone());
        state.ranges.push(LayoutRangeSummary {
            name: field.name.clone(),
            kind: LayoutKind::Field,
            offset,
            end_offset: checked_add(offset, byte_len, "field")?,
            length: byte_len,
        });
    }
    for filler in &schema.fillers {
        state.ranges.push(LayoutRangeSummary {
            name: filler.name.clone(),
            kind: LayoutKind::Filler,
            offset: filler.offset,
            end_offset: checked_add(filler.offset, filler.length, "filler")?,
            length: filler.length,
        });
    }
    let max_record_len = schema.record_length.unwrap_or_else(|| {
        state
            .ranges
            .iter()
            .map(|range| range.end_offset)
            .max()
            .unwrap_or(0)
    });
    Ok(PlannedSchema {
        record_length: schema.record_length,
        input_encoding: schema.input_encoding,
        on_error: schema.on_error,
        output: schema.output,
        verification_scope: schema.verification_scope,
        layout_mode: LayoutMode::Declared,
        platform_profile: PlatformProfile::IbmZOs,
        items: state
            .fields
            .iter()
            .cloned()
            .map(PlannedLayoutItem::Field)
            .collect(),
        fields: state.fields,
        ranges: state.ranges,
        has_occurs: false,
        max_record_len,
        odo_header_len: 0,
    })
}

fn plan_v2_schema(schema: &Schema) -> Result<PlannedSchema, CliError> {
    if !schema.fields.is_empty() || !schema.fillers.is_empty() {
        return Err(CliError::config(
            "E_SCHEMA",
            "schema v2 uses layout items, not top-level fields/fillers",
        ));
    }
    if schema.layout.is_empty() {
        return Err(CliError::config("E_SCHEMA", "schema v2 requires layout"));
    }
    if schema.layout.len() > super::MAX_SCHEMA_FIELDS {
        return Err(CliError::config(
            "E_SCHEMA",
            format!(
                "schema supports at most {} top-level layout items",
                super::MAX_SCHEMA_FIELDS
            ),
        ));
    }
    let layout_mode = schema.layout_mode.ok_or_else(|| {
        CliError::config(
            "E_SCHEMA",
            "schema v2 requires layout_mode: declared or sequential",
        )
    })?;
    if matches!(
        schema.input_encoding,
        InputEncoding::Csv | InputEncoding::Jsonl
    ) {
        return Err(CliError::config(
            "E_SCHEMA",
            "schema v2 record layouts currently support binary or hex input",
        ));
    }
    let mut state = PlanState {
        fields: Vec::new(),
        ranges: Vec::new(),
        names: BTreeSet::new(),
        has_occurs: false,
        odo_header_len: 0,
    };
    let planned = plan_items(
        &schema.layout,
        layout_mode,
        schema.platform_profile,
        0,
        "",
        true,
        &mut state,
    )?;
    if state.fields.len() > super::MAX_SCHEMA_FIELDS {
        return Err(CliError::config(
            "E_SCHEMA",
            format!(
                "schema supports at most {} planned decoded fields",
                super::MAX_SCHEMA_FIELDS
            ),
        ));
    }
    if state.ranges.len() > super::MAX_SCHEMA_FIELDS {
        return Err(CliError::config(
            "E_SCHEMA",
            format!(
                "schema supports at most {} planned coverage ranges",
                super::MAX_SCHEMA_FIELDS
            ),
        ));
    }
    let max_record_len = schema.record_length.unwrap_or(planned.total_size);
    if max_record_len > MAX_RECORD_BYTES {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("planned record length exceeds {MAX_RECORD_BYTES} bytes"),
        ));
    }
    if let Some(record_length) = schema.record_length {
        if planned.total_size > record_length {
            return Err(CliError::config(
                "E_SCHEMA",
                "planned layout extends past record_length",
            ));
        }
    }
    Ok(PlannedSchema {
        record_length: schema.record_length,
        input_encoding: schema.input_encoding,
        on_error: schema.on_error,
        output: schema.output,
        verification_scope: schema.verification_scope,
        layout_mode,
        platform_profile: schema.platform_profile,
        items: planned.items,
        fields: state.fields,
        ranges: state.ranges,
        has_occurs: state.has_occurs,
        max_record_len,
        odo_header_len: state.odo_header_len,
    })
}

fn plan_items(
    raw_items: &[RawLayoutItem],
    mode: LayoutMode,
    platform: PlatformProfile,
    base_offset: usize,
    prefix: &str,
    add_coverage: bool,
    state: &mut PlanState,
) -> Result<PlanResult, CliError> {
    let mut cursor = 0usize;
    let mut planned_items = Vec::new();
    for (idx, raw) in raw_items.iter().enumerate() {
        match raw {
            RawLayoutItem::Field(raw_field) => {
                let item_offset =
                    item_offset(mode, raw_field.offset, cursor, base_offset, "field")?;
                let mut adjusted_offset = item_offset;
                let codec = build_codec(raw_field)?;
                let byte_len = codec.byte_len();
                validate_field_name(&raw_field.name)?;
                validate_declared_length(raw_field.length, byte_len, &raw_field.name)?;
                if mode == LayoutMode::Sequential && raw_field.sync {
                    let align = alignment_for(&codec, platform);
                    let slack = sync_slack(adjusted_offset, align);
                    if slack > 0 {
                        if add_coverage {
                            state.ranges.push(LayoutRangeSummary {
                                name: format!("{prefix}sync-slack@{adjusted_offset}"),
                                kind: LayoutKind::SyncSlack,
                                offset: adjusted_offset,
                                end_offset: checked_add(adjusted_offset, slack, "sync slack")?,
                                length: slack,
                            });
                        }
                        planned_items.push(PlannedLayoutItem::SyncSlack {
                            offset: adjusted_offset,
                            length: slack,
                        });
                        adjusted_offset = checked_add(adjusted_offset, slack, "sync slack")?;
                    }
                }
                let path = path_join(prefix, &raw_field.name);
                if !state.names.insert(path.clone()) {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!("duplicate field path {path}"),
                    ));
                }
                let planned = PlannedFieldSpec {
                    name: raw_field.name.clone(),
                    path: path.clone(),
                    offset: adjusted_offset,
                    byte_len,
                    codec,
                    required: raw_field.required,
                    sync: raw_field.sync,
                };
                if add_coverage {
                    state.ranges.push(LayoutRangeSummary {
                        name: path.clone(),
                        kind: LayoutKind::Field,
                        offset: adjusted_offset,
                        end_offset: checked_add(adjusted_offset, byte_len, "field")?,
                        length: byte_len,
                    });
                }
                state.fields.push(planned.clone());
                planned_items.push(PlannedLayoutItem::Field(planned));
                cursor = checked_add(
                    adjusted_offset.checked_sub(base_offset).ok_or_else(|| {
                        CliError::config("E_SCHEMA", "field offset is before base")
                    })?,
                    byte_len,
                    "field",
                )?;
            }
            RawLayoutItem::Filler(raw_filler) => {
                validate_field_name(&raw_filler.name)?;
                if raw_filler.length == 0 {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!(
                            "filler {} length must be greater than zero",
                            raw_filler.name
                        ),
                    ));
                }
                let offset = item_offset(mode, raw_filler.offset, cursor, base_offset, "filler")?;
                let end = checked_add(offset, raw_filler.length, "filler")?;
                let path = path_join(prefix, &raw_filler.name);
                if add_coverage {
                    state.ranges.push(LayoutRangeSummary {
                        name: path.clone(),
                        kind: LayoutKind::Filler,
                        offset,
                        end_offset: end,
                        length: raw_filler.length,
                    });
                }
                planned_items.push(PlannedLayoutItem::Filler {
                    _name: path,
                    offset,
                    length: raw_filler.length,
                });
                cursor = end
                    .checked_sub(base_offset)
                    .ok_or_else(|| CliError::config("E_SCHEMA", "filler offset is before base"))?;
            }
            RawLayoutItem::OccursGroup(raw_group) => {
                if mode == LayoutMode::Sequential && idx + 1 != raw_items.len() {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!(
                            "sequential OCCURS group {} must be terminal until dynamic following-item offsets are implemented",
                            raw_group.name
                        ),
                    ));
                }
                validate_field_name(&raw_group.name)?;
                if raw_group.max_occurs < raw_group.min_occurs {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!(
                            "OCCURS group {} max_occurs is below min_occurs",
                            raw_group.name
                        ),
                    ));
                }
                if raw_group.element_layout.is_empty() {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!("OCCURS group {} requires element_layout", raw_group.name),
                    ));
                }
                let offset = item_offset(mode, raw_group.offset, cursor, base_offset, "occurs")?;
                let group_prefix = path_join(prefix, &raw_group.name);
                let before_field_count = state.fields.len();
                let element = plan_items(
                    &raw_group.element_layout,
                    mode,
                    platform,
                    0,
                    &group_prefix,
                    false,
                    state,
                )?;
                let element_fields = state.fields.split_off(before_field_count);
                let element_byte_len = element.total_size;
                if element_byte_len == 0 {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!("OCCURS group {} element length is zero", raw_group.name),
                    ));
                }
                let counter = find_counter_field(&state.fields, &raw_group.counter_field, offset)?;
                let counter_end = checked_add(counter.offset, counter.byte_len, "counter field")?;
                state.odo_header_len = state.odo_header_len.max(counter_end);
                let max_group_len = raw_group
                    .max_occurs
                    .checked_mul(element_byte_len)
                    .ok_or_else(|| CliError::config("E_SCHEMA", "OCCURS group length overflows"))?;
                if add_coverage {
                    state.ranges.push(LayoutRangeSummary {
                        name: group_prefix.clone(),
                        kind: LayoutKind::Occurs,
                        offset,
                        end_offset: checked_add(offset, max_group_len, "OCCURS group")?,
                        length: max_group_len,
                    });
                }
                let mut element_items = element.items;
                rewrite_element_fields(&mut element_items, &element_fields);
                state
                    .fields
                    .extend(absolute_nested_fields(&element_fields, offset)?);
                planned_items.push(PlannedLayoutItem::OccursGroup(PlannedOccursGroup {
                    name: group_prefix,
                    offset,
                    min_occurs: raw_group.min_occurs,
                    max_occurs: raw_group.max_occurs,
                    element_byte_len,
                    counter_field: counter,
                    element_items,
                }));
                state.has_occurs = true;
                cursor = checked_add(
                    offset.checked_sub(base_offset).ok_or_else(|| {
                        CliError::config("E_SCHEMA", "OCCURS offset is before base")
                    })?,
                    max_group_len,
                    "OCCURS group",
                )?;
            }
            RawLayoutItem::RedefinesGroup(raw_group) => {
                validate_field_name(&raw_group.name)?;
                if raw_group.variants.is_empty() {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!("REDEFINES group {} requires variants", raw_group.name),
                    ));
                }
                let offset = item_offset(mode, raw_group.offset, cursor, base_offset, "redefines")?;
                let group_prefix = path_join(prefix, &raw_group.name);
                let mut variants = Vec::new();
                let mut base_length = raw_group.base_length.unwrap_or(0);
                for variant in &raw_group.variants {
                    validate_field_name(&variant.name)?;
                    let selector = match &variant.selector {
                        Some(value) => Some(parse_selector(value, &state.fields)?),
                        None => None,
                    };
                    let variant_prefix = path_join(&group_prefix, &variant.name);
                    let before_field_count = state.fields.len();
                    let planned_variant = plan_items(
                        &variant.layout,
                        mode,
                        platform,
                        0,
                        &variant_prefix,
                        false,
                        state,
                    )?;
                    let variant_fields = state.fields.split_off(before_field_count);
                    let mut items = planned_variant.items;
                    rewrite_element_fields(&mut items, &variant_fields);
                    state
                        .fields
                        .extend(absolute_nested_fields(&variant_fields, offset)?);
                    base_length = base_length.max(planned_variant.total_size);
                    variants.push(PlannedRedefinesVariant {
                        _name: variant.name.clone(),
                        selector,
                        items,
                    });
                }
                if base_length == 0 {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!("REDEFINES group {} base_length is zero", raw_group.name),
                    ));
                }
                if add_coverage {
                    state.ranges.push(LayoutRangeSummary {
                        name: group_prefix.clone(),
                        kind: LayoutKind::RedefinesBase,
                        offset,
                        end_offset: checked_add(offset, base_length, "REDEFINES group")?,
                        length: base_length,
                    });
                }
                planned_items.push(PlannedLayoutItem::RedefinesGroup(PlannedRedefinesGroup {
                    name: group_prefix,
                    offset,
                    base_length,
                    variants,
                }));
                cursor = checked_add(
                    offset.checked_sub(base_offset).ok_or_else(|| {
                        CliError::config("E_SCHEMA", "REDEFINES offset is before base")
                    })?,
                    base_length,
                    "REDEFINES group",
                )?;
            }
        }
    }
    Ok(PlanResult {
        items: planned_items,
        total_size: cursor,
    })
}

fn rewrite_element_fields(items: &mut [PlannedLayoutItem], fields: &[PlannedFieldSpec]) {
    let mut by_path = BTreeMap::new();
    for field in fields {
        by_path.insert(field.path.clone(), field.clone());
    }
    rewrite_item_fields(items, &by_path);
}

fn rewrite_item_fields(
    items: &mut [PlannedLayoutItem],
    fields: &BTreeMap<String, PlannedFieldSpec>,
) {
    for item in items {
        match item {
            PlannedLayoutItem::Field(field) => {
                if let Some(replacement) = fields.get(&field.path) {
                    *field = replacement.clone();
                }
            }
            PlannedLayoutItem::OccursGroup(group) => {
                rewrite_item_fields(&mut group.element_items, fields)
            }
            PlannedLayoutItem::RedefinesGroup(group) => {
                for variant in &mut group.variants {
                    rewrite_item_fields(&mut variant.items, fields);
                }
            }
            PlannedLayoutItem::Filler { .. } | PlannedLayoutItem::SyncSlack { .. } => {}
        }
    }
}

fn absolute_nested_fields(
    fields: &[PlannedFieldSpec],
    base_offset: usize,
) -> Result<Vec<PlannedFieldSpec>, CliError> {
    fields
        .iter()
        .map(|field| {
            let mut absolute = field.clone();
            absolute.offset = checked_add(base_offset, field.offset, "nested field")?;
            Ok(absolute)
        })
        .collect()
}

fn item_offset(
    mode: LayoutMode,
    raw_offset: Option<usize>,
    cursor: usize,
    base_offset: usize,
    item_kind: &str,
) -> Result<usize, CliError> {
    match (mode, raw_offset) {
        (LayoutMode::Declared, Some(offset)) => checked_add(base_offset, offset, item_kind),
        (LayoutMode::Declared, None) => Err(CliError::config(
            "E_SCHEMA",
            format!("declared layout requires explicit {item_kind} offset"),
        )),
        (LayoutMode::Sequential, Some(_)) => Err(CliError::config(
            "E_SCHEMA",
            format!("sequential layout rejects explicit {item_kind} offsets"),
        )),
        (LayoutMode::Sequential, None) => checked_add(base_offset, cursor, item_kind),
    }
}

fn build_codec(raw: &RawFieldSpec) -> Result<FieldCodec, CliError> {
    match raw.field_type {
        CobolType::PackedDecimal => {
            reject_unsupported_option(raw, "sign_policy", raw.sign_policy.is_some())?;
            reject_unsupported_option(raw, "encoding", raw.encoding.is_some())?;
            reject_unsupported_option(raw, "codepage", raw.codepage.is_some())?;
            reject_unsupported_option(raw, "endian", raw.endian.is_some())?;
            let total_digits = required_digits(raw)?;
            let scale = raw.scale.unwrap_or(0);
            let signed = raw.signed.unwrap_or(false);
            let cfg = PackedConfig::new(total_digits, scale, signed).map_err(map_packed_error)?;
            Ok(FieldCodec::PackedDecimal {
                cfg,
                sign_mode: raw.sign_mode.unwrap_or(CliSignMode::Pfd),
                mode: raw.mode.unwrap_or(FieldMode::Lossless),
            })
        }
        CobolType::ZonedDecimal => {
            reject_unsupported_option(raw, "sign_mode", raw.sign_mode.is_some())?;
            reject_unsupported_option(raw, "mode", raw.mode.is_some())?;
            reject_unsupported_option(raw, "endian", raw.endian.is_some())?;
            let total_digits = required_digits(raw)?;
            let scale = raw.scale.unwrap_or(0);
            validate_decimal_digits_and_scale(&raw.name, total_digits, scale)?;
            let encoding = raw.encoding.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("zoned field {} requires encoding", raw.name),
                )
            })?;
            if encoding != FieldEncoding::Ebcdic {
                reject_unsupported_option(raw, "codepage", raw.codepage.is_some())?;
            }
            if encoding == FieldEncoding::AsciiOverpunch {
                reject_unsupported_option(raw, "sign_policy", raw.sign_policy.is_some())?;
            }
            let codepage = if encoding == FieldEncoding::Ebcdic {
                Some(raw.codepage.ok_or_else(|| {
                    CliError::config(
                        "E_SCHEMA",
                        format!("EBCDIC zoned field {} requires codepage", raw.name),
                    )
                })?)
            } else {
                None
            };
            Ok(FieldCodec::ZonedDecimal {
                total_digits,
                scale,
                signed: raw.signed.unwrap_or(false),
                encoding,
                codepage,
                sign_policy: raw.sign_policy.unwrap_or_default(),
            })
        }
        CobolType::Binary | CobolType::NativeBinary => {
            reject_unsupported_option(raw, "sign_mode", raw.sign_mode.is_some())?;
            reject_unsupported_option(raw, "mode", raw.mode.is_some())?;
            reject_unsupported_option(raw, "encoding", raw.encoding.is_some())?;
            reject_unsupported_option(raw, "codepage", raw.codepage.is_some())?;
            if raw.sign_policy.is_some() {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("binary field {} must not set sign_policy", raw.name),
                ));
            }
            let total_digits = required_digits(raw)?;
            let scale = raw.scale.unwrap_or(0);
            validate_decimal_digits_and_scale(&raw.name, total_digits, scale)?;
            let byte_len = binary_width_for_digits(total_digits)?;
            let endian = raw.endian.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("binary field {} requires endian", raw.name),
                )
            })?;
            let signed = raw.signed.unwrap_or(false);
            if raw.field_type == CobolType::Binary {
                Ok(FieldCodec::Binary {
                    byte_len,
                    endian,
                    signed,
                    scale,
                })
            } else {
                Ok(FieldCodec::NativeBinary {
                    byte_len,
                    endian,
                    signed,
                    scale,
                })
            }
        }
        CobolType::IbmFloat32 => {
            reject_float_options(raw)?;
            Ok(FieldCodec::IbmFloat32 {
                endian: raw.endian.ok_or_else(|| {
                    CliError::config(
                        "E_SCHEMA",
                        format!("float field {} requires endian", raw.name),
                    )
                })?,
            })
        }
        CobolType::IbmFloat64 => {
            reject_float_options(raw)?;
            Ok(FieldCodec::IbmFloat64 {
                endian: raw.endian.ok_or_else(|| {
                    CliError::config(
                        "E_SCHEMA",
                        format!("float field {} requires endian", raw.name),
                    )
                })?,
            })
        }
        CobolType::Alphanumeric => {
            reject_unsupported_option(raw, "total_digits", raw.total_digits.is_some())?;
            reject_unsupported_option(raw, "scale", raw.scale.is_some())?;
            reject_unsupported_option(raw, "signed", raw.signed.is_some())?;
            reject_unsupported_option(raw, "sign_mode", raw.sign_mode.is_some())?;
            reject_unsupported_option(raw, "sign_policy", raw.sign_policy.is_some())?;
            reject_unsupported_option(raw, "mode", raw.mode.is_some())?;
            reject_unsupported_option(raw, "endian", raw.endian.is_some())?;
            let byte_len = raw.length.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("alphanumeric field {} requires length", raw.name),
                )
            })?;
            if byte_len == 0 {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!(
                        "alphanumeric field {} length must be greater than zero",
                        raw.name
                    ),
                ));
            }
            let encoding = raw.encoding.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("alphanumeric field {} requires encoding", raw.name),
                )
            })?;
            let codepage = match encoding {
                FieldEncoding::Ebcdic => Some(raw.codepage.ok_or_else(|| {
                    CliError::config(
                        "E_SCHEMA",
                        format!("EBCDIC alphanumeric field {} requires codepage", raw.name),
                    )
                })?),
                FieldEncoding::Ascii => None,
                FieldEncoding::AsciiOverpunch => {
                    return Err(CliError::config(
                        "E_SCHEMA",
                        format!(
                            "alphanumeric field {} cannot use ascii-overpunch encoding",
                            raw.name
                        ),
                    ))
                }
            };
            Ok(FieldCodec::Alphanumeric {
                byte_len,
                encoding,
                codepage,
            })
        }
        CobolType::Filler => {
            reject_unsupported_option(raw, "total_digits", raw.total_digits.is_some())?;
            reject_unsupported_option(raw, "scale", raw.scale.is_some())?;
            reject_unsupported_option(raw, "signed", raw.signed.is_some())?;
            reject_unsupported_option(raw, "sign_mode", raw.sign_mode.is_some())?;
            reject_unsupported_option(raw, "sign_policy", raw.sign_policy.is_some())?;
            reject_unsupported_option(raw, "mode", raw.mode.is_some())?;
            reject_unsupported_option(raw, "encoding", raw.encoding.is_some())?;
            reject_unsupported_option(raw, "codepage", raw.codepage.is_some())?;
            reject_unsupported_option(raw, "endian", raw.endian.is_some())?;
            let byte_len = raw.length.ok_or_else(|| {
                CliError::config(
                    "E_SCHEMA",
                    format!("filler field {} requires length", raw.name),
                )
            })?;
            if byte_len == 0 {
                return Err(CliError::config(
                    "E_SCHEMA",
                    format!("filler field {} length must be greater than zero", raw.name),
                ));
            }
            Ok(FieldCodec::Bytes { byte_len })
        }
    }
}

fn reject_float_options(raw: &RawFieldSpec) -> Result<(), CliError> {
    reject_unsupported_option(raw, "total_digits", raw.total_digits.is_some())?;
    reject_unsupported_option(raw, "scale", raw.scale.is_some())?;
    reject_unsupported_option(raw, "signed", raw.signed.is_some())?;
    reject_unsupported_option(raw, "sign_mode", raw.sign_mode.is_some())?;
    reject_unsupported_option(raw, "sign_policy", raw.sign_policy.is_some())?;
    reject_unsupported_option(raw, "mode", raw.mode.is_some())?;
    reject_unsupported_option(raw, "encoding", raw.encoding.is_some())?;
    reject_unsupported_option(raw, "codepage", raw.codepage.is_some())
}

fn reject_unsupported_option(
    raw: &RawFieldSpec,
    option: &str,
    is_set: bool,
) -> Result<(), CliError> {
    if is_set {
        return Err(CliError::config(
            "E_SCHEMA",
            format!(
                "field {} of type {:?} must not set {option}",
                raw.name, raw.field_type
            ),
        ));
    }
    Ok(())
}

fn required_digits(raw: &RawFieldSpec) -> Result<u8, CliError> {
    raw.total_digits.ok_or_else(|| {
        CliError::config(
            "E_SCHEMA",
            format!("numeric field {} requires total_digits", raw.name),
        )
    })
}

fn validate_decimal_digits_and_scale(
    field_name: &str,
    total_digits: u8,
    scale: u8,
) -> Result<(), CliError> {
    if !(1..=18).contains(&total_digits) {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("field {field_name} total_digits must be in 1..=18"),
        ));
    }
    if scale > total_digits {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("field {field_name} scale exceeds total_digits"),
        ));
    }
    Ok(())
}

fn validate_declared_length(
    raw_length: Option<usize>,
    expected: usize,
    field_name: &str,
) -> Result<(), CliError> {
    if let Some(length) = raw_length {
        if length != expected {
            return Err(CliError::config(
                "E_SCHEMA",
                format!(
                    "field {field_name} length {length} does not match expected length {expected}"
                ),
            ));
        }
    }
    Ok(())
}

fn binary_width_for_digits(total_digits: u8) -> Result<usize, CliError> {
    match total_digits {
        1..=18 => Ok(cobol_record::binary_width_from_digits(usize::from(
            total_digits,
        ))),
        _ => Err(CliError::config(
            "E_SCHEMA",
            "binary total_digits must be in 1..=18",
        )),
    }
}

fn find_counter_field(
    fields: &[PlannedFieldSpec],
    counter: &str,
    group_offset: usize,
) -> Result<PlannedFieldSpec, CliError> {
    let field = fields
        .iter()
        .find(|field| field.path == counter || field.name == counter)
        .ok_or_else(|| {
            CliError::config(
                "E_SCHEMA",
                format!("OCCURS counter field {counter} must refer to a preceding scalar field"),
            )
        })?;
    let end = checked_add(field.offset, field.byte_len, "counter field")?;
    if end > group_offset {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("OCCURS counter field {counter} must end before or at the group offset for binary streaming"),
        ));
    }
    if !codec_is_integer_counter(&field.codec) {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("OCCURS counter field {counter} must be a scale-zero numeric field"),
        ));
    }
    Ok(field.clone())
}

fn codec_is_integer_counter(codec: &FieldCodec) -> bool {
    match codec {
        FieldCodec::PackedDecimal { cfg, .. } => cfg.scale() == 0,
        FieldCodec::ZonedDecimal { scale, .. }
        | FieldCodec::Binary { scale, .. }
        | FieldCodec::NativeBinary { scale, .. } => *scale == 0,
        FieldCodec::IbmFloat32 { .. }
        | FieldCodec::IbmFloat64 { .. }
        | FieldCodec::Alphanumeric { .. }
        | FieldCodec::Bytes { .. } => false,
    }
}

fn parse_selector(
    value: &serde_json::Value,
    fields: &[PlannedFieldSpec],
) -> Result<PlannedSelector, CliError> {
    let object = value.as_object().ok_or_else(|| {
        CliError::config(
            "E_SCHEMA",
            "REDEFINES selector must be an object with field and equals",
        )
    })?;
    let field_name = object
        .get("field")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| CliError::config("E_SCHEMA", "REDEFINES selector requires field"))?;
    let equals = object
        .get("equals")
        .or_else(|| object.get("value"))
        .ok_or_else(|| CliError::config("E_SCHEMA", "REDEFINES selector requires equals"))?;
    let equals = selector_value_to_text(equals)?;
    let field = fields
        .iter()
        .find(|field| field.path == field_name || field.name == field_name)
        .ok_or_else(|| {
            CliError::config(
                "E_SCHEMA",
                format!("REDEFINES selector field {field_name} must refer to a preceding field"),
            )
        })?;
    Ok(PlannedSelector {
        field: field.clone(),
        equals,
    })
}

fn selector_value_to_text(value: &serde_json::Value) -> Result<String, CliError> {
    match value {
        serde_json::Value::String(value) => Ok(value.clone()),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        serde_json::Value::Bool(value) => Ok(value.to_string()),
        _ => Err(CliError::config(
            "E_SCHEMA",
            "REDEFINES selector equals must be a string, number, or boolean",
        )),
    }
}

fn alignment_for(codec: &FieldCodec, platform: PlatformProfile) -> usize {
    if platform == PlatformProfile::GnuCobol {
        return 1;
    }
    match codec {
        FieldCodec::Binary { byte_len, .. } | FieldCodec::NativeBinary { byte_len, .. } => {
            (*byte_len).min(8)
        }
        FieldCodec::PackedDecimal { cfg, .. } => cfg.byte_len().min(8),
        FieldCodec::IbmFloat32 { .. } => 4,
        FieldCodec::IbmFloat64 { .. } => 8,
        FieldCodec::ZonedDecimal { .. }
        | FieldCodec::Alphanumeric { .. }
        | FieldCodec::Bytes { .. } => 1,
    }
}

fn sync_slack(offset: usize, align: usize) -> usize {
    if align <= 1 {
        return 0;
    }
    cobol_record::align_offset(offset, align).saturating_sub(offset)
}

fn checked_add(lhs: usize, rhs: usize, label: &str) -> Result<usize, CliError> {
    lhs.checked_add(rhs)
        .ok_or_else(|| CliError::config("E_SCHEMA", format!("{label} offset overflows")))
}

fn path_join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

fn coverage_from_ranges(
    record_length: Option<usize>,
    ranges: &[LayoutRangeSummary],
) -> SchemaCoverageSummary {
    let mut ranges = ranges.to_vec();
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
    if let Some(record_len) = record_length {
        if cursor < record_len {
            gaps.push(LayoutGapSummary {
                offset: cursor,
                end_offset: record_len,
                length: record_len - cursor,
            });
        }
    }
    let gap_bytes = record_length.map(|_| gaps.iter().map(|gap| gap.length).sum());
    let full_coverage = record_length
        .map(|record_len| gaps.is_empty() && overlaps.is_empty() && covered == record_len);
    let overlap_count = overlaps.len();
    SchemaCoverageSummary {
        record_length,
        covered_bytes: covered,
        gap_bytes,
        full_coverage,
        overlap_count,
        first_offset: first,
        last_end_offset: last,
        ranges,
        gaps,
        overlaps,
    }
}

fn process_binary_records_v2(
    schema: &Schema,
    plan: &PlannedSchema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    audit: &mut super::AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    if plan.has_occurs {
        return process_dynamic_binary_records_v2(schema, plan, input, verify, limits, audit, sink);
    }
    let record_len = plan.record_length.unwrap_or(plan.max_record_len);
    let mut reader = BufReader::new(fs::File::open(input)?);
    let mut record = vec![0u8; record_len];
    let mut idx = 0usize;
    loop {
        if super::record_limit_reached(idx, limits) {
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
        process_record_v2(schema, plan, idx, &record, verify, audit, sink)?;
        record.fill(0);
        idx += 1;
    }
    Ok(())
}

fn process_dynamic_binary_records_v2(
    schema: &Schema,
    plan: &PlannedSchema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    audit: &mut super::AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let mut reader = BufReader::new(fs::File::open(input)?);
    let mut idx = 0usize;
    loop {
        if super::record_limit_reached(idx, limits) {
            break;
        }
        let mut header = vec![0u8; plan.odo_header_len];
        let mut read = 0usize;
        while read < header.len() {
            let n = reader.read(&mut header[read..])?;
            if n == 0 {
                break;
            }
            read += n;
        }
        if read == 0 {
            break;
        }
        if read != header.len() {
            let err = DecodedField::error(
                Some(idx),
                "<record>",
                Some(read),
                &header[..read],
                "E_RECORD_LENGTH",
                format!(
                    "truncated record header: expected {} bytes, got {read}",
                    header.len()
                ),
            );
            handle_record_error(schema, audit, sink, err)?;
            break;
        }
        let actual_len = match actual_record_len(plan, &header, idx) {
            Ok(len) => len,
            Err(err) => {
                handle_record_error(schema, audit, sink, *err)?;
                idx += 1;
                continue;
            }
        };
        if actual_len > MAX_RECORD_BYTES {
            let err = DecodedField::error(
                Some(idx),
                "<record>",
                Some(header.len()),
                &header,
                "E_RECORD_LENGTH",
                format!("computed ODO record length exceeds {MAX_RECORD_BYTES} bytes"),
            );
            handle_record_error(schema, audit, sink, err)?;
            idx += 1;
            continue;
        }
        let mut record = header;
        record.resize(actual_len, 0);
        let mut body_read = plan.odo_header_len;
        while body_read < actual_len {
            let n = reader.read(&mut record[body_read..])?;
            if n == 0 {
                break;
            }
            body_read += n;
        }
        if body_read != actual_len {
            let err = DecodedField::error(
                Some(idx),
                "<record>",
                Some(body_read),
                &record[..body_read],
                "E_RECORD_LENGTH",
                format!("truncated ODO record: expected {actual_len} bytes, got {body_read}"),
            );
            handle_record_error(schema, audit, sink, err)?;
            break;
        }
        process_record_v2(schema, plan, idx, &record, verify, audit, sink)?;
        idx += 1;
    }
    Ok(())
}

fn process_hex_records_v2(
    schema: &Schema,
    plan: &PlannedSchema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    audit: &mut super::AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let max_record_len = plan.record_length.unwrap_or(plan.max_record_len);
    let mut reader = BufReader::new(fs::File::open(input)?);
    let mut line = String::new();
    let mut idx = 0usize;
    while super::read_bounded_line(&mut reader, &mut line, super::MAX_LINE_BYTES)? != 0 {
        if line.trim().is_empty() {
            continue;
        }
        if super::record_limit_reached(idx, limits) {
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
        process_record_v2(schema, plan, idx, &record, verify, audit, sink)?;
        idx += 1;
    }
    Ok(())
}

fn process_record_v2(
    schema: &Schema,
    plan: &PlannedSchema,
    idx: usize,
    record: &[u8],
    verify: bool,
    audit: &mut super::AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    if !plan.has_occurs {
        let expected = plan.record_length.unwrap_or(plan.max_record_len);
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
    } else {
        match actual_record_len(plan, record, idx) {
            Ok(expected) if record.len() == expected => {}
            Ok(expected) => {
                let err = DecodedField::error(
                    Some(idx),
                    "<record>",
                    Some(record.len()),
                    record,
                    "E_RECORD_LENGTH",
                    format!(
                        "ODO record length mismatch: expected {expected} bytes, got {}",
                        record.len()
                    ),
                );
                handle_record_error(schema, audit, sink, err)?;
                return Ok(());
            }
            Err(err) => {
                handle_record_error(schema, audit, sink, *err)?;
                return Ok(());
            }
        }
    }
    let rows = decode_rows_v2(plan, idx, record, verify);
    emit_record_rows(schema, audit, sink, rows)
}

fn emit_record_rows(
    schema: &Schema,
    audit: &mut super::AuditReport,
    sink: &mut RowSink,
    rows: Vec<DecodedField>,
) -> Result<(), CliError> {
    if schema.on_error != OnError::EmitErrorRow {
        audit.records_seen += 1;
        let mut pending = Vec::new();
        for row in rows {
            if !row.valid {
                let code = row.error_code.unwrap_or("E_DATA");
                let message = row
                    .message
                    .clone()
                    .unwrap_or_else(|| "data error".to_string());
                super::record_audit(audit, &row);
                audit.records_invalid += 1;
                return match schema.on_error {
                    OnError::Fail => Err(CliError::data(code, message)),
                    OnError::SkipRecord => Ok(()),
                    OnError::EmitErrorRow => Err(CliError::internal(
                        "emit-error-row reached v2 atomic processor",
                    )),
                };
            }
            pending.push(row);
        }
        for row in pending {
            super::record_audit(audit, &row);
            sink.emit(row)?;
        }
        audit.records_valid += 1;
        return Ok(());
    }

    audit.records_seen += 1;
    let before_invalid = audit.fields_invalid;
    for row in rows {
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

fn decode_rows_v2(
    plan: &PlannedSchema,
    idx: usize,
    record: &[u8],
    verify: bool,
) -> Vec<DecodedField> {
    let mut rows = Vec::new();
    decode_items(&plan.items, idx, record, 0, verify, &mut rows);
    rows
}

fn decode_items(
    items: &[PlannedLayoutItem],
    idx: usize,
    record: &[u8],
    base_offset: usize,
    verify: bool,
    rows: &mut Vec<DecodedField>,
) {
    for item in items {
        match item {
            PlannedLayoutItem::Field(field) => {
                rows.push(decode_field_v2(
                    idx,
                    record,
                    base_offset,
                    field,
                    verify,
                    None,
                ));
            }
            PlannedLayoutItem::Filler { .. } | PlannedLayoutItem::SyncSlack { .. } => {}
            PlannedLayoutItem::OccursGroup(group) => {
                let count = match decode_occurs_count(group, record, idx) {
                    Ok(count) => count,
                    Err(err) => {
                        rows.push(*err);
                        continue;
                    }
                };
                for occurrence in 0..count {
                    let Some(element_delta) = occurrence.checked_mul(group.element_byte_len) else {
                        rows.push(DecodedField::error(
                            Some(idx),
                            &group.name,
                            Some(group.offset),
                            &[],
                            "E_OCCURS_COUNT",
                            "OCCURS element offset overflow",
                        ));
                        break;
                    };
                    let Some(element_base) = group.offset.checked_add(element_delta) else {
                        rows.push(DecodedField::error(
                            Some(idx),
                            &group.name,
                            Some(group.offset),
                            &[],
                            "E_OCCURS_COUNT",
                            "OCCURS element offset overflow",
                        ));
                        break;
                    };
                    let before = rows.len();
                    decode_items(
                        &group.element_items,
                        idx,
                        record,
                        element_base,
                        verify,
                        rows,
                    );
                    for row in &mut rows[before..] {
                        if row.field.starts_with(&group.name) {
                            row.field = row.field.replacen(
                                &group.name,
                                &format!("{}[{occurrence}]", group.name),
                                1,
                            );
                        }
                    }
                }
            }
            PlannedLayoutItem::RedefinesGroup(group) => {
                let Some(end) = group.offset.checked_add(group.base_length) else {
                    rows.push(DecodedField::error(
                        Some(idx),
                        &group.name,
                        Some(group.offset),
                        &[],
                        "E_OFFSET",
                        "REDEFINES base offset overflow",
                    ));
                    continue;
                };
                if end > record.len() {
                    rows.push(DecodedField::error(
                        Some(idx),
                        &group.name,
                        Some(group.offset),
                        &[],
                        "E_OFFSET",
                        "REDEFINES base extends past record boundary",
                    ));
                    continue;
                }
                for variant in &group.variants {
                    let selector_status =
                        match evaluate_selector(variant.selector.as_ref(), idx, record) {
                            Ok(status) => status,
                            Err(err) => {
                                rows.push(*err);
                                continue;
                            }
                        };
                    let before = rows.len();
                    decode_items(&variant.items, idx, record, group.offset, verify, rows);
                    if let Some(status) = selector_status {
                        for row in &mut rows[before..] {
                            row.field = format!("{}:{}", row.field, status);
                        }
                    }
                }
            }
        }
    }
}

fn evaluate_selector(
    selector: Option<&PlannedSelector>,
    idx: usize,
    record: &[u8],
) -> Result<Option<&'static str>, Box<DecodedField>> {
    let Some(selector) = selector else {
        return Ok(None);
    };
    let row = decode_field_v2(idx, record, 0, &selector.field, false, None);
    if !row.valid {
        return Err(Box::new(DecodedField::error(
            Some(idx),
            &selector.field.path,
            Some(selector.field.offset),
            &[],
            "E_SELECTOR",
            format!(
                "failed to decode REDEFINES selector field {}: {}",
                selector.field.path,
                row.message.unwrap_or_else(|| "data error".to_string())
            ),
        )));
    }
    let actual = row.value.unwrap_or_default();
    if actual == selector.equals {
        Ok(Some("selector-active"))
    } else {
        Ok(Some("selector-inactive"))
    }
}

fn decode_field_v2(
    idx: usize,
    record: &[u8],
    base_offset: usize,
    field: &PlannedFieldSpec,
    verify: bool,
    display_name: Option<String>,
) -> DecodedField {
    let field_name = display_name.unwrap_or_else(|| field.path.clone());
    let Some(offset) = base_offset.checked_add(field.offset) else {
        return DecodedField::error(
            Some(idx),
            &field_name,
            Some(base_offset),
            &[],
            "E_OFFSET",
            "field offset overflow",
        );
    };
    let Some(end) = offset.checked_add(field.byte_len) else {
        return DecodedField::error(
            Some(idx),
            &field_name,
            Some(offset),
            &[],
            "E_OFFSET",
            "field end offset overflow",
        );
    };
    if end > record.len() {
        return DecodedField::error(
            Some(idx),
            &field_name,
            Some(offset),
            &[],
            "E_OFFSET",
            "field extends past record boundary",
        );
    }
    let bytes = &record[offset..end];
    let decoded = match field.codec.decode(bytes) {
        Ok(decoded) => decoded,
        Err(err) => {
            return DecodedField::error(
                Some(idx),
                &field_name,
                Some(offset),
                bytes,
                err.code,
                err.message,
            )
        }
    };
    let verified = if verify {
        field.codec.verify(bytes, &decoded.value).unwrap_or(false)
    } else {
        true
    };
    let (raw_hex, raw_hex_truncated) = raw_hex_for_output(bytes);
    DecodedField {
        version: OUTPUT_VERSION,
        record_index: Some(idx),
        field: field_name,
        offset: Some(offset),
        raw_hex,
        raw_byte_len: bytes.len(),
        raw_hex_truncated,
        value: Some(decoded.value.to_text()),
        sign_nibble: decoded.sign_nibble,
        sign_class: decoded.sign_class,
        valid: verified,
        error_code: if verified { None } else { Some("E_VERIFY") },
        message: if verified {
            None
        } else {
            Some("re-encode did not match original bytes".to_string())
        },
        recoverable: !verified,
    }
}

fn decode_occurs_count(
    group: &PlannedOccursGroup,
    record: &[u8],
    idx: usize,
) -> Result<usize, Box<DecodedField>> {
    let row = decode_field_v2(idx, record, 0, &group.counter_field, false, None);
    if !row.valid {
        return Err(Box::new(DecodedField::error(
            Some(idx),
            &group.name,
            Some(group.offset),
            &[],
            "E_OCCURS_COUNT",
            format!(
                "failed to decode OCCURS counter {}: {}",
                group.counter_field.path,
                row.message.unwrap_or_else(|| "data error".to_string())
            ),
        )));
    }
    let count = row
        .value
        .as_deref()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            Box::new(DecodedField::error(
                Some(idx),
                &group.name,
                Some(group.offset),
                &[],
                "E_OCCURS_COUNT",
                "OCCURS counter is not a non-negative integer",
            ))
        })?;
    if count < group.min_occurs || count > group.max_occurs {
        return Err(Box::new(DecodedField::error(
            Some(idx),
            &group.name,
            Some(group.offset),
            &[],
            "E_OCCURS_COUNT",
            format!(
                "OCCURS counter {count} is outside {}..={}",
                group.min_occurs, group.max_occurs
            ),
        )));
    }
    Ok(count)
}

fn actual_record_len(
    plan: &PlannedSchema,
    header: &[u8],
    idx: usize,
) -> Result<usize, Box<DecodedField>> {
    let mut actual = 0usize;
    for item in &plan.items {
        match item {
            PlannedLayoutItem::OccursGroup(group) => {
                let count = decode_occurs_count(group, header, idx)?;
                let group_len = count.checked_mul(group.element_byte_len).ok_or_else(|| {
                    Box::new(DecodedField::error(
                        Some(idx),
                        &group.name,
                        Some(group.offset),
                        &[],
                        "E_OCCURS_COUNT",
                        "OCCURS record length overflow",
                    ))
                })?;
                actual = actual.max(group.offset.saturating_add(group_len));
            }
            _ => actual = actual.max(item_end_offset(item)),
        }
    }
    Ok(actual.max(plan.odo_header_len))
}

fn item_end_offset(item: &PlannedLayoutItem) -> usize {
    match item {
        PlannedLayoutItem::Field(field) => field.offset.saturating_add(field.byte_len),
        PlannedLayoutItem::Filler { offset, length, .. }
        | PlannedLayoutItem::SyncSlack { offset, length } => offset.saturating_add(*length),
        PlannedLayoutItem::OccursGroup(group) => group
            .offset
            .saturating_add(group.max_occurs.saturating_mul(group.element_byte_len)),
        PlannedLayoutItem::RedefinesGroup(group) => group.offset.saturating_add(group.base_length),
    }
}

impl FieldCodec {
    fn byte_len(&self) -> usize {
        match self {
            FieldCodec::PackedDecimal { cfg, .. } => cfg.byte_len(),
            FieldCodec::ZonedDecimal { total_digits, .. } => *total_digits as usize,
            FieldCodec::Binary { byte_len, .. } | FieldCodec::NativeBinary { byte_len, .. } => {
                *byte_len
            }
            FieldCodec::IbmFloat32 { .. } => 4,
            FieldCodec::IbmFloat64 { .. } => 8,
            FieldCodec::Alphanumeric { byte_len, .. } | FieldCodec::Bytes { byte_len } => *byte_len,
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            FieldCodec::PackedDecimal { .. } => "packed-decimal",
            FieldCodec::ZonedDecimal { .. } => "zoned-decimal",
            FieldCodec::Binary { .. } => "binary",
            FieldCodec::NativeBinary { .. } => "native-binary",
            FieldCodec::IbmFloat32 { .. } => "ibm-float32",
            FieldCodec::IbmFloat64 { .. } => "ibm-float64",
            FieldCodec::Alphanumeric { .. } => "alphanumeric",
            FieldCodec::Bytes { .. } => "filler",
        }
    }

    fn semantic_json(&self) -> serde_json::Value {
        match self {
            FieldCodec::PackedDecimal {
                cfg,
                sign_mode,
                mode,
            } => serde_json::json!({
                "field_type": self.type_name(),
                "total_digits": cfg.total_digits(),
                "scale": cfg.scale(),
                "signed": cfg.is_signed(),
                "sign_mode": sign_mode,
                "mode": mode,
            }),
            FieldCodec::ZonedDecimal {
                total_digits,
                scale,
                signed,
                encoding,
                codepage,
                sign_policy,
            } => serde_json::json!({
                "field_type": self.type_name(),
                "total_digits": total_digits,
                "scale": scale,
                "signed": signed,
                "encoding": encoding,
                "codepage": codepage,
                "sign_policy": sign_policy,
            }),
            FieldCodec::Binary {
                byte_len,
                endian,
                signed,
                scale,
            }
            | FieldCodec::NativeBinary {
                byte_len,
                endian,
                signed,
                scale,
            } => serde_json::json!({
                "field_type": self.type_name(),
                "byte_len": byte_len,
                "endian": endian,
                "signed": signed,
                "scale": scale,
            }),
            FieldCodec::IbmFloat32 { endian } | FieldCodec::IbmFloat64 { endian } => {
                serde_json::json!({
                    "field_type": self.type_name(),
                    "endian": endian,
                })
            }
            FieldCodec::Alphanumeric {
                byte_len,
                encoding,
                codepage,
            } => serde_json::json!({
                "field_type": self.type_name(),
                "byte_len": byte_len,
                "encoding": encoding,
                "codepage": codepage,
            }),
            FieldCodec::Bytes { byte_len } => serde_json::json!({
                "field_type": self.type_name(),
                "byte_len": byte_len,
            }),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<CodecOutput, DecodeFailure> {
        if bytes.len() != self.byte_len() {
            return Err(DecodeFailure {
                code: "E_LENGTH",
                message: format!("expected {} bytes, got {}", self.byte_len(), bytes.len()),
            });
        }
        match self {
            FieldCodec::PackedDecimal {
                cfg,
                sign_mode,
                mode,
            } => {
                let nibble = bytes.last().map(|b| b & 0x0F);
                let sign_class = nibble.map(|n| classify_sign(n, cfg.is_signed()).to_string());
                let result = match mode {
                    FieldMode::Canonical => from_packed(bytes, cfg, sign_mode.to_core()),
                    FieldMode::Lossless => {
                        from_packed_lossless(bytes, cfg, sign_mode.to_core()).map(|loss| loss.value)
                    }
                };
                result
                    .map(|value| CodecOutput {
                        value: DecodedValue::Decimal(value),
                        sign_nibble: nibble.map(|n| format!("0x{n:X}")),
                        sign_class,
                    })
                    .map_err(packed_decode_failure)
            }
            FieldCodec::ZonedDecimal {
                scale,
                signed,
                encoding,
                codepage: _,
                sign_policy,
                ..
            } => decode_zoned_decimal(bytes, *scale, *signed, *encoding, *sign_policy),
            FieldCodec::Binary {
                endian,
                signed,
                scale,
                ..
            }
            | FieldCodec::NativeBinary {
                endian,
                signed,
                scale,
                ..
            } => decode_binary(bytes, *endian, *signed, *scale),
            FieldCodec::IbmFloat32 { endian } => decode_ibm_float32(bytes, *endian),
            FieldCodec::IbmFloat64 { endian } => decode_ibm_float64(bytes, *endian),
            FieldCodec::Alphanumeric {
                encoding, codepage, ..
            } => decode_alphanumeric(bytes, *encoding, *codepage),
            FieldCodec::Bytes { .. } => Ok(CodecOutput {
                value: if bytes.is_empty() {
                    DecodedValue::Null
                } else {
                    DecodedValue::Bytes(bytes.to_vec())
                },
                sign_nibble: None,
                sign_class: None,
            }),
        }
    }

    fn verify(&self, bytes: &[u8], value: &DecodedValue) -> Result<bool, DecodeFailure> {
        match self {
            FieldCodec::PackedDecimal { cfg, sign_mode, .. } => {
                let loss = from_packed_lossless(bytes, cfg, sign_mode.to_core())
                    .map_err(packed_decode_failure)?;
                let rebuilt = to_packed_lossless(&loss, cfg).map_err(packed_decode_failure)?;
                Ok(rebuilt == bytes)
            }
            FieldCodec::Binary { .. }
            | FieldCodec::NativeBinary { .. }
            | FieldCodec::ZonedDecimal { .. }
            | FieldCodec::IbmFloat32 { .. }
            | FieldCodec::IbmFloat64 { .. }
            | FieldCodec::Alphanumeric { .. }
            | FieldCodec::Bytes { .. } => {
                let mut out = vec![0u8; self.byte_len()];
                self.encode(value, &mut out)?;
                Ok(out == bytes)
            }
        }
    }

    fn encode(&self, value: &DecodedValue, out: &mut [u8]) -> Result<(), DecodeFailure> {
        if out.len() != self.byte_len() {
            return Err(DecodeFailure {
                code: "E_LENGTH",
                message: format!("expected output length {}", self.byte_len()),
            });
        }
        match (self, value) {
            (
                FieldCodec::Binary {
                    endian,
                    signed,
                    scale,
                    ..
                }
                | FieldCodec::NativeBinary {
                    endian,
                    signed,
                    scale,
                    ..
                },
                value,
            ) => encode_binary(value, out, *endian, *signed, *scale),
            (
                FieldCodec::Alphanumeric {
                    encoding, codepage, ..
                },
                DecodedValue::Text(text),
            ) => encode_alphanumeric(text, out, *encoding, *codepage),
            (FieldCodec::ZonedDecimal { .. }, DecodedValue::DecimalWithRaw { raw, .. })
            | (FieldCodec::IbmFloat32 { .. }, DecodedValue::FloatWithRaw { raw, .. })
            | (FieldCodec::IbmFloat64 { .. }, DecodedValue::FloatWithRaw { raw, .. })
                if raw.len() == out.len() =>
            {
                out.copy_from_slice(raw);
                Ok(())
            }
            (FieldCodec::Bytes { .. }, DecodedValue::Bytes(bytes)) if bytes.len() == out.len() => {
                out.copy_from_slice(bytes);
                Ok(())
            }
            _ => Err(DecodeFailure {
                code: "E_VERIFY",
                message: "codec does not support byte-for-byte re-encode for this value"
                    .to_string(),
            }),
        }
    }
}

impl DecodedValue {
    fn to_text(&self) -> String {
        match self {
            DecodedValue::Decimal(value) => value.to_string(),
            DecodedValue::DecimalWithRaw { value, .. } => value.to_string(),
            DecodedValue::Integer(value) => value.to_string(),
            DecodedValue::UnsignedInteger(value) => value.to_string(),
            DecodedValue::FloatWithRaw { value, .. } => {
                if value.fract() == 0.0 {
                    format!("{value:.1}")
                } else {
                    value.to_string()
                }
            }
            DecodedValue::Text(value) => value.clone(),
            DecodedValue::Bytes(value) => to_hex(value),
            DecodedValue::Null => String::new(),
        }
    }
}

fn packed_decode_failure(err: PackedError) -> DecodeFailure {
    DecodeFailure {
        code: super::packed_error_code(&err),
        message: err.to_string(),
    }
}

fn decode_zoned_decimal(
    bytes: &[u8],
    scale: u8,
    signed: bool,
    encoding: FieldEncoding,
    sign_policy: SignPolicy,
) -> Result<CodecOutput, DecodeFailure> {
    if bytes.is_empty() {
        return Err(DecodeFailure {
            code: "E_LENGTH",
            message: "zoned decimal field is empty".to_string(),
        });
    }
    match encoding {
        FieldEncoding::Ebcdic => decode_ebcdic_zoned(bytes, scale, signed, sign_policy),
        FieldEncoding::AsciiOverpunch => decode_ascii_overpunch(bytes, scale, signed),
        FieldEncoding::Ascii => Err(DecodeFailure {
            code: "E_SCHEMA",
            message: "zoned decimal requires ebcdic or ascii-overpunch encoding".to_string(),
        }),
    }
}

fn decode_ebcdic_zoned(
    bytes: &[u8],
    scale: u8,
    signed: bool,
    sign_policy: SignPolicy,
) -> Result<CodecOutput, DecodeFailure> {
    let mut mantissa = 0i128;
    for &byte in &bytes[..bytes.len() - 1] {
        let zone = byte >> 4;
        let digit = byte & 0x0F;
        if zone != 0xF || digit > 9 {
            return Err(DecodeFailure {
                code: "E_DIGIT",
                message: format!("invalid EBCDIC zoned digit byte 0x{byte:02X}"),
            });
        }
        push_decimal_digit(&mut mantissa, digit)?;
    }
    let last = bytes[bytes.len() - 1];
    let zone = last >> 4;
    let digit = last & 0x0F;
    if digit > 9 {
        return Err(DecodeFailure {
            code: "E_DIGIT",
            message: format!("invalid zoned digit nibble 0x{digit:X}"),
        });
    }
    let sign = classify_zoned_zone(zone, digit, signed, sign_policy)?;
    push_decimal_digit(&mut mantissa, digit)?;
    let signed_mantissa = if sign.is_negative {
        mantissa.checked_neg().ok_or_else(|| DecodeFailure {
            code: "E_OVERFLOW",
            message: "zoned decimal mantissa overflow".to_string(),
        })?
    } else {
        mantissa
    };
    Ok(CodecOutput {
        value: DecodedValue::DecimalWithRaw {
            value: Decimal::from_i128_with_scale(signed_mantissa, u32::from(scale)),
            raw: bytes.to_vec(),
        },
        sign_nibble: Some(format!("0x{zone:X}")),
        sign_class: Some(sign.class.to_string()),
    })
}

fn decode_ascii_overpunch(
    bytes: &[u8],
    scale: u8,
    signed: bool,
) -> Result<CodecOutput, DecodeFailure> {
    let mut mantissa = 0i128;
    for &byte in &bytes[..bytes.len() - 1] {
        if !byte.is_ascii_digit() {
            return Err(DecodeFailure {
                code: "E_DIGIT",
                message: format!("invalid ASCII digit byte 0x{byte:02X}"),
            });
        }
        push_decimal_digit(&mut mantissa, byte - b'0')?;
    }
    let last = bytes[bytes.len() - 1];
    let (digit, is_negative, class) = ascii_overpunch_digit(last, signed)?;
    push_decimal_digit(&mut mantissa, digit)?;
    let signed_mantissa = if is_negative {
        mantissa.checked_neg().ok_or_else(|| DecodeFailure {
            code: "E_OVERFLOW",
            message: "zoned decimal mantissa overflow".to_string(),
        })?
    } else {
        mantissa
    };
    Ok(CodecOutput {
        value: DecodedValue::DecimalWithRaw {
            value: Decimal::from_i128_with_scale(signed_mantissa, u32::from(scale)),
            raw: bytes.to_vec(),
        },
        sign_nibble: Some(format!("0x{last:02X}")),
        sign_class: Some(class.to_string()),
    })
}

fn push_decimal_digit(mantissa: &mut i128, digit: u8) -> Result<(), DecodeFailure> {
    *mantissa = mantissa
        .checked_mul(10)
        .and_then(|value| value.checked_add(i128::from(digit)))
        .ok_or_else(|| DecodeFailure {
            code: "E_OVERFLOW",
            message: "decimal mantissa overflow".to_string(),
        })?;
    Ok(())
}

struct ZonedSign {
    is_negative: bool,
    class: &'static str,
}

fn classify_zoned_zone(
    zone: u8,
    digit: u8,
    signed: bool,
    sign_policy: SignPolicy,
) -> Result<ZonedSign, DecodeFailure> {
    let sign = match sign_policy {
        SignPolicy::Preferred => match zone {
            0xC | 0xF => ZonedSign {
                is_negative: false,
                class: "positive",
            },
            0xD if signed => ZonedSign {
                is_negative: true,
                class: "negative",
            },
            0xD => {
                return Err(DecodeFailure {
                    code: "E_SIGN",
                    message: "negative zoned sign in unsigned field".to_string(),
                })
            }
            _ => {
                return Err(DecodeFailure {
                    code: "E_SIGN",
                    message: format!("invalid preferred zoned sign zone 0x{zone:X}"),
                })
            }
        },
        SignPolicy::NonPreferred => match zone {
            0xA | 0xC | 0xE | 0xF => ZonedSign {
                is_negative: false,
                class: "positive",
            },
            0xB | 0xD if signed => ZonedSign {
                is_negative: true,
                class: "negative",
            },
            0xB | 0xD => {
                return Err(DecodeFailure {
                    code: "E_SIGN",
                    message: "negative zoned sign in unsigned field".to_string(),
                })
            }
            _ => {
                return Err(DecodeFailure {
                    code: "E_SIGN",
                    message: format!("invalid non-preferred zoned sign zone 0x{zone:X}"),
                })
            }
        },
        SignPolicy::Permissive {
            blank_as_positive,
            zero_nibble_as_positive,
        } => match zone {
            0xA | 0xC | 0xE | 0xF => ZonedSign {
                is_negative: false,
                class: "positive",
            },
            0xB | 0xD if signed => ZonedSign {
                is_negative: true,
                class: "negative",
            },
            0xB | 0xD => {
                return Err(DecodeFailure {
                    code: "E_SIGN",
                    message: "negative zoned sign in unsigned field".to_string(),
                })
            }
            0x0 if zero_nibble_as_positive => ZonedSign {
                is_negative: false,
                class: "repaired-zero-nibble-positive",
            },
            0x4 if blank_as_positive && digit == 0 => ZonedSign {
                is_negative: false,
                class: "repaired-blank-positive",
            },
            _ => {
                return Err(DecodeFailure {
                    code: "E_SIGN",
                    message: format!("invalid permissive zoned sign zone 0x{zone:X}"),
                })
            }
        },
    };
    Ok(sign)
}

fn ascii_overpunch_digit(
    byte: u8,
    signed: bool,
) -> Result<(u8, bool, &'static str), DecodeFailure> {
    match byte {
        b'0'..=b'9' => Ok((byte - b'0', false, "unsigned-positive")),
        b'{' => Ok((0, false, "positive")),
        b'A'..=b'I' => Ok((byte - b'A' + 1, false, "positive")),
        b'}' if signed => Ok((0, true, "negative")),
        b'J'..=b'R' if signed => Ok((byte - b'J' + 1, true, "negative")),
        b'}' | b'J'..=b'R' => Err(DecodeFailure {
            code: "E_SIGN",
            message: "negative ASCII overpunch sign in unsigned field".to_string(),
        }),
        _ => Err(DecodeFailure {
            code: "E_SIGN",
            message: format!("invalid ASCII overpunch byte 0x{byte:02X}"),
        }),
    }
}

fn decode_binary(
    bytes: &[u8],
    endian: Endian,
    signed: bool,
    scale: u8,
) -> Result<CodecOutput, DecodeFailure> {
    let value = match (bytes.len(), signed) {
        (2, true) => DecodedValue::Integer(i64::from(read_i16(bytes, endian))),
        (2, false) => DecodedValue::UnsignedInteger(u64::from(read_u16(bytes, endian))),
        (4, true) => DecodedValue::Integer(i64::from(read_i32(bytes, endian))),
        (4, false) => DecodedValue::UnsignedInteger(u64::from(read_u32(bytes, endian))),
        (8, true) => DecodedValue::Integer(read_i64(bytes, endian)),
        (8, false) => DecodedValue::UnsignedInteger(read_u64(bytes, endian)),
        _ => {
            return Err(DecodeFailure {
                code: "E_LENGTH",
                message: "binary field length must be 2, 4, or 8".to_string(),
            })
        }
    };
    let value = if scale == 0 {
        value
    } else {
        match value {
            DecodedValue::Integer(v) => DecodedValue::Decimal(Decimal::from_i128_with_scale(
                i128::from(v),
                u32::from(scale),
            )),
            DecodedValue::UnsignedInteger(v) => DecodedValue::Decimal(
                Decimal::from_i128_with_scale(i128::from(v), u32::from(scale)),
            ),
            other => other,
        }
    };
    Ok(CodecOutput {
        value,
        sign_nibble: None,
        sign_class: None,
    })
}

fn read_u16(bytes: &[u8], endian: Endian) -> u16 {
    let arr = [bytes[0], bytes[1]];
    match endian {
        Endian::Big => u16::from_be_bytes(arr),
        Endian::Little => u16::from_le_bytes(arr),
    }
}

fn read_i16(bytes: &[u8], endian: Endian) -> i16 {
    let arr = [bytes[0], bytes[1]];
    match endian {
        Endian::Big => i16::from_be_bytes(arr),
        Endian::Little => i16::from_le_bytes(arr),
    }
}

fn read_u32(bytes: &[u8], endian: Endian) -> u32 {
    let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
    match endian {
        Endian::Big => u32::from_be_bytes(arr),
        Endian::Little => u32::from_le_bytes(arr),
    }
}

fn read_i32(bytes: &[u8], endian: Endian) -> i32 {
    let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
    match endian {
        Endian::Big => i32::from_be_bytes(arr),
        Endian::Little => i32::from_le_bytes(arr),
    }
}

fn read_u64(bytes: &[u8], endian: Endian) -> u64 {
    let arr = [
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ];
    match endian {
        Endian::Big => u64::from_be_bytes(arr),
        Endian::Little => u64::from_le_bytes(arr),
    }
}

fn read_i64(bytes: &[u8], endian: Endian) -> i64 {
    let arr = [
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ];
    match endian {
        Endian::Big => i64::from_be_bytes(arr),
        Endian::Little => i64::from_le_bytes(arr),
    }
}

fn encode_binary(
    value: &DecodedValue,
    out: &mut [u8],
    endian: Endian,
    signed: bool,
    scale: u8,
) -> Result<(), DecodeFailure> {
    let signed_value;
    let unsigned_value;
    let normalized = match (signed, value) {
        (true, DecodedValue::Integer(v)) if scale == 0 => Some(EitherInteger::Signed(*v)),
        (false, DecodedValue::UnsignedInteger(v)) if scale == 0 => {
            Some(EitherInteger::Unsigned(*v))
        }
        (true, DecodedValue::Decimal(value)) => {
            signed_value = decimal_to_i128_mantissa(value, scale).and_then(|mantissa| {
                i64::try_from(mantissa).map_err(|_| DecodeFailure {
                    code: "E_OVERFLOW",
                    message: "scaled signed binary mantissa overflows i64".to_string(),
                })
            })?;
            Some(EitherInteger::Signed(signed_value))
        }
        (false, DecodedValue::Decimal(value)) => {
            unsigned_value = decimal_to_i128_mantissa(value, scale).and_then(|mantissa| {
                u64::try_from(mantissa).map_err(|_| DecodeFailure {
                    code: "E_OVERFLOW",
                    message: "scaled unsigned binary mantissa is negative or overflows u64"
                        .to_string(),
                })
            })?;
            Some(EitherInteger::Unsigned(unsigned_value))
        }
        _ => None,
    }
    .ok_or_else(|| DecodeFailure {
        code: "E_VERIFY",
        message: "binary value kind does not match field signedness".to_string(),
    })?;

    match (out.len(), normalized) {
        (2, EitherInteger::Signed(v)) => {
            let narrowed = i16::try_from(v).map_err(|_| DecodeFailure {
                code: "E_OVERFLOW",
                message: "i16 overflow".to_string(),
            })?;
            out.copy_from_slice(&match endian {
                Endian::Big => narrowed.to_be_bytes(),
                Endian::Little => narrowed.to_le_bytes(),
            });
        }
        (2, EitherInteger::Unsigned(v)) => {
            let narrowed = u16::try_from(v).map_err(|_| DecodeFailure {
                code: "E_OVERFLOW",
                message: "u16 overflow".to_string(),
            })?;
            out.copy_from_slice(&match endian {
                Endian::Big => narrowed.to_be_bytes(),
                Endian::Little => narrowed.to_le_bytes(),
            });
        }
        (4, EitherInteger::Signed(v)) => {
            let narrowed = i32::try_from(v).map_err(|_| DecodeFailure {
                code: "E_OVERFLOW",
                message: "i32 overflow".to_string(),
            })?;
            out.copy_from_slice(&match endian {
                Endian::Big => narrowed.to_be_bytes(),
                Endian::Little => narrowed.to_le_bytes(),
            });
        }
        (4, EitherInteger::Unsigned(v)) => {
            let narrowed = u32::try_from(v).map_err(|_| DecodeFailure {
                code: "E_OVERFLOW",
                message: "u32 overflow".to_string(),
            })?;
            out.copy_from_slice(&match endian {
                Endian::Big => narrowed.to_be_bytes(),
                Endian::Little => narrowed.to_le_bytes(),
            });
        }
        (8, EitherInteger::Signed(v)) => {
            out.copy_from_slice(&match endian {
                Endian::Big => v.to_be_bytes(),
                Endian::Little => v.to_le_bytes(),
            });
        }
        (8, EitherInteger::Unsigned(v)) => {
            out.copy_from_slice(&match endian {
                Endian::Big => v.to_be_bytes(),
                Endian::Little => v.to_le_bytes(),
            });
        }
        _ => {
            return Err(DecodeFailure {
                code: "E_VERIFY",
                message: "binary value kind does not match field signedness".to_string(),
            })
        }
    }
    Ok(())
}

enum EitherInteger {
    Signed(i64),
    Unsigned(u64),
}

fn decimal_to_i128_mantissa(value: &Decimal, target_scale: u8) -> Result<i128, DecodeFailure> {
    let current_scale = value.scale();
    let mut mantissa = value.mantissa();
    match current_scale.cmp(&u32::from(target_scale)) {
        std::cmp::Ordering::Equal => Ok(mantissa),
        std::cmp::Ordering::Less => {
            let delta = u32::from(target_scale) - current_scale;
            let factor = pow10_i128(delta)?;
            mantissa.checked_mul(factor).ok_or_else(|| DecodeFailure {
                code: "E_OVERFLOW",
                message: "scaled binary mantissa overflow".to_string(),
            })
        }
        std::cmp::Ordering::Greater => {
            let delta = current_scale - u32::from(target_scale);
            let factor = pow10_i128(delta)?;
            if mantissa % factor != 0 {
                return Err(DecodeFailure {
                    code: "E_VERIFY",
                    message: "scaled binary decimal cannot be re-encoded without precision loss"
                        .to_string(),
                });
            }
            mantissa /= factor;
            Ok(mantissa)
        }
    }
}

fn pow10_i128(exp: u32) -> Result<i128, DecodeFailure> {
    let mut value = 1i128;
    for _ in 0..exp {
        value = value.checked_mul(10).ok_or_else(|| DecodeFailure {
            code: "E_OVERFLOW",
            message: "power-of-ten overflow".to_string(),
        })?;
    }
    Ok(value)
}

fn decode_ibm_float32(bytes: &[u8], endian: Endian) -> Result<CodecOutput, DecodeFailure> {
    let raw = read_u32(bytes, endian);
    let value = ibm_hex_float_to_f64(
        u64::from(raw >> 31),
        u64::from((raw >> 24) & 0x7F),
        u64::from(raw & 0x00FF_FFFF),
        24,
    )?;
    Ok(CodecOutput {
        value: DecodedValue::FloatWithRaw {
            value,
            raw: bytes.to_vec(),
        },
        sign_nibble: None,
        sign_class: None,
    })
}

fn decode_ibm_float64(bytes: &[u8], endian: Endian) -> Result<CodecOutput, DecodeFailure> {
    let raw = read_u64(bytes, endian);
    let sign = raw >> 63;
    let exponent = (raw >> 56) & 0x7F;
    let mantissa = raw & 0x00FF_FFFF_FFFF_FFFF;
    let value = ibm_hex_float_to_f64(sign, exponent, mantissa, 56)?;
    Ok(CodecOutput {
        value: DecodedValue::FloatWithRaw {
            value,
            raw: bytes.to_vec(),
        },
        sign_nibble: None,
        sign_class: None,
    })
}

fn ibm_hex_float_to_f64(
    sign: u64,
    exponent: u64,
    mantissa: u64,
    mantissa_bits: i32,
) -> Result<f64, DecodeFailure> {
    if mantissa == 0 {
        return Ok(0.0);
    }
    let sign = if sign == 0 { 1.0 } else { -1.0 };
    let exp_i32 = i32::try_from(exponent).map_err(|_| DecodeFailure {
        code: "E_FLOAT",
        message: "IBM float exponent overflow".to_string(),
    })? - 64;
    let denom = 2f64.powi(mantissa_bits);
    let value = sign * ((mantissa as f64) / denom) * 16f64.powi(exp_i32);
    if !value.is_finite() {
        return Err(DecodeFailure {
            code: "E_FLOAT",
            message: "IBM hexadecimal float converted to non-finite value".to_string(),
        });
    }
    Ok(value)
}

fn decode_alphanumeric(
    bytes: &[u8],
    encoding: FieldEncoding,
    codepage: Option<CodePage>,
) -> Result<CodecOutput, DecodeFailure> {
    let text = match encoding {
        FieldEncoding::Ascii => {
            if !bytes.is_ascii() {
                return Err(DecodeFailure {
                    code: "E_ENCODING",
                    message: "ASCII text contains non-ASCII bytes".to_string(),
                });
            }
            let text = std::str::from_utf8(bytes).map_err(|err| DecodeFailure {
                code: "E_ENCODING",
                message: format!("ASCII text is not valid UTF-8: {err}"),
            })?;
            if text.chars().any(char::is_control) {
                return Err(DecodeFailure {
                    code: "E_ENCODING",
                    message: "ASCII text contains rejected control bytes".to_string(),
                });
            }
            text.to_string()
        }
        FieldEncoding::Ebcdic => decode_ebcdic_text(
            bytes,
            codepage.ok_or_else(|| DecodeFailure {
                code: "E_SCHEMA",
                message: "EBCDIC text requires codepage".to_string(),
            })?,
        )?,
        FieldEncoding::AsciiOverpunch => {
            return Err(DecodeFailure {
                code: "E_SCHEMA",
                message: "alphanumeric cannot use ascii-overpunch".to_string(),
            })
        }
    };
    Ok(CodecOutput {
        value: DecodedValue::Text(text),
        sign_nibble: None,
        sign_class: None,
    })
}

fn decode_ebcdic_text(bytes: &[u8], codepage: CodePage) -> Result<String, DecodeFailure> {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        let Some(ch) = ebcdic_char(byte, codepage) else {
            return Err(DecodeFailure {
                code: "E_ENCODING",
                message: format!("unsupported or control EBCDIC byte 0x{byte:02X}"),
            });
        };
        out.push(ch);
    }
    Ok(out)
}

fn ebcdic_char(byte: u8, codepage: CodePage) -> Option<char> {
    let code = ebcdic_codepoint(byte, codepage);
    let ch = char::from_u32(u32::from(code))?;
    if ch.is_control() {
        None
    } else {
        Some(ch)
    }
}

fn encode_alphanumeric(
    text: &str,
    out: &mut [u8],
    encoding: FieldEncoding,
    codepage: Option<CodePage>,
) -> Result<(), DecodeFailure> {
    match encoding {
        FieldEncoding::Ascii => {
            if text.len() != out.len() {
                return Err(DecodeFailure {
                    code: "E_LENGTH",
                    message: "ASCII text length changed during verification".to_string(),
                });
            }
            if !text.is_ascii() {
                return Err(DecodeFailure {
                    code: "E_ENCODING",
                    message: "ASCII text contains non-ASCII characters".to_string(),
                });
            }
            if text.chars().any(char::is_control) {
                return Err(DecodeFailure {
                    code: "E_ENCODING",
                    message: "ASCII text contains rejected control characters".to_string(),
                });
            }
            out.copy_from_slice(text.as_bytes());
            Ok(())
        }
        FieldEncoding::Ebcdic => {
            let codepage = codepage.ok_or_else(|| DecodeFailure {
                code: "E_SCHEMA",
                message: "EBCDIC text requires codepage".to_string(),
            })?;
            let encoded = encode_ebcdic_text(text, codepage)?;
            if encoded.len() != out.len() {
                return Err(DecodeFailure {
                    code: "E_LENGTH",
                    message: "EBCDIC text length changed during verification".to_string(),
                });
            }
            out.copy_from_slice(&encoded);
            Ok(())
        }
        FieldEncoding::AsciiOverpunch => Err(DecodeFailure {
            code: "E_SCHEMA",
            message: "alphanumeric cannot use ascii-overpunch".to_string(),
        }),
    }
}

fn encode_ebcdic_text(text: &str, codepage: CodePage) -> Result<Vec<u8>, DecodeFailure> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let code = ch as u32;
        if ch.is_control() {
            return Err(DecodeFailure {
                code: "E_ENCODING",
                message: format!("cannot encode control character {ch:?} in cp{}", codepage.0),
            });
        }
        let byte = (0u8..=u8::MAX)
            .find(|candidate| u32::from(ebcdic_codepoint(*candidate, codepage)) == code)
            .ok_or_else(|| DecodeFailure {
                code: "E_ENCODING",
                message: format!("cannot encode character {ch:?} in cp{}", codepage.0),
            })?;
        out.push(byte);
    }
    Ok(out)
}

fn ebcdic_codepoint(byte: u8, codepage: CodePage) -> u16 {
    if byte == 0x9F && matches!(codepage.0, 1140 | 1148) {
        return 0x20AC;
    }
    let table = match codepage.0 {
        37 | 1140 => &CP037,
        500 | 1148 => &CP500,
        _ => &CP037,
    };
    table[usize::from(byte)]
}

const CP037: [u16; 256] = [
    0x0000, 0x0001, 0x0002, 0x0003, 0x009C, 0x0009, 0x0086, 0x007F, 0x0097, 0x008D, 0x008E, 0x000B,
    0x000C, 0x000D, 0x000E, 0x000F, 0x0010, 0x0011, 0x0012, 0x0013, 0x009D, 0x0085, 0x0008, 0x0087,
    0x0018, 0x0019, 0x0092, 0x008F, 0x001C, 0x001D, 0x001E, 0x001F, 0x0080, 0x0081, 0x0082, 0x0083,
    0x0084, 0x000A, 0x0017, 0x001B, 0x0088, 0x0089, 0x008A, 0x008B, 0x008C, 0x0005, 0x0006, 0x0007,
    0x0090, 0x0091, 0x0016, 0x0093, 0x0094, 0x0095, 0x0096, 0x0004, 0x0098, 0x0099, 0x009A, 0x009B,
    0x0014, 0x0015, 0x009E, 0x001A, 0x0020, 0x00A0, 0x00E2, 0x00E4, 0x00E0, 0x00E1, 0x00E3, 0x00E5,
    0x00E7, 0x00F1, 0x00A2, 0x002E, 0x003C, 0x0028, 0x002B, 0x007C, 0x0026, 0x00E9, 0x00EA, 0x00EB,
    0x00E8, 0x00ED, 0x00EE, 0x00EF, 0x00EC, 0x00DF, 0x0021, 0x0024, 0x002A, 0x0029, 0x003B, 0x00AC,
    0x002D, 0x002F, 0x00C2, 0x00C4, 0x00C0, 0x00C1, 0x00C3, 0x00C5, 0x00C7, 0x00D1, 0x00A6, 0x002C,
    0x0025, 0x005F, 0x003E, 0x003F, 0x00F8, 0x00C9, 0x00CA, 0x00CB, 0x00C8, 0x00CD, 0x00CE, 0x00CF,
    0x00CC, 0x0060, 0x003A, 0x0023, 0x0040, 0x0027, 0x003D, 0x0022, 0x00D8, 0x0061, 0x0062, 0x0063,
    0x0064, 0x0065, 0x0066, 0x0067, 0x0068, 0x0069, 0x00AB, 0x00BB, 0x00F0, 0x00FD, 0x00FE, 0x00B1,
    0x00B0, 0x006A, 0x006B, 0x006C, 0x006D, 0x006E, 0x006F, 0x0070, 0x0071, 0x0072, 0x00AA, 0x00BA,
    0x00E6, 0x00B8, 0x00C6, 0x00A4, 0x00B5, 0x007E, 0x0073, 0x0074, 0x0075, 0x0076, 0x0077, 0x0078,
    0x0079, 0x007A, 0x00A1, 0x00BF, 0x00D0, 0x00DD, 0x00DE, 0x00AE, 0x005E, 0x00A3, 0x00A5, 0x00B7,
    0x00A9, 0x00A7, 0x00B6, 0x00BC, 0x00BD, 0x00BE, 0x005B, 0x005D, 0x00AF, 0x00A8, 0x00B4, 0x00D7,
    0x007B, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047, 0x0048, 0x0049, 0x00AD, 0x00F4,
    0x00F6, 0x00F2, 0x00F3, 0x00F5, 0x007D, 0x004A, 0x004B, 0x004C, 0x004D, 0x004E, 0x004F, 0x0050,
    0x0051, 0x0052, 0x00B9, 0x00FB, 0x00FC, 0x00F9, 0x00FA, 0x00FF, 0x005C, 0x00F7, 0x0053, 0x0054,
    0x0055, 0x0056, 0x0057, 0x0058, 0x0059, 0x005A, 0x00B2, 0x00D4, 0x00D6, 0x00D2, 0x00D3, 0x00D5,
    0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036, 0x0037, 0x0038, 0x0039, 0x00B3, 0x00DB,
    0x00DC, 0x00D9, 0x00DA, 0x009F,
];

const CP500: [u16; 256] = [
    0x0000, 0x0001, 0x0002, 0x0003, 0x009C, 0x0009, 0x0086, 0x007F, 0x0097, 0x008D, 0x008E, 0x000B,
    0x000C, 0x000D, 0x000E, 0x000F, 0x0010, 0x0011, 0x0012, 0x0013, 0x009D, 0x0085, 0x0008, 0x0087,
    0x0018, 0x0019, 0x0092, 0x008F, 0x001C, 0x001D, 0x001E, 0x001F, 0x0080, 0x0081, 0x0082, 0x0083,
    0x0084, 0x000A, 0x0017, 0x001B, 0x0088, 0x0089, 0x008A, 0x008B, 0x008C, 0x0005, 0x0006, 0x0007,
    0x0090, 0x0091, 0x0016, 0x0093, 0x0094, 0x0095, 0x0096, 0x0004, 0x0098, 0x0099, 0x009A, 0x009B,
    0x0014, 0x0015, 0x009E, 0x001A, 0x0020, 0x00A0, 0x00E2, 0x00E4, 0x00E0, 0x00E1, 0x00E3, 0x00E5,
    0x00E7, 0x00F1, 0x005B, 0x002E, 0x003C, 0x0028, 0x002B, 0x0021, 0x0026, 0x00E9, 0x00EA, 0x00EB,
    0x00E8, 0x00ED, 0x00EE, 0x00EF, 0x00EC, 0x00DF, 0x005D, 0x0024, 0x002A, 0x0029, 0x003B, 0x005E,
    0x002D, 0x002F, 0x00C2, 0x00C4, 0x00C0, 0x00C1, 0x00C3, 0x00C5, 0x00C7, 0x00D1, 0x00A6, 0x002C,
    0x0025, 0x005F, 0x003E, 0x003F, 0x00F8, 0x00C9, 0x00CA, 0x00CB, 0x00C8, 0x00CD, 0x00CE, 0x00CF,
    0x00CC, 0x0060, 0x003A, 0x0023, 0x0040, 0x0027, 0x003D, 0x0022, 0x00D8, 0x0061, 0x0062, 0x0063,
    0x0064, 0x0065, 0x0066, 0x0067, 0x0068, 0x0069, 0x00AB, 0x00BB, 0x00F0, 0x00FD, 0x00FE, 0x00B1,
    0x00B0, 0x006A, 0x006B, 0x006C, 0x006D, 0x006E, 0x006F, 0x0070, 0x0071, 0x0072, 0x00AA, 0x00BA,
    0x00E6, 0x00B8, 0x00C6, 0x00A4, 0x00B5, 0x007E, 0x0073, 0x0074, 0x0075, 0x0076, 0x0077, 0x0078,
    0x0079, 0x007A, 0x00A1, 0x00BF, 0x00D0, 0x00DD, 0x00DE, 0x00AE, 0x00A2, 0x00A3, 0x00A5, 0x00B7,
    0x00A9, 0x00A7, 0x00B6, 0x00BC, 0x00BD, 0x00BE, 0x00AC, 0x007C, 0x00AF, 0x00A8, 0x00B4, 0x00D7,
    0x007B, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047, 0x0048, 0x0049, 0x00AD, 0x00F4,
    0x00F6, 0x00F2, 0x00F3, 0x00F5, 0x007D, 0x004A, 0x004B, 0x004C, 0x004D, 0x004E, 0x004F, 0x0050,
    0x0051, 0x0052, 0x00B9, 0x00FB, 0x00FC, 0x00F9, 0x00FA, 0x00FF, 0x005C, 0x00F7, 0x0053, 0x0054,
    0x0055, 0x0056, 0x0057, 0x0058, 0x0059, 0x005A, 0x00B2, 0x00D4, 0x00D6, 0x00D2, 0x00D3, 0x00D5,
    0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036, 0x0037, 0x0038, 0x0039, 0x00B3, 0x00DB,
    0x00DC, 0x00D9, 0x00DA, 0x009F,
];

fn rust_ident(path: &str) -> String {
    let mut out = String::new();
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if out
        .as_bytes()
        .first()
        .is_some_and(|first| first.is_ascii_digit())
    {
        out.insert(0, '_');
    }
    out
}

fn rust_type_ident(path: &str) -> String {
    let mut out = String::new();
    let mut uppercase_next = true;
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                out.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                out.push(ch.to_ascii_lowercase());
            }
        } else {
            uppercase_next = true;
        }
    }
    if out.is_empty() {
        out.push_str("Generated");
    }
    if out
        .as_bytes()
        .first()
        .is_some_and(|first| first.is_ascii_digit())
    {
        out.insert(0, 'T');
    }
    out
}
