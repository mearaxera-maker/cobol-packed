use super::*;

pub(super) fn decode_plan_field(
    record_index: Option<usize>,
    plan: &FieldPlan<'_>,
    offset: Option<usize>,
    bytes: &[u8],
    verify: bool,
) -> DecodedField {
    let mut row = match plan.kind {
        FieldPlanKind::PackedDecimal(cfg) => decode_named_field(
            record_index,
            &plan.spec.name,
            offset,
            bytes,
            &cfg,
            plan.spec.sign_mode.to_core(),
            plan.spec.mode,
        ),
        FieldPlanKind::DisplayText(encoding) => {
            decode_display_text_field(record_index, &plan.spec.name, offset, bytes, encoding)
        }
        FieldPlanKind::MixedDbcsText(encoding) => {
            decode_mixed_dbcs_text_field(record_index, &plan.spec.name, offset, bytes, encoding)
        }
        FieldPlanKind::ZonedDecimal(cfg) => decode_zoned_decimal_field(
            record_index,
            &plan.spec.name,
            offset,
            bytes,
            &cfg,
            plan.spec.sign_mode.to_core(),
        ),
        FieldPlanKind::Binary { signed, scale } => {
            decode_binary_field(record_index, &plan.spec.name, offset, bytes, signed, scale)
        }
        FieldPlanKind::RawBytes => {
            decode_raw_bytes_field(record_index, &plan.spec.name, offset, bytes)
        }
    };
    if verify && row.valid && !verify_plan_field(plan, bytes) {
        row.valid = false;
        row.error_code = Some("E_VERIFY");
        row.error_docs_url = Some(ERROR_DOCS_BASE_URL);
        row.message = Some("lossless re-encode did not match original bytes".to_string());
        row.recoverable = true;
    }
    row
}

fn verify_plan_field(plan: &FieldPlan<'_>, bytes: &[u8]) -> bool {
    match plan.kind {
        FieldPlanKind::PackedDecimal(cfg) => {
            match from_packed_lossless(bytes, &cfg, plan.spec.sign_mode.to_core()) {
                Ok(loss) => to_packed_lossless(&loss, &cfg)
                    .map(|rebuilt| rebuilt == bytes)
                    .unwrap_or(false),
                Err(_) => false,
            }
        }
        FieldPlanKind::DisplayText(encoding) => ebcdic::decode(bytes, encoding)
            .and_then(|text| ebcdic::encode(&text, encoding))
            .map(|rebuilt| rebuilt == bytes)
            .unwrap_or(false),
        FieldPlanKind::MixedDbcsText(encoding) => mixed_dbcs::decode(bytes, encoding)
            .and_then(|text| mixed_dbcs::encode(&text, encoding))
            .map(|rebuilt| rebuilt == bytes)
            .unwrap_or(false),
        FieldPlanKind::ZonedDecimal(cfg) => {
            verify_zoned_decimal_lossless(bytes, &cfg, plan.spec.sign_mode.to_core())
        }
        FieldPlanKind::Binary { signed, scale } => {
            verify_binary_integer_lossless(bytes, signed, scale)
        }
        FieldPlanKind::RawBytes => true,
    }
}

fn decode_display_text_field(
    record_index: Option<usize>,
    name: &str,
    offset: Option<usize>,
    bytes: &[u8],
    encoding: TextEncoding,
) -> DecodedField {
    match ebcdic::decode(bytes, encoding) {
        Ok(value) => DecodedField::valid(record_index, name, offset, bytes, value, None, None),
        Err(err) => DecodedField::error(record_index, name, offset, bytes, "E_ENCODING", err),
    }
}

fn decode_mixed_dbcs_text_field(
    record_index: Option<usize>,
    name: &str,
    offset: Option<usize>,
    bytes: &[u8],
    encoding: TextEncoding,
) -> DecodedField {
    match mixed_dbcs::decode(bytes, encoding) {
        Ok(value) => DecodedField::valid(record_index, name, offset, bytes, value, None, None),
        Err(err) => DecodedField::error(record_index, name, offset, bytes, "E_ENCODING", err),
    }
}

fn decode_raw_bytes_field(
    record_index: Option<usize>,
    name: &str,
    offset: Option<usize>,
    bytes: &[u8],
) -> DecodedField {
    DecodedField::valid(record_index, name, offset, bytes, to_hex(bytes), None, None)
}

fn decode_zoned_decimal_field(
    record_index: Option<usize>,
    name: &str,
    offset: Option<usize>,
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
) -> DecodedField {
    match decode_zoned_decimal(bytes, cfg, sign_mode) {
        Ok((value, sign_nibble, sign_class)) => DecodedField::valid(
            record_index,
            name,
            offset,
            bytes,
            value.to_string(),
            Some(format!("0x{sign_nibble:X}")),
            Some(sign_class.to_string()),
        ),
        Err(err) => DecodedField::error(record_index, name, offset, bytes, "E_DIGIT", err),
    }
}

fn decode_binary_field(
    record_index: Option<usize>,
    name: &str,
    offset: Option<usize>,
    bytes: &[u8],
    signed: bool,
    scale: u8,
) -> DecodedField {
    match decode_binary_integer(bytes, signed, scale) {
        Ok(value) => DecodedField::valid(record_index, name, offset, bytes, value, None, None),
        Err(err) => DecodedField::error(record_index, name, offset, bytes, "E_LENGTH", err),
    }
}

