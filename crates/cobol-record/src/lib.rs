use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutMode {
    #[default]
    Declared,
    Sequential,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlatformProfile {
    #[default]
    IbmZOs,
    MicroFocus,
    GnuCobol,
    IbmI,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordUsage {
    Group,
    PackedDecimal,
    ZonedDecimal,
    Binary,
    NativeBinary,
    IbmFloat32,
    IbmFloat64,
    Alphanumeric,
    Display,
    Filler,
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PicCategory {
    Alphanumeric,
    Alphabetic,
    NumericDisplay,
    NumericEdited,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordPicture {
    pub raw: String,
    pub category: PicCategory,
    pub signed: bool,
    pub digits: usize,
    pub scale: usize,
    pub char_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordOccurs {
    pub min: usize,
    pub max: usize,
    pub depending_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordField {
    pub layout_id: String,
    pub name: String,
    pub qualified_name: String,
    pub path: Vec<String>,
    pub offset: usize,
    pub byte_len: usize,
    pub usage: RecordUsage,
    pub picture: Option<RecordPicture>,
    pub occurs: Option<RecordOccurs>,
    pub redefines: Option<String>,
    pub parent: Option<String>,
    pub addressable: bool,
    pub sync: bool,
    pub value: Option<String>,
    pub source: SourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordRedefines {
    pub redefining_item: String,
    pub base_item: String,
    pub offset: usize,
    pub byte_len: usize,
    pub base_byte_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordConditionName {
    pub name: String,
    pub rust_name: String,
    pub parent: String,
    pub values: Vec<String>,
    pub value_set: Vec<RecordConditionValue>,
    pub source: SourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordConditionValue {
    Single(String),
    Range { start: String, end: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoverageKind {
    Field,
    Filler,
    SyncSlack,
    Occurs,
    RedefinesBase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageRange {
    pub kind: CoverageKind,
    pub name: String,
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageSummary {
    pub record_length: usize,
    pub covered_bytes: usize,
    pub uncovered_bytes: usize,
    pub overlaps: Vec<CoverageRange>,
    pub gaps: Vec<CoverageRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordPlan {
    pub layout_mode: LayoutMode,
    pub platform_profile: PlatformProfile,
    pub record_length: usize,
    pub fields: Vec<RecordField>,
    pub redefines: Vec<RecordRedefines>,
    pub condition_names: Vec<RecordConditionName>,
    pub coverage: CoverageSummary,
}

impl Default for RecordPlan {
    fn default() -> Self {
        Self {
            layout_mode: LayoutMode::Declared,
            platform_profile: PlatformProfile::IbmZOs,
            record_length: 0,
            fields: Vec::new(),
            redefines: Vec::new(),
            condition_names: Vec::new(),
            coverage: CoverageSummary {
                record_length: 0,
                covered_bytes: 0,
                uncovered_bytes: 0,
                overlaps: Vec::new(),
                gaps: Vec::new(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DecodedValue {
    Decimal(Decimal),
    Integer(i64),
    UnsignedInteger(u64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZonedEncoding {
    Ebcdic,
    AsciiOverpunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignPolicy {
    Preferred,
    NonPreferred,
    Permissive {
        blank_as_positive: bool,
        zero_nibble_as_positive: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordCodecError {
    #[error("field length {actual} does not match expected length {expected}")]
    InvalidLength { expected: usize, actual: usize },
    #[error("invalid packed decimal digit nibble 0x{nibble:X}")]
    InvalidPackedDigit { nibble: u8 },
    #[error("invalid packed decimal sign nibble 0x{nibble:X}")]
    InvalidPackedSign { nibble: u8 },
    #[error("packed decimal value exceeds declared {total_digits} digit capacity")]
    PackedDecimalOverflow { total_digits: usize },
    #[error("packed decimal value has fractional precision beyond declared scale {scale}")]
    PackedPrecisionLoss { scale: u32 },
    #[error("negative packed decimal value cannot be stored in an unsigned field")]
    NegativePackedUnsigned,
    #[error("packed decimal mantissa overflow")]
    PackedMantissaOverflow,
    #[error("invalid binary width {width}; expected 2, 4, or 8")]
    InvalidBinaryWidth { width: usize },
    #[error("unsigned 8-byte binary value exceeds i64-compatible decimal path")]
    UnsignedBinaryOverflow,
    #[error("zoned decimal field is empty")]
    EmptyZonedDecimal,
    #[error("invalid zoned decimal digit byte 0x{byte:02X}")]
    InvalidZonedDigitByte { byte: u8 },
    #[error("invalid zoned decimal digit nibble 0x{nibble:X}")]
    InvalidZonedDigitNibble { nibble: u8 },
    #[error("invalid zoned decimal sign zone 0x{zone:X}")]
    InvalidZonedSignZone { zone: u8 },
    #[error("negative zoned decimal sign in unsigned field")]
    NegativeZonedUnsigned,
    #[error("zoned decimal mantissa overflow")]
    ZonedMantissaOverflow,
    #[error("invalid IBM hexadecimal float length {actual}; expected {expected}")]
    InvalidIbmFloatLength { expected: usize, actual: usize },
    #[error("IBM hexadecimal float decoded to a non-finite value")]
    NonFiniteIbmFloat,
}

pub fn packed_decimal_len(total_digits: usize) -> usize {
    (total_digits + 2) / 2
}

pub fn binary_width_from_digits(digits: usize) -> usize {
    match digits {
        0..=4 => 2,
        5..=9 => 4,
        _ => 8,
    }
}

pub fn elementary_byte_len(usage: &RecordUsage, picture: Option<&RecordPicture>) -> usize {
    if matches!(usage, RecordUsage::Group) {
        return 0;
    }
    let Some(pic) = picture else {
        return match usage {
            RecordUsage::IbmFloat32 => 4,
            RecordUsage::IbmFloat64 => 8,
            _ => 0,
        };
    };
    match usage {
        RecordUsage::PackedDecimal => packed_decimal_len(pic.digits),
        RecordUsage::ZonedDecimal => pic.digits.max(pic.char_len),
        RecordUsage::Binary | RecordUsage::NativeBinary => binary_width_from_digits(pic.digits),
        RecordUsage::IbmFloat32 => 4,
        RecordUsage::IbmFloat64 => 8,
        RecordUsage::Alphanumeric | RecordUsage::Display | RecordUsage::Filler => {
            pic.char_len.max(pic.digits)
        }
        RecordUsage::Group | RecordUsage::Unknown(_) => 0,
    }
}

pub fn sync_alignment(
    usage: &RecordUsage,
    byte_len: usize,
    sync: bool,
    profile: PlatformProfile,
) -> Option<usize> {
    if !sync || byte_len <= 1 {
        return None;
    }
    if matches!(profile, PlatformProfile::GnuCobol) {
        return None;
    }
    match usage {
        RecordUsage::Binary
        | RecordUsage::NativeBinary
        | RecordUsage::IbmFloat32
        | RecordUsage::IbmFloat64 => Some(byte_len.min(8)),
        RecordUsage::PackedDecimal => Some(byte_len.min(8)),
        RecordUsage::Group
        | RecordUsage::ZonedDecimal
        | RecordUsage::Alphanumeric
        | RecordUsage::Display
        | RecordUsage::Filler
        | RecordUsage::Unknown(_) => None,
    }
}

pub fn align_offset(offset: usize, alignment: usize) -> usize {
    if alignment <= 1 {
        return offset;
    }
    let rem = offset % alignment;
    if rem == 0 {
        offset
    } else {
        offset + (alignment - rem)
    }
}

pub fn coverage_summary(record_length: usize, ranges: &[CoverageRange]) -> CoverageSummary {
    let mut counts = vec![0u16; record_length];
    for range in ranges {
        let end = range.offset.saturating_add(range.length).min(record_length);
        for count in counts
            .iter_mut()
            .take(end)
            .skip(range.offset.min(record_length))
        {
            *count = count.saturating_add(1);
        }
    }

    let covered_bytes = counts.iter().filter(|count| **count > 0).count();
    let mut gaps = Vec::new();
    let mut overlaps = Vec::new();
    collect_runs(&counts, 0, CoverageKind::Field, "gap", &mut gaps);
    collect_runs_gt_one(&counts, &mut overlaps);
    CoverageSummary {
        record_length,
        covered_bytes,
        uncovered_bytes: record_length.saturating_sub(covered_bytes),
        overlaps,
        gaps,
    }
}

fn collect_runs(
    counts: &[u16],
    target: u16,
    kind: CoverageKind,
    label: &str,
    out: &mut Vec<CoverageRange>,
) {
    let mut start = None;
    for (idx, count) in counts.iter().enumerate() {
        if *count == target {
            start.get_or_insert(idx);
        } else if let Some(offset) = start.take() {
            out.push(CoverageRange {
                kind,
                name: label.to_string(),
                offset,
                length: idx - offset,
            });
        }
    }
    if let Some(offset) = start {
        out.push(CoverageRange {
            kind,
            name: label.to_string(),
            offset,
            length: counts.len() - offset,
        });
    }
}

fn collect_runs_gt_one(counts: &[u16], out: &mut Vec<CoverageRange>) {
    let mut start = None;
    for (idx, count) in counts.iter().enumerate() {
        if *count > 1 {
            start.get_or_insert(idx);
        } else if let Some(offset) = start.take() {
            out.push(CoverageRange {
                kind: CoverageKind::Field,
                name: "overlap".to_string(),
                offset,
                length: idx - offset,
            });
        }
    }
    if let Some(offset) = start {
        out.push(CoverageRange {
            kind: CoverageKind::Field,
            name: "overlap".to_string(),
            offset,
            length: counts.len() - offset,
        });
    }
}

pub fn decode_packed_decimal(
    bytes: &[u8],
    total_digits: usize,
    scale: u32,
    signed: bool,
) -> Result<Decimal, RecordCodecError> {
    let expected = packed_decimal_len(total_digits);
    if bytes.len() != expected {
        return Err(RecordCodecError::InvalidLength {
            expected,
            actual: bytes.len(),
        });
    }
    let available_digit_nibbles = bytes.len().saturating_mul(2).saturating_sub(1);
    let leading_pad_nibbles = available_digit_nibbles.saturating_sub(total_digits);
    let mut mantissa = 0i128;
    let mut nibbles_seen = 0usize;
    for (byte_idx, byte) in bytes.iter().enumerate() {
        let high = byte >> 4;
        let low = byte & 0x0F;
        if byte_idx + 1 == bytes.len() {
            if nibbles_seen >= leading_pad_nibbles {
                mantissa = push_digit(mantissa, high)?;
            } else if high != 0 {
                return Err(RecordCodecError::InvalidPackedDigit { nibble: high });
            }
            let negative = match low {
                0xC | 0xF => false,
                0xD if signed => true,
                0xD => return Err(RecordCodecError::InvalidPackedSign { nibble: low }),
                other => return Err(RecordCodecError::InvalidPackedSign { nibble: other }),
            };
            if negative {
                mantissa = -mantissa;
            }
        } else {
            for digit in [high, low] {
                if nibbles_seen >= leading_pad_nibbles {
                    mantissa = push_digit(mantissa, digit)?;
                } else if digit != 0 {
                    return Err(RecordCodecError::InvalidPackedDigit { nibble: digit });
                }
                nibbles_seen += 1;
            }
        }
    }
    Ok(Decimal::from_i128_with_scale(mantissa, scale))
}

pub fn encode_packed_decimal(
    value: Decimal,
    total_digits: usize,
    scale: u32,
    signed: bool,
) -> Result<Vec<u8>, RecordCodecError> {
    let expected = packed_decimal_len(total_digits);
    let mut out = vec![0u8; expected];
    encode_packed_decimal_into(value, total_digits, scale, signed, &mut out)?;
    Ok(out)
}

pub fn encode_packed_decimal_into(
    value: Decimal,
    total_digits: usize,
    scale: u32,
    signed: bool,
    out: &mut [u8],
) -> Result<(), RecordCodecError> {
    if total_digits == 0 {
        return Err(RecordCodecError::PackedDecimalOverflow { total_digits });
    }
    let expected = packed_decimal_len(total_digits);
    if out.len() != expected {
        return Err(RecordCodecError::InvalidLength {
            expected,
            actual: out.len(),
        });
    }
    let (scaled, is_negative) = scaled_packed_mantissa(value, total_digits, scale, signed)?;
    let sign = if signed {
        if is_negative {
            0x0D
        } else {
            0x0C
        }
    } else {
        0x0F
    };
    write_packed_mantissa(scaled, total_digits, sign, out)
}

fn scaled_packed_mantissa(
    value: Decimal,
    total_digits: usize,
    scale: u32,
    signed: bool,
) -> Result<(i128, bool), RecordCodecError> {
    let raw_mantissa = value.mantissa();
    if raw_mantissa == i128::MIN {
        return Err(RecordCodecError::PackedMantissaOverflow);
    }
    let is_negative = raw_mantissa < 0 || value.is_sign_negative();
    if is_negative && !signed {
        return Err(RecordCodecError::NegativePackedUnsigned);
    }

    let mut mantissa = raw_mantissa.abs();
    let current_scale = value.scale();
    if scale >= current_scale {
        let factor = pow10_i128(scale - current_scale)?;
        mantissa = mantissa
            .checked_mul(factor)
            .ok_or(RecordCodecError::PackedMantissaOverflow)?;
    } else {
        let divisor = pow10_i128(current_scale - scale)?;
        if mantissa % divisor != 0 {
            return Err(RecordCodecError::PackedPrecisionLoss { scale });
        }
        mantissa /= divisor;
    }

    let max = packed_digit_max(total_digits)?;
    if mantissa > max {
        return Err(RecordCodecError::PackedDecimalOverflow { total_digits });
    }
    Ok((mantissa, is_negative))
}

fn packed_digit_max(total_digits: usize) -> Result<i128, RecordCodecError> {
    let factor = pow10_i128(
        u32::try_from(total_digits).map_err(|_| RecordCodecError::PackedMantissaOverflow)?,
    )?;
    factor
        .checked_sub(1)
        .ok_or(RecordCodecError::PackedMantissaOverflow)
}

fn pow10_i128(exp: u32) -> Result<i128, RecordCodecError> {
    let mut value = 1i128;
    for _ in 0..exp {
        value = value
            .checked_mul(10)
            .ok_or(RecordCodecError::PackedMantissaOverflow)?;
    }
    Ok(value)
}

fn write_packed_mantissa(
    mut mantissa: i128,
    total_digits: usize,
    sign: u8,
    out: &mut [u8],
) -> Result<(), RecordCodecError> {
    let mut digit_nibbles = vec![0u8; total_digits];
    for idx in (0..total_digits).rev() {
        digit_nibbles[idx] =
            u8::try_from(mantissa % 10).map_err(|_| RecordCodecError::PackedMantissaOverflow)?;
        mantissa /= 10;
    }

    let leading_pad = total_digits % 2 == 0;
    let mut nibbles = Vec::with_capacity(out.len() * 2);
    if leading_pad {
        nibbles.push(0);
    }
    nibbles.extend(digit_nibbles);
    nibbles.push(sign);

    for (idx, byte) in out.iter_mut().enumerate() {
        *byte = (nibbles[idx * 2] << 4) | nibbles[idx * 2 + 1];
    }
    Ok(())
}

fn push_digit(mantissa: i128, digit: u8) -> Result<i128, RecordCodecError> {
    if digit > 9 {
        return Err(RecordCodecError::InvalidPackedDigit { nibble: digit });
    }
    mantissa
        .checked_mul(10)
        .and_then(|value| value.checked_add(i128::from(digit)))
        .ok_or(RecordCodecError::PackedMantissaOverflow)
}

pub fn decode_binary_integer(
    bytes: &[u8],
    signed: bool,
    endian: Endian,
) -> Result<DecodedValue, RecordCodecError> {
    match bytes.len() {
        2 => {
            let arr = [bytes[0], bytes[1]];
            if signed {
                let value = match endian {
                    Endian::Big => i16::from_be_bytes(arr),
                    Endian::Little => i16::from_le_bytes(arr),
                };
                Ok(DecodedValue::Integer(i64::from(value)))
            } else {
                let value = match endian {
                    Endian::Big => u16::from_be_bytes(arr),
                    Endian::Little => u16::from_le_bytes(arr),
                };
                Ok(DecodedValue::UnsignedInteger(u64::from(value)))
            }
        }
        4 => {
            let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
            if signed {
                let value = match endian {
                    Endian::Big => i32::from_be_bytes(arr),
                    Endian::Little => i32::from_le_bytes(arr),
                };
                Ok(DecodedValue::Integer(i64::from(value)))
            } else {
                let value = match endian {
                    Endian::Big => u32::from_be_bytes(arr),
                    Endian::Little => u32::from_le_bytes(arr),
                };
                Ok(DecodedValue::UnsignedInteger(u64::from(value)))
            }
        }
        8 => {
            let arr = [
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ];
            if signed {
                let value = match endian {
                    Endian::Big => i64::from_be_bytes(arr),
                    Endian::Little => i64::from_le_bytes(arr),
                };
                Ok(DecodedValue::Integer(value))
            } else {
                let value = match endian {
                    Endian::Big => u64::from_be_bytes(arr),
                    Endian::Little => u64::from_le_bytes(arr),
                };
                Ok(DecodedValue::UnsignedInteger(value))
            }
        }
        width => Err(RecordCodecError::InvalidBinaryWidth { width }),
    }
}

pub fn decode_zoned_decimal(
    bytes: &[u8],
    scale: u32,
    signed: bool,
    encoding: ZonedEncoding,
    sign_policy: SignPolicy,
) -> Result<Decimal, RecordCodecError> {
    if bytes.is_empty() {
        return Err(RecordCodecError::EmptyZonedDecimal);
    }
    match encoding {
        ZonedEncoding::Ebcdic => decode_ebcdic_zoned(bytes, scale, signed, sign_policy),
        ZonedEncoding::AsciiOverpunch => decode_ascii_overpunch(bytes, scale, signed),
    }
}

fn decode_ebcdic_zoned(
    bytes: &[u8],
    scale: u32,
    signed: bool,
    sign_policy: SignPolicy,
) -> Result<Decimal, RecordCodecError> {
    let mut mantissa = 0i128;
    for byte in &bytes[..bytes.len() - 1] {
        let zone = byte >> 4;
        let digit = byte & 0x0F;
        if zone != 0xF || digit > 9 {
            return Err(RecordCodecError::InvalidZonedDigitByte { byte: *byte });
        }
        mantissa = push_zoned_digit(mantissa, digit)?;
    }
    let last = bytes[bytes.len() - 1];
    let zone = last >> 4;
    let digit = last & 0x0F;
    if digit > 9 {
        return Err(RecordCodecError::InvalidZonedDigitNibble { nibble: digit });
    }
    let negative = classify_zoned_zone(zone, digit, signed, sign_policy)?;
    mantissa = push_zoned_digit(mantissa, digit)?;
    if negative {
        mantissa = -mantissa;
    }
    Ok(Decimal::from_i128_with_scale(mantissa, scale))
}

fn classify_zoned_zone(
    zone: u8,
    digit: u8,
    signed: bool,
    sign_policy: SignPolicy,
) -> Result<bool, RecordCodecError> {
    match sign_policy {
        SignPolicy::Preferred => match zone {
            0xC | 0xF => Ok(false),
            0xD if signed => Ok(true),
            0xD => Err(RecordCodecError::NegativeZonedUnsigned),
            _ => Err(RecordCodecError::InvalidZonedSignZone { zone }),
        },
        SignPolicy::NonPreferred => match zone {
            0xA | 0xC | 0xE | 0xF => Ok(false),
            0xB | 0xD if signed => Ok(true),
            0xB | 0xD => Err(RecordCodecError::NegativeZonedUnsigned),
            _ => Err(RecordCodecError::InvalidZonedSignZone { zone }),
        },
        SignPolicy::Permissive {
            blank_as_positive,
            zero_nibble_as_positive,
        } => match zone {
            0xA | 0xC | 0xE | 0xF => Ok(false),
            0xB | 0xD if signed => Ok(true),
            0xB | 0xD => Err(RecordCodecError::NegativeZonedUnsigned),
            0x0 if zero_nibble_as_positive => Ok(false),
            0x4 if blank_as_positive && digit == 0 => Ok(false),
            _ => Err(RecordCodecError::InvalidZonedSignZone { zone }),
        },
    }
}

fn decode_ascii_overpunch(
    bytes: &[u8],
    scale: u32,
    signed: bool,
) -> Result<Decimal, RecordCodecError> {
    let mut mantissa = 0i128;
    for byte in &bytes[..bytes.len() - 1] {
        if !byte.is_ascii_digit() {
            return Err(RecordCodecError::InvalidZonedDigitByte { byte: *byte });
        }
        mantissa = push_zoned_digit(mantissa, byte - b'0')?;
    }
    let last = bytes[bytes.len() - 1];
    let (digit, negative) = match last {
        b'0'..=b'9' => (last - b'0', false),
        b'{' => (0, false),
        b'A'..=b'I' => (last - b'A' + 1, false),
        b'}' if signed => (0, true),
        b'}' => return Err(RecordCodecError::NegativeZonedUnsigned),
        b'J'..=b'R' if signed => (last - b'J' + 1, true),
        b'J'..=b'R' => return Err(RecordCodecError::NegativeZonedUnsigned),
        _ => return Err(RecordCodecError::InvalidZonedDigitByte { byte: last }),
    };
    mantissa = push_zoned_digit(mantissa, digit)?;
    if negative {
        mantissa = -mantissa;
    }
    Ok(Decimal::from_i128_with_scale(mantissa, scale))
}

fn push_zoned_digit(mantissa: i128, digit: u8) -> Result<i128, RecordCodecError> {
    mantissa
        .checked_mul(10)
        .and_then(|value| value.checked_add(i128::from(digit)))
        .ok_or(RecordCodecError::ZonedMantissaOverflow)
}

pub fn decode_ibm_float32(bytes: &[u8], endian: Endian) -> Result<f64, RecordCodecError> {
    if bytes.len() != 4 {
        return Err(RecordCodecError::InvalidIbmFloatLength {
            expected: 4,
            actual: bytes.len(),
        });
    }
    let raw = match endian {
        Endian::Big => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        Endian::Little => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    };
    let value = ibm_float_to_f64(
        (raw >> 31) != 0,
        ((raw >> 24) & 0x7F) as i32,
        u64::from(raw & 0x00FF_FFFF),
        24,
    );
    if value.is_finite() {
        Ok(value)
    } else {
        Err(RecordCodecError::NonFiniteIbmFloat)
    }
}

pub fn decode_ibm_float64(bytes: &[u8], endian: Endian) -> Result<f64, RecordCodecError> {
    if bytes.len() != 8 {
        return Err(RecordCodecError::InvalidIbmFloatLength {
            expected: 8,
            actual: bytes.len(),
        });
    }
    let raw = match endian {
        Endian::Big => u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        Endian::Little => u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
    };
    let value = ibm_float_to_f64(
        (raw >> 63) != 0,
        ((raw >> 56) & 0x7F) as i32,
        raw & 0x00FF_FFFF_FFFF_FFFF,
        56,
    );
    if value.is_finite() {
        Ok(value)
    } else {
        Err(RecordCodecError::NonFiniteIbmFloat)
    }
}

fn ibm_float_to_f64(sign: bool, exponent: i32, mantissa: u64, mantissa_bits: i32) -> f64 {
    if mantissa == 0 {
        return 0.0;
    }
    let sign = if sign { -1.0 } else { 1.0 };
    let fraction = mantissa as f64 / 2f64.powi(mantissa_bits);
    sign * fraction * 16f64.powi(exponent - 64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Endian {
    Big,
    Little,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_lengths_match_cobol_storage_rules() {
        assert_eq!(packed_decimal_len(1), 1);
        assert_eq!(packed_decimal_len(9), 5);
        assert_eq!(binary_width_from_digits(4), 2);
        assert_eq!(binary_width_from_digits(9), 4);
        assert_eq!(binary_width_from_digits(18), 8);
    }

    #[test]
    fn coverage_reports_gaps_and_overlaps() {
        let summary = coverage_summary(
            6,
            &[
                CoverageRange {
                    kind: CoverageKind::Field,
                    name: "A".to_string(),
                    offset: 0,
                    length: 2,
                },
                CoverageRange {
                    kind: CoverageKind::Field,
                    name: "B".to_string(),
                    offset: 1,
                    length: 2,
                },
                CoverageRange {
                    kind: CoverageKind::Field,
                    name: "C".to_string(),
                    offset: 4,
                    length: 1,
                },
            ],
        );
        assert_eq!(summary.covered_bytes, 4);
        assert_eq!(summary.uncovered_bytes, 2);
        assert_eq!(summary.overlaps[0].offset, 1);
        assert_eq!(summary.gaps[0].offset, 3);
    }

    #[test]
    fn decodes_binary_big_and_little_endian() {
        assert_eq!(
            decode_binary_integer(&[0x00, 0x01], true, Endian::Big).unwrap(),
            DecodedValue::Integer(1)
        );
        assert_eq!(
            decode_binary_integer(&[0x01, 0x00], true, Endian::Little).unwrap(),
            DecodedValue::Integer(1)
        );
    }

    #[test]
    fn decodes_even_digit_packed_with_leading_pad() {
        assert_eq!(
            decode_packed_decimal(&[0x01, 0x23, 0x4C], 4, 2, true).unwrap(),
            Decimal::new(1234, 2)
        );
    }

    #[test]
    fn decodes_odd_digit_packed_without_leading_pad() {
        assert_eq!(
            decode_packed_decimal(&[0x12, 0x34, 0x5D], 5, 0, true).unwrap(),
            Decimal::new(-12345, 0)
        );
    }

    #[test]
    fn encodes_packed_decimal_strictly() {
        assert_eq!(
            encode_packed_decimal(Decimal::new(123, 0), 3, 0, true).unwrap(),
            vec![0x12, 0x3C]
        );
        assert_eq!(
            encode_packed_decimal(Decimal::new(-45, 0), 3, 0, true).unwrap(),
            vec![0x04, 0x5D]
        );
        assert_eq!(
            encode_packed_decimal(Decimal::new(1234, 2), 4, 2, true).unwrap(),
            vec![0x01, 0x23, 0x4C]
        );
        assert!(matches!(
            encode_packed_decimal(Decimal::new(1234, 0), 3, 0, true),
            Err(RecordCodecError::PackedDecimalOverflow { total_digits: 3 })
        ));
        assert!(matches!(
            encode_packed_decimal(Decimal::new(12345, 3), 4, 2, true),
            Err(RecordCodecError::PackedPrecisionLoss { scale: 2 })
        ));
        assert!(matches!(
            encode_packed_decimal(Decimal::new(-1, 0), 1, 0, false),
            Err(RecordCodecError::NegativePackedUnsigned)
        ));
    }

    #[test]
    fn decodes_ebcdic_zoned_and_ascii_overpunch() {
        assert_eq!(
            decode_zoned_decimal(
                &[0xF1, 0xF2, 0xD3],
                0,
                true,
                ZonedEncoding::Ebcdic,
                SignPolicy::Preferred
            )
            .unwrap(),
            Decimal::new(-123, 0)
        );
        assert_eq!(
            decode_zoned_decimal(
                b"12L",
                0,
                true,
                ZonedEncoding::AsciiOverpunch,
                SignPolicy::Preferred
            )
            .unwrap(),
            Decimal::new(-123, 0)
        );
    }

    #[test]
    fn decodes_ibm_hex_float_fixture() {
        let value = decode_ibm_float32(&[0x42, 0x64, 0x00, 0x00], Endian::Big).unwrap();
        assert!((value - 100.0).abs() < f64::EPSILON);
    }
}