pub(super) fn decode_named_field(
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
        FieldMode::Canonical => from_packed(bytes, cfg, sign_mode).map(|value| LosslessDecimal {
            value,
            sign_nibble: sign_nibble.unwrap_or(0),
        }),
        FieldMode::Lossless => from_packed_lossless(bytes, cfg, sign_mode),
    };
    match result {
        Ok(loss) => {
            let (raw_hex, raw_hex_truncated) = raw_hex_for_output(bytes);
            DecodedField {
                version: OUTPUT_VERSION,
                record_index,
                field: name.to_string(),
                offset,
                raw_hex,
                raw_byte_len: bytes.len(),
                raw_hex_truncated,
                value: Some(loss.value.to_string()),
                sign_nibble: sign_nibble.map(|n| format!("0x{n:X}")),
                sign_class,
                valid: true,
                error_code: None,
                error_docs_url: None,
                message: None,
                recoverable: false,
            }
        }
        Err(err) => DecodedField::packed_error(record_index, name, offset, bytes, err),
    }
}

fn decode_zoned_decimal(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
) -> Result<(Decimal, u8, &'static str), String> {
    if bytes.len() != cfg.total_digits() as usize {
        return Err(format!(
            "zoned decimal length mismatch: expected {} bytes, got {}",
            cfg.total_digits(),
            bytes.len()
        ));
    }
    let mut magnitude = 0i128;
    let mut negative = false;
    let mut sign_nibble = 0x0F;
    let mut sign_class = if cfg.is_signed() {
        "positive"
    } else {
        "unsigned-positive"
    };
    for (idx, byte) in bytes.iter().copied().enumerate() {
        let zone = byte >> 4;
        let digit = byte & 0x0F;
        if digit > 9 {
            return Err(format!(
                "invalid zoned digit nibble 0x{digit:X} at position {idx}"
            ));
        }
        if idx + 1 == bytes.len() {
            sign_nibble = zone;
            match zone {
                0x0F => {
                    sign_class = if cfg.is_signed() {
                        "positive"
                    } else {
                        "unsigned-positive"
                    };
                }
                0x0C if cfg.is_signed() => sign_class = "positive",
                0x0D if cfg.is_signed() => {
                    negative = true;
                    sign_class = "negative";
                }
                0x0A | 0x0E if cfg.is_signed() && matches!(sign_mode, SignMode::Nopfd) => {
                    sign_class = "positive";
                }
                0x0B if cfg.is_signed() && matches!(sign_mode, SignMode::Nopfd) => {
                    negative = true;
                    sign_class = "negative";
                }
                _ => {
                    return Err(format!(
                        "invalid zoned sign zone 0x{zone:X} at final position"
                    ));
                }
            }
        } else if zone != 0x0F {
            return Err(format!(
                "invalid zoned numeric zone 0x{zone:X} at position {idx}"
            ));
        }
        magnitude = magnitude
            .checked_mul(10)
            .and_then(|current| current.checked_add(digit as i128))
            .ok_or_else(|| "zoned decimal magnitude overflow".to_string())?;
    }
    if negative && !cfg.is_signed() {
        return Err("negative zoned sign not allowed in unsigned field".to_string());
    }
    let signed = if negative { -magnitude } else { magnitude };
    Ok((
        Decimal::from_i128_with_scale(signed, cfg.scale() as u32),
        sign_nibble,
        sign_class,
    ))
}

fn verify_zoned_decimal_lossless(bytes: &[u8], cfg: &PackedConfig, sign_mode: SignMode) -> bool {
    if decode_zoned_decimal(bytes, cfg, sign_mode).is_err() {
        return false;
    }
    let mut rebuilt = Vec::with_capacity(bytes.len());
    for (idx, byte) in bytes.iter().copied().enumerate() {
        let digit = byte & 0x0F;
        let zone = if idx + 1 == bytes.len() {
            byte & 0xF0
        } else {
            0xF0
        };
        rebuilt.push(zone | digit);
    }
    rebuilt == bytes
}

fn decode_binary_integer(bytes: &[u8], signed: bool, scale: u8) -> Result<String, String> {
    let raw = match bytes.len() {
        1 => {
            if signed {
                i8::from_be_bytes([bytes[0]]) as i128
            } else {
                bytes[0] as i128
            }
        }
        2 => {
            let arr = [bytes[0], bytes[1]];
            if signed {
                i16::from_be_bytes(arr) as i128
            } else {
                u16::from_be_bytes(arr) as i128
            }
        }
        4 => {
            let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
            if signed {
                i32::from_be_bytes(arr) as i128
            } else {
                u32::from_be_bytes(arr) as i128
            }
        }
        8 => {
            let arr = [
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ];
            if signed {
                i64::from_be_bytes(arr) as i128
            } else {
                u64::from_be_bytes(arr) as i128
            }
        }
        other => return Err(format!("binary length must be 1, 2, 4, or 8, got {other}")),
    };
    Ok(Decimal::from_i128_with_scale(raw, scale as u32).to_string())
}

fn verify_binary_integer_lossless(bytes: &[u8], signed: bool, scale: u8) -> bool {
    if decode_binary_integer(bytes, signed, scale).is_err() {
        return false;
    }
    match bytes.len() {
        1 => true,
        2 => {
            let arr = [bytes[0], bytes[1]];
            if signed {
                i16::from_be_bytes(arr).to_be_bytes() == arr
            } else {
                u16::from_be_bytes(arr).to_be_bytes() == arr
            }
        }
        4 => {
            let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
            if signed {
                i32::from_be_bytes(arr).to_be_bytes() == arr
            } else {
                u32::from_be_bytes(arr).to_be_bytes() == arr
            }
        }
        8 => {
            let arr = [
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ];
            if signed {
                i64::from_be_bytes(arr).to_be_bytes() == arr
            } else {
                u64::from_be_bytes(arr).to_be_bytes() == arr
            }
        }
        _ => false,
    }
}
