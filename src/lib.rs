//! Packed-decimal codec for IBM Enterprise COBOL COMP-3 fields.
//!
//! This crate deliberately splits the problem into:
//! - a dynamic runtime codec (`PackedConfig`)
//! - a const-generic zero-sized codec (`Packed<DIGITS, SCALE, SIGNED>`)
//! - a lossless forensic wrapper (`LosslessDecimal`)
//! - a scalar reference decoder with optional SIMD validation
//! - an explicit policy object for zero/sign handling
//!
//! The implementation is conservative: no `abs()` on external decimals, exact
//! overflow boundaries, explicit sign policy, and byte-for-byte lossless round
//! trips when requested.

#![warn(missing_docs)]

use rust_decimal::Decimal;
use thiserror::Error;

#[cfg(feature = "simd")]
mod simd;

/// Precomputed powers of 10 for `0..=38`.
const POW10: [i128; 39] = {
    let mut t = [1i128; 39];
    let mut i = 1;
    while i < 39 {
        t[i] = t[i - 1] * 10;
        i += 1;
    }
    t
};

#[inline]
fn pow10(exp: u32) -> Option<i128> {
    POW10.get(exp as usize).copied()
}

/// COBOL sign interpretation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignMode {
    /// Preferred signs only (`C`/`D` and `F` for unsigned positive).
    Pfd,
    /// Non-preferred signs accepted (`A`–`F` family).
    Nopfd,
}

/// Policy for how zero sign bits are handled when encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroSignPolicy {
    /// Encode zero with canonical positive/unsigned sign nibble.
    Canonical,
    /// Preserve the incoming or computed sign nibble even for zero.
    Preserve,
}

/// Explicit policy object for decode sign mode and zero handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedPolicy {
    /// Which sign classes are accepted while decoding.
    pub sign_mode: SignMode,
    /// How zero is encoded.
    pub zero_sign: ZeroSignPolicy,
}

impl PackedPolicy {
    /// Canonical policy: `PFD` decoding plus canonical zero encoding.
    pub const fn canonical(sign_mode: SignMode) -> Self {
        Self {
            sign_mode,
            zero_sign: ZeroSignPolicy::Canonical,
        }
    }

    /// Lossless policy: `NOPFD`-friendly decoding plus zero-sign preservation.
    pub const fn lossless(sign_mode: SignMode) -> Self {
        Self {
            sign_mode,
            zero_sign: ZeroSignPolicy::Preserve,
        }
    }
}

/// Validated runtime configuration for a packed-decimal field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedConfig {
    total_digits: u8,
    scale: u8,
    is_signed: bool,
}

impl PackedConfig {
    /// Create a validated config.
    pub fn new(total_digits: u8, scale: u8, is_signed: bool) -> Result<Self, PackedError> {
        if !(1..=18).contains(&total_digits) {
            return Err(PackedError::InvalidTotalDigits(total_digits));
        }
        if scale > total_digits {
            return Err(PackedError::ScaleExceedsTotalDigits {
                scale,
                total_digits,
            });
        }
        Ok(Self {
            total_digits,
            scale,
            is_signed,
        })
    }

    /// `PIC S9(...)` helper.
    pub fn signed(total_digits: u8, scale: u8) -> Result<Self, PackedError> {
        Self::new(total_digits, scale, true)
    }

    /// `PIC 9(...)` helper.
    pub fn unsigned(total_digits: u8, scale: u8) -> Result<Self, PackedError> {
        Self::new(total_digits, scale, false)
    }

    /// Total digits.
    #[inline]
    pub const fn total_digits(&self) -> u8 {
        self.total_digits
    }
    /// Scale.
    #[inline]
    pub const fn scale(&self) -> u8 {
        self.scale
    }
    /// Signedness.
    #[inline]
    pub const fn is_signed(&self) -> bool {
        self.is_signed
    }

    /// Packed byte length for this schema.
    #[inline]
    pub const fn byte_len(&self) -> usize {
        expected_len(self.total_digits)
    }

    /// Maximum representable value.
    #[inline]
    pub fn max_value(&self) -> Decimal {
        Decimal::from_i128_with_scale(digit_max(self.total_digits), self.scale as u32)
    }

    /// Minimum representable value.
    #[inline]
    pub fn min_value(&self) -> Decimal {
        if self.is_signed {
            -self.max_value()
        } else {
            Decimal::ZERO
        }
    }
}

/// Zero-sized const-generic packed codec.
///
/// Example: `Packed::<18, 2, true>::encode(&value)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Packed<const DIGITS: u8, const SCALE: u8, const SIGNED: bool>;

impl<const DIGITS: u8, const SCALE: u8, const SIGNED: bool> Packed<DIGITS, SCALE, SIGNED> {
    /// Exact packed byte length for this schema.
    pub const LEN: usize = {
        let () = Self::VALID;
        expected_len(DIGITS)
    };

    const VALID: () = {
        assert!(DIGITS >= 1 && DIGITS <= 18, "DIGITS must be 1..=18");
        assert!(SCALE <= DIGITS, "SCALE cannot exceed DIGITS");
    };

    #[inline]
    fn cfg() -> PackedConfig {
        let _checked_len = Self::LEN;
        PackedConfig {
            total_digits: DIGITS,
            scale: SCALE,
            is_signed: SIGNED,
        }
    }

    /// Encode using the const-generic configuration.
    ///
    /// If `value` has more fractional digits than `SCALE`, extra low-order
    /// fractional digits are truncated toward zero. Use [`Self::encode_strict`]
    /// to reject precision loss instead.
    pub fn encode(value: &Decimal) -> Result<Vec<u8>, PackedError> {
        encode_with_config(value, &Self::cfg())
    }

    /// Encode using the const-generic configuration and reject precision loss.
    pub fn encode_strict(value: &Decimal) -> Result<Vec<u8>, StrictPackedError> {
        encode_with_config_strict(value, &Self::cfg())
    }

    /// Encode directly into a caller-provided stack buffer.
    ///
    /// The buffer must have length [`Self::LEN`]. If `value` has more
    /// fractional digits than `SCALE`, extra low-order fractional digits are
    /// truncated toward zero. Use [`Self::encode_into_strict`] to reject
    /// precision loss instead.
    pub fn encode_into(value: &Decimal, out: &mut [u8]) -> Result<(), PackedError> {
        encode_into_with_config(value, &Self::cfg(), out)
    }

    /// Encode into a caller-provided stack buffer and reject precision loss.
    pub fn encode_into_strict(value: &Decimal, out: &mut [u8]) -> Result<(), StrictPackedError> {
        encode_into_with_config_strict(value, &Self::cfg(), out)
    }

    /// Encode into a fixed-size stack array.
    ///
    /// `N` must equal [`Self::LEN`]. Keeping `N` explicit avoids unstable
    /// generic-const-expr support while still allowing stack arrays on stable Rust.
    pub fn encode_array<const N: usize>(value: &Decimal) -> Result<[u8; N], PackedError> {
        if N != Self::LEN {
            return Err(PackedError::InvalidByteLength {
                expected: Self::LEN,
                actual: N,
                total_digits: DIGITS,
            });
        }
        let mut out = [0u8; N];
        Self::encode_into(value, &mut out)?;
        Ok(out)
    }

    /// Encode into a fixed-size stack array and reject precision loss.
    ///
    /// `N` must equal [`Self::LEN`].
    pub fn encode_array_strict<const N: usize>(
        value: &Decimal,
    ) -> Result<[u8; N], StrictPackedError> {
        if N != Self::LEN {
            return Err(PackedError::InvalidByteLength {
                expected: Self::LEN,
                actual: N,
                total_digits: DIGITS,
            }
            .into());
        }
        let mut out = [0u8; N];
        Self::encode_into_strict(value, &mut out)?;
        Ok(out)
    }

    /// Encode with an explicit sign nibble.
    ///
    /// If `value` has more fractional digits than `SCALE`, extra low-order
    /// fractional digits are truncated toward zero. Use
    /// [`Self::encode_with_sign_strict`] to reject precision loss instead.
    pub fn encode_with_sign(value: &Decimal, sign_nibble: u8) -> Result<Vec<u8>, PackedError> {
        encode_with_sign_config(value, &Self::cfg(), sign_nibble)
    }

    /// Encode with an explicit sign nibble and reject precision loss.
    pub fn encode_with_sign_strict(
        value: &Decimal,
        sign_nibble: u8,
    ) -> Result<Vec<u8>, StrictPackedError> {
        encode_with_sign_config_strict(value, &Self::cfg(), sign_nibble)
    }

    /// Encode with an explicit sign nibble into a caller-provided stack buffer.
    pub fn encode_with_sign_into(
        value: &Decimal,
        sign_nibble: u8,
        out: &mut [u8],
    ) -> Result<(), PackedError> {
        encode_with_sign_into_config(value, &Self::cfg(), sign_nibble, out)
    }

    /// Encode with an explicit sign nibble into a caller-provided stack buffer
    /// and reject precision loss.
    pub fn encode_with_sign_into_strict(
        value: &Decimal,
        sign_nibble: u8,
        out: &mut [u8],
    ) -> Result<(), StrictPackedError> {
        encode_with_sign_into_config_strict(value, &Self::cfg(), sign_nibble, out)
    }

    /// Encode with an explicit sign nibble into a fixed-size stack array.
    ///
    /// `N` must equal [`Self::LEN`].
    pub fn encode_with_sign_array<const N: usize>(
        value: &Decimal,
        sign_nibble: u8,
    ) -> Result<[u8; N], PackedError> {
        if N != Self::LEN {
            return Err(PackedError::InvalidByteLength {
                expected: Self::LEN,
                actual: N,
                total_digits: DIGITS,
            });
        }
        let mut out = [0u8; N];
        Self::encode_with_sign_into(value, sign_nibble, &mut out)?;
        Ok(out)
    }

    /// Encode with an explicit sign nibble into a fixed-size stack array and
    /// reject precision loss.
    ///
    /// `N` must equal [`Self::LEN`].
    pub fn encode_with_sign_array_strict<const N: usize>(
        value: &Decimal,
        sign_nibble: u8,
    ) -> Result<[u8; N], StrictPackedError> {
        if N != Self::LEN {
            return Err(PackedError::InvalidByteLength {
                expected: Self::LEN,
                actual: N,
                total_digits: DIGITS,
            }
            .into());
        }
        let mut out = [0u8; N];
        Self::encode_with_sign_into_strict(value, sign_nibble, &mut out)?;
        Ok(out)
    }

    /// Decode using the const-generic configuration.
    pub fn decode(bytes: &[u8], sign_mode: SignMode) -> Result<Decimal, PackedError> {
        decode_with_config(bytes, &Self::cfg(), sign_mode)
    }

    /// Decode directly into a caller-provided `Decimal` slot.
    pub fn decode_into(
        bytes: &[u8],
        sign_mode: SignMode,
        out: &mut Decimal,
    ) -> Result<(), PackedError> {
        *out = decode_with_config(bytes, &Self::cfg(), sign_mode)?;
        Ok(())
    }

    /// Decode losslessly using the const-generic configuration.
    pub fn decode_lossless(
        bytes: &[u8],
        sign_mode: SignMode,
    ) -> Result<LosslessDecimal, PackedError> {
        decode_lossless_with_config(bytes, &Self::cfg(), sign_mode)
    }

    /// Decode losslessly into a caller-provided slot.
    pub fn decode_lossless_into(
        bytes: &[u8],
        sign_mode: SignMode,
        out: &mut LosslessDecimal,
    ) -> Result<(), PackedError> {
        *out = decode_lossless_with_config(bytes, &Self::cfg(), sign_mode)?;
        Ok(())
    }

    /// Lossless re-encode using the const-generic configuration.
    ///
    /// If the numeric value has more fractional digits than `SCALE`, extra
    /// low-order fractional digits are truncated toward zero. Use
    /// [`Self::encode_lossless_strict`] to reject precision loss instead.
    pub fn encode_lossless(lossless: &LosslessDecimal) -> Result<Vec<u8>, PackedError> {
        encode_lossless_with_config(lossless, &Self::cfg())
    }

    /// Lossless re-encode and reject precision loss.
    pub fn encode_lossless_strict(
        lossless: &LosslessDecimal,
    ) -> Result<Vec<u8>, StrictPackedError> {
        encode_lossless_with_config_strict(lossless, &Self::cfg())
    }

    /// Lossless re-encode into a caller-provided stack buffer.
    pub fn encode_lossless_into(
        lossless: &LosslessDecimal,
        out: &mut [u8],
    ) -> Result<(), PackedError> {
        encode_lossless_into_with_config(lossless, &Self::cfg(), out)
    }

    /// Lossless re-encode into a caller-provided stack buffer and reject
    /// precision loss.
    pub fn encode_lossless_into_strict(
        lossless: &LosslessDecimal,
        out: &mut [u8],
    ) -> Result<(), StrictPackedError> {
        encode_lossless_into_with_config_strict(lossless, &Self::cfg(), out)
    }

    /// Lossless re-encode into a fixed-size stack array.
    ///
    /// `N` must equal [`Self::LEN`].
    pub fn encode_lossless_array<const N: usize>(
        lossless: &LosslessDecimal,
    ) -> Result<[u8; N], PackedError> {
        if N != Self::LEN {
            return Err(PackedError::InvalidByteLength {
                expected: Self::LEN,
                actual: N,
                total_digits: DIGITS,
            });
        }
        let mut out = [0u8; N];
        Self::encode_lossless_into(lossless, &mut out)?;
        Ok(out)
    }

    /// Lossless re-encode into a fixed-size stack array and reject precision
    /// loss.
    ///
    /// `N` must equal [`Self::LEN`].
    pub fn encode_lossless_array_strict<const N: usize>(
        lossless: &LosslessDecimal,
    ) -> Result<[u8; N], StrictPackedError> {
        if N != Self::LEN {
            return Err(PackedError::InvalidByteLength {
                expected: Self::LEN,
                actual: N,
                total_digits: DIGITS,
            }
            .into());
        }
        let mut out = [0u8; N];
        Self::encode_lossless_into_strict(lossless, &mut out)?;
        Ok(out)
    }

    /// Byte length for this packed field.
    #[inline]
    #[deprecated(note = "use LEN instead")]
    pub const fn len() -> usize {
        expected_len(DIGITS)
    }
}

/// All codec errors.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum PackedError {
    /// Byte length mismatch.
    #[error("invalid byte length: expected {expected} for {total_digits} digits, got {actual}")]
    InvalidByteLength {
        /// Expected packed byte length.
        expected: usize,
        /// Actual byte length.
        actual: usize,
        /// Configured total digit capacity.
        total_digits: u8,
    },

    /// Invalid `total_digits`.
    #[error("total_digits must be 1..=18, got {0}")]
    InvalidTotalDigits(u8),

    /// Scale exceeds digits.
    #[error("scale ({scale}) cannot exceed total_digits ({total_digits})")]
    ScaleExceedsTotalDigits {
        /// Requested implied decimal scale.
        scale: u8,
        /// Configured total digit capacity.
        total_digits: u8,
    },

    /// Decimal scale is too large.
    ///
    /// This variant is retained for source compatibility. Current configs
    /// reject scale through [`PackedError::ScaleExceedsTotalDigits`] because
    /// `total_digits` is capped at 18.
    #[error("scale ({0}) exceeds the supported packed-decimal scale")]
    ScaleTooLargeForDecimal(u8),

    /// A digit nibble was outside 0..=9.
    #[error("invalid digit nibble 0x{nibble:02X} at digit position {position}")]
    InvalidDigitNibble {
        /// Zero-based digit position in the packed field.
        position: usize,
        /// Invalid digit nibble value.
        nibble: u8,
    },

    /// A sign nibble is not valid for the configured sign mode.
    #[error("invalid sign nibble 0x{nibble:02X} (sign_mode={sign_mode:?}, is_signed={is_signed})")]
    InvalidSignNibble {
        /// Invalid sign nibble value.
        nibble: u8,
        /// Sign interpretation mode used during decode.
        sign_mode: SignMode,
        /// Whether the field was configured as signed.
        is_signed: bool,
    },

    /// Negative nibble found in unsigned field.
    #[error("negative sign nibble 0x{nibble:02X} not allowed in unsigned field")]
    NegativeInUnsigned {
        /// Negative sign nibble value found in an unsigned field.
        nibble: u8,
    },

    /// Leading pad nibble must be zero for even digit counts.
    #[error("invalid padding nibble 0x{nibble:02X}, expected 0x0 for even total_digits")]
    InvalidPaddingNibble {
        /// Invalid leading padding nibble.
        nibble: u8,
    },

    /// Value overflows the digit capacity.
    #[error("value overflows {max_digits}-digit field")]
    Overflow {
        /// Maximum configured digit capacity.
        max_digits: u8,
    },

    /// Arithmetic overflow occurred while scaling.
    #[error("arithmetic overflow while scaling mantissa (target_scale={target_scale}, current_scale={current_scale})")]
    ArithmeticOverflow {
        /// Requested output scale.
        target_scale: u32,
        /// Current decimal scale.
        current_scale: u32,
    },

    /// Negative value supplied to unsigned field.
    #[error("negative value not allowed in unsigned field")]
    NegativeUnsigned,

    /// Explicit sign override is incompatible with the numeric value.
    #[error("sign nibble 0x{nibble:02X} mismatch: signed={is_signed}, negative={is_negative}")]
    InvalidSignOverride {
        /// Requested sign nibble.
        nibble: u8,
        /// Whether the field was configured as signed.
        is_signed: bool,
        /// Whether the numeric value is negative.
        is_negative: bool,
    },

    /// `Decimal::MIN` style absolute overflow guard.
    #[error("absolute value overflow for Decimal::MIN")]
    AbsoluteOverflow,
}

/// Strict encoding errors.
///
/// Strict APIs preserve codec compatibility while giving migration and
/// forensic callers an explicit way to reject fractional precision loss.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum StrictPackedError {
    /// Standard packed-decimal codec error.
    #[error(transparent)]
    Codec(#[from] PackedError),

    /// Encoding would drop non-zero low-order fractional digits.
    #[error(
        "precision loss while scaling mantissa (target_scale={target_scale}, current_scale={current_scale}, dropped_digit_count={dropped_digit_count})"
    )]
    PrecisionLoss {
        /// Requested output scale.
        target_scale: u32,
        /// Current decimal scale.
        current_scale: u32,
        /// Number of low-order fractional digit positions that would be dropped.
        dropped_digit_count: u32,
    },
}

/// A decoded value and its original sign nibble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LosslessDecimal {
    /// The numeric value.
    pub value: Decimal,
    /// Original sign nibble.
    pub sign_nibble: u8,
}

/// Expected packed byte length for the given digit count.
#[inline]
pub const fn expected_len(total_digits: u8) -> usize {
    (total_digits as usize / 2) + 1
}

/// Extract a single nibble directly from the packed byte slice.
#[inline]
fn nibble_at(bytes: &[u8], nibble_index: usize) -> u8 {
    let byte = bytes[nibble_index / 2];
    if nibble_index % 2 == 0 {
        byte >> 4
    } else {
        byte & 0x0F
    }
}

/// Zero-allocation nibble stream over a packed byte slice.
#[derive(Debug, Clone)]
pub struct NibbleIter<'a> {
    bytes: &'a [u8],
    nibble_index: usize,
}

impl<'a> NibbleIter<'a> {
    /// Create a new nibble iterator.
    #[inline]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            nibble_index: 0,
        }
    }
}

impl<'a> Iterator for NibbleIter<'a> {
    type Item = u8;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.nibble_index >= self.bytes.len() * 2 {
            return None;
        }
        let nib = nibble_at(self.bytes, self.nibble_index);
        self.nibble_index += 1;
        Some(nib)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.bytes.len() * 2 - self.nibble_index;
        (remaining, Some(remaining))
    }
}

impl<'a> ExactSizeIterator for NibbleIter<'a> {}

/// Construct a lazy nibble stream without allocating.
#[inline]
pub fn nibble_iter(bytes: &[u8]) -> NibbleIter<'_> {
    NibbleIter::new(bytes)
}

/// Scalar nibble expansion used as the reference implementation.
#[inline]
#[cfg(feature = "simd")]
fn scalar_expand_nibbles(bytes: &[u8]) -> Vec<u8> {
    nibble_iter(bytes).collect()
}

/// Expand nibbles without allocation by streaming them through an iterator.
#[inline]
pub fn stream_nibbles(bytes: &[u8]) -> NibbleIter<'_> {
    nibble_iter(bytes)
}

/// Validate the SIMD nibble expander against the scalar reference when the SIMD feature is enabled.
///
/// With the feature disabled, this returns `true` because no SIMD path is compiled.
#[inline]
pub fn simd_matches_scalar(bytes: &[u8]) -> bool {
    #[cfg(feature = "simd")]
    {
        simd::expand_nibbles(bytes) == scalar_expand_nibbles(bytes)
    }
    #[cfg(not(feature = "simd"))]
    {
        let _ = bytes;
        true
    }
}

#[inline]
fn canonical_sign(is_signed: bool, is_negative: bool, is_zero: bool) -> u8 {
    if is_zero {
        if is_signed {
            0x0C
        } else {
            0x0F
        }
    } else if is_signed {
        if is_negative {
            0x0D
        } else {
            0x0C
        }
    } else {
        0x0F
    }
}

#[inline]
fn digit_max(total_digits: u8) -> i128 {
    pow10(total_digits as u32).map_or(i128::MAX, |value| value - 1)
}

fn split_decimal(value: &Decimal) -> Result<(i128, bool), PackedError> {
    let mant = value.mantissa();
    if mant == i128::MIN {
        return Err(PackedError::AbsoluteOverflow);
    }
    Ok((
        if mant < 0 { -mant } else { mant },
        mant < 0 || value.is_sign_negative(),
    ))
}

fn scale_mantissa_common(
    value: &Decimal,
    cfg: &PackedConfig,
    reject_precision_loss: bool,
) -> Result<(i128, bool), StrictPackedError> {
    let (abs_mant, is_negative) = split_decimal(value)?;
    let current_scale = value.scale();
    let target_scale = cfg.scale as u32;

    let scaled = if target_scale >= current_scale {
        let diff = target_scale - current_scale;
        let factor = pow10(diff).ok_or(PackedError::ArithmeticOverflow {
            target_scale,
            current_scale,
        })?;
        abs_mant
            .checked_mul(factor)
            .ok_or(PackedError::ArithmeticOverflow {
                target_scale,
                current_scale,
            })?
    } else {
        let diff = current_scale - target_scale;
        let divisor = pow10(diff).ok_or(PackedError::ArithmeticOverflow {
            target_scale,
            current_scale,
        })?;
        if reject_precision_loss && abs_mant % divisor != 0 {
            return Err(StrictPackedError::PrecisionLoss {
                target_scale,
                current_scale,
                dropped_digit_count: diff,
            });
        }
        abs_mant / divisor
    };

    if scaled > digit_max(cfg.total_digits) {
        return Err(PackedError::Overflow {
            max_digits: cfg.total_digits,
        }
        .into());
    }
    Ok((scaled, is_negative))
}

fn scale_mantissa(value: &Decimal, cfg: &PackedConfig) -> Result<(i128, bool), PackedError> {
    match scale_mantissa_common(value, cfg, false) {
        Ok(scaled) => Ok(scaled),
        Err(StrictPackedError::Codec(err)) => Err(err),
        Err(StrictPackedError::PrecisionLoss {
            target_scale,
            current_scale,
            ..
        }) => Err(PackedError::ArithmeticOverflow {
            target_scale,
            current_scale,
        }),
    }
}

fn scale_mantissa_strict(
    value: &Decimal,
    cfg: &PackedConfig,
) -> Result<(i128, bool), StrictPackedError> {
    scale_mantissa_common(value, cfg, true)
}

fn validate_sign_override(
    sign: u8,
    is_signed: bool,
    is_negative: bool,
    is_zero: bool,
) -> Result<(), PackedError> {
    if is_signed {
        if is_zero {
            if matches!(sign, 0xA..=0xF) {
                return Ok(());
            }
        } else if (!is_negative && matches!(sign, 0xA | 0xC | 0xE | 0xF))
            || (is_negative && matches!(sign, 0xB | 0xD))
        {
            return Ok(());
        }
    } else if !is_negative && matches!(sign, 0xA | 0xC | 0xE | 0xF) {
        return Ok(());
    }
    Err(PackedError::InvalidSignOverride {
        nibble: sign,
        is_signed,
        is_negative,
    })
}

fn pack_integer(
    mut value: i128,
    buf: &mut [u8],
    sign: u8,
    total_digits: u8,
) -> Result<(), PackedError> {
    let td = total_digits as usize;
    let byte_len = expected_len(total_digits);
    if buf.len() != byte_len {
        return Err(PackedError::InvalidByteLength {
            expected: byte_len,
            actual: buf.len(),
            total_digits,
        });
    }

    let mut digits = [0u8; 18];
    for idx in (0..td).rev() {
        digits[idx] = (value % 10) as u8;
        value /= 10;
    }

    // Nibble sequence: optional leading pad, all digits, sign nibble.
    let mut nibbles = [0u8; 20];
    let offset = if total_digits % 2 == 0 { 1 } else { 0 };
    if offset == 1 {
        nibbles[0] = 0;
    }
    nibbles[offset..(td + offset)].copy_from_slice(&digits[..td]);
    nibbles[offset + td] = sign;

    for i in 0..byte_len {
        buf[i] = (nibbles[2 * i] << 4) | nibbles[2 * i + 1];
    }
    Ok(())
}

fn decode_scalar_core(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
) -> Result<(i128, u8), PackedError> {
    let expected = expected_len(cfg.total_digits);
    if bytes.len() != expected {
        return Err(PackedError::InvalidByteLength {
            expected,
            actual: bytes.len(),
            total_digits: cfg.total_digits,
        });
    }

    let mut nibbles = nibble_iter(bytes);
    if cfg.total_digits % 2 == 0 {
        let pad = nibbles.next().unwrap_or(0);
        if pad != 0 {
            return Err(PackedError::InvalidPaddingNibble { nibble: pad });
        }
    }

    let mut accum: i128 = 0;
    for i in 0..cfg.total_digits as usize {
        let nib = nibbles.next().ok_or(PackedError::InvalidByteLength {
            expected,
            actual: bytes.len(),
            total_digits: cfg.total_digits,
        })?;
        if nib > 9 {
            return Err(PackedError::InvalidDigitNibble {
                position: i,
                nibble: nib,
            });
        }
        accum = accum * 10 + i128::from(nib);
    }

    let sign_nibble = nibbles.next().ok_or(PackedError::InvalidByteLength {
        expected,
        actual: bytes.len(),
        total_digits: cfg.total_digits,
    })?;
    let is_negative = match sign_mode {
        SignMode::Pfd => match (sign_nibble, cfg.is_signed) {
            (0xC, _) => false,
            (0xD, true) => true,
            (0xD, false) => {
                return Err(PackedError::NegativeInUnsigned {
                    nibble: sign_nibble,
                })
            }
            (0xF, false) => false,
            _ => {
                return Err(PackedError::InvalidSignNibble {
                    nibble: sign_nibble,
                    sign_mode,
                    is_signed: cfg.is_signed,
                })
            }
        },
        SignMode::Nopfd => match sign_nibble {
            0xA | 0xC | 0xE | 0xF => false,
            0xB | 0xD => {
                if !cfg.is_signed {
                    return Err(PackedError::NegativeInUnsigned {
                        nibble: sign_nibble,
                    });
                }
                true
            }
            _ => {
                return Err(PackedError::InvalidSignNibble {
                    nibble: sign_nibble,
                    sign_mode,
                    is_signed: cfg.is_signed,
                })
            }
        },
    };

    let signed = if is_negative { -accum } else { accum };
    Ok((signed, sign_nibble))
}

fn decode_core(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
) -> Result<(i128, u8), PackedError> {
    let expected = expected_len(cfg.total_digits);
    if bytes.len() != expected {
        return Err(PackedError::InvalidByteLength {
            expected,
            actual: bytes.len(),
            total_digits: cfg.total_digits,
        });
    }
    #[cfg(feature = "simd")]
    {
        debug_assert!(
            simd_matches_scalar(bytes),
            "SIMD nibble expansion diverged from scalar reference"
        );
    }
    decode_scalar_core(bytes, cfg, sign_mode)
}

fn decode_with_config(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
) -> Result<Decimal, PackedError> {
    let (signed, _) = decode_core(bytes, cfg, sign_mode)?;
    Ok(Decimal::from_i128_with_scale(signed, cfg.scale as u32))
}

fn decode_lossless_with_config(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
) -> Result<LosslessDecimal, PackedError> {
    let (signed, sign_nibble) = decode_core(bytes, cfg, sign_mode)?;
    Ok(LosslessDecimal {
        value: Decimal::from_i128_with_scale(signed, cfg.scale as u32),
        sign_nibble,
    })
}

fn encode_into_with_policy(
    value: &Decimal,
    cfg: &PackedConfig,
    zero_sign: ZeroSignPolicy,
    out: &mut [u8],
) -> Result<(), PackedError> {
    let expected = expected_len(cfg.total_digits);
    if out.len() != expected {
        return Err(PackedError::InvalidByteLength {
            expected,
            actual: out.len(),
            total_digits: cfg.total_digits,
        });
    }
    let (scaled, is_negative) = scale_mantissa(value, cfg)?;
    if is_negative && !cfg.is_signed {
        return Err(PackedError::NegativeUnsigned);
    }
    let is_zero = scaled == 0;
    let sign = match zero_sign {
        ZeroSignPolicy::Canonical => canonical_sign(cfg.is_signed, is_negative, is_zero),
        ZeroSignPolicy::Preserve => {
            if is_zero {
                if cfg.is_signed {
                    if is_negative {
                        0x0D
                    } else {
                        0x0C
                    }
                } else {
                    0x0F
                }
            } else {
                canonical_sign(cfg.is_signed, is_negative, false)
            }
        }
    };
    pack_integer(scaled, out, sign, cfg.total_digits)?;
    Ok(())
}

fn encode_into_with_config(
    value: &Decimal,
    cfg: &PackedConfig,
    out: &mut [u8],
) -> Result<(), PackedError> {
    encode_into_with_policy(value, cfg, ZeroSignPolicy::Canonical, out)
}

fn encode_with_sign_into_policy(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    zero_sign: ZeroSignPolicy,
    out: &mut [u8],
) -> Result<(), PackedError> {
    let expected = expected_len(cfg.total_digits);
    if out.len() != expected {
        return Err(PackedError::InvalidByteLength {
            expected,
            actual: out.len(),
            total_digits: cfg.total_digits,
        });
    }
    let (scaled, is_negative) = scale_mantissa(value, cfg)?;
    if is_negative && !cfg.is_signed {
        return Err(PackedError::NegativeUnsigned);
    }
    let is_zero = scaled == 0;
    validate_sign_override(sign_nibble, cfg.is_signed, is_negative, is_zero)?;
    let sign = match zero_sign {
        ZeroSignPolicy::Canonical if is_zero => canonical_sign(cfg.is_signed, is_negative, is_zero),
        _ => sign_nibble,
    };
    pack_integer(scaled, out, sign, cfg.total_digits)?;
    Ok(())
}

fn encode_with_sign_into_config(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    out: &mut [u8],
) -> Result<(), PackedError> {
    encode_with_sign_into_policy(value, cfg, sign_nibble, ZeroSignPolicy::Preserve, out)
}

fn encode_with_policy(
    value: &Decimal,
    cfg: &PackedConfig,
    zero_sign: ZeroSignPolicy,
) -> Result<Vec<u8>, PackedError> {
    let (scaled, is_negative) = scale_mantissa(value, cfg)?;
    if is_negative && !cfg.is_signed {
        return Err(PackedError::NegativeUnsigned);
    }
    let is_zero = scaled == 0;
    let sign = match zero_sign {
        ZeroSignPolicy::Canonical => canonical_sign(cfg.is_signed, is_negative, is_zero),
        ZeroSignPolicy::Preserve => {
            if is_zero {
                if cfg.is_signed {
                    if is_negative {
                        0x0D
                    } else {
                        0x0C
                    }
                } else {
                    0x0F
                }
            } else {
                canonical_sign(cfg.is_signed, is_negative, false)
            }
        }
    };
    let mut out = vec![0u8; expected_len(cfg.total_digits)];
    pack_integer(scaled, &mut out, sign, cfg.total_digits)?;
    Ok(out)
}

fn encode_with_config(value: &Decimal, cfg: &PackedConfig) -> Result<Vec<u8>, PackedError> {
    encode_with_policy(value, cfg, ZeroSignPolicy::Canonical)
}

fn encode_with_sign_policy(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    zero_sign: ZeroSignPolicy,
) -> Result<Vec<u8>, PackedError> {
    let (scaled, is_negative) = scale_mantissa(value, cfg)?;
    if is_negative && !cfg.is_signed {
        return Err(PackedError::NegativeUnsigned);
    }
    let is_zero = scaled == 0;
    validate_sign_override(sign_nibble, cfg.is_signed, is_negative, is_zero)?;
    let sign = match zero_sign {
        ZeroSignPolicy::Canonical if is_zero => canonical_sign(cfg.is_signed, is_negative, is_zero),
        _ => sign_nibble,
    };
    let mut out = vec![0u8; expected_len(cfg.total_digits)];
    pack_integer(scaled, &mut out, sign, cfg.total_digits)?;
    Ok(out)
}

fn encode_with_sign_config(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
) -> Result<Vec<u8>, PackedError> {
    encode_with_sign_policy(value, cfg, sign_nibble, ZeroSignPolicy::Preserve)
}

fn encode_lossless_with_policy(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    zero_sign: ZeroSignPolicy,
) -> Result<Vec<u8>, PackedError> {
    let (scaled, is_negative) = scale_mantissa(&lossless.value, cfg)?;
    if is_negative && !cfg.is_signed {
        return Err(PackedError::NegativeUnsigned);
    }
    let is_zero = scaled == 0;
    validate_sign_override(lossless.sign_nibble, cfg.is_signed, is_negative, is_zero)?;
    let sign = match zero_sign {
        ZeroSignPolicy::Canonical if is_zero => canonical_sign(cfg.is_signed, is_negative, is_zero),
        _ => lossless.sign_nibble,
    };
    let mut out = vec![0u8; expected_len(cfg.total_digits)];
    pack_integer(scaled, &mut out, sign, cfg.total_digits)?;
    Ok(out)
}

fn encode_lossless_with_config(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
) -> Result<Vec<u8>, PackedError> {
    encode_lossless_with_policy(lossless, cfg, ZeroSignPolicy::Preserve)
}

fn encode_lossless_into_with_policy(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    zero_sign: ZeroSignPolicy,
    out: &mut [u8],
) -> Result<(), PackedError> {
    let expected = expected_len(cfg.total_digits);
    if out.len() != expected {
        return Err(PackedError::InvalidByteLength {
            expected,
            actual: out.len(),
            total_digits: cfg.total_digits,
        });
    }
    let (scaled, is_negative) = scale_mantissa(&lossless.value, cfg)?;
    if is_negative && !cfg.is_signed {
        return Err(PackedError::NegativeUnsigned);
    }
    let is_zero = scaled == 0;
    validate_sign_override(lossless.sign_nibble, cfg.is_signed, is_negative, is_zero)?;
    let sign = match zero_sign {
        ZeroSignPolicy::Canonical if is_zero => canonical_sign(cfg.is_signed, is_negative, is_zero),
        _ => lossless.sign_nibble,
    };
    pack_integer(scaled, out, sign, cfg.total_digits)?;
    Ok(())
}

fn encode_lossless_into_with_config(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    out: &mut [u8],
) -> Result<(), PackedError> {
    encode_lossless_into_with_policy(lossless, cfg, ZeroSignPolicy::Preserve, out)
}

fn encode_into_with_policy_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    zero_sign: ZeroSignPolicy,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    let expected = expected_len(cfg.total_digits);
    if out.len() != expected {
        return Err(PackedError::InvalidByteLength {
            expected,
            actual: out.len(),
            total_digits: cfg.total_digits,
        }
        .into());
    }
    let (scaled, is_negative) = scale_mantissa_strict(value, cfg)?;
    if is_negative && !cfg.is_signed {
        return Err(PackedError::NegativeUnsigned.into());
    }
    let is_zero = scaled == 0;
    let sign = match zero_sign {
        ZeroSignPolicy::Canonical => canonical_sign(cfg.is_signed, is_negative, is_zero),
        ZeroSignPolicy::Preserve => {
            if is_zero {
                if cfg.is_signed {
                    if is_negative {
                        0x0D
                    } else {
                        0x0C
                    }
                } else {
                    0x0F
                }
            } else {
                canonical_sign(cfg.is_signed, is_negative, false)
            }
        }
    };
    pack_integer(scaled, out, sign, cfg.total_digits)?;
    Ok(())
}

fn encode_into_with_config_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    encode_into_with_policy_strict(value, cfg, ZeroSignPolicy::Canonical, out)
}

fn encode_with_policy_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    zero_sign: ZeroSignPolicy,
) -> Result<Vec<u8>, StrictPackedError> {
    let mut out = vec![0u8; expected_len(cfg.total_digits)];
    encode_into_with_policy_strict(value, cfg, zero_sign, &mut out)?;
    Ok(out)
}

fn encode_with_config_strict(
    value: &Decimal,
    cfg: &PackedConfig,
) -> Result<Vec<u8>, StrictPackedError> {
    encode_with_policy_strict(value, cfg, ZeroSignPolicy::Canonical)
}

fn encode_with_sign_into_policy_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    zero_sign: ZeroSignPolicy,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    let expected = expected_len(cfg.total_digits);
    if out.len() != expected {
        return Err(PackedError::InvalidByteLength {
            expected,
            actual: out.len(),
            total_digits: cfg.total_digits,
        }
        .into());
    }
    let (scaled, is_negative) = scale_mantissa_strict(value, cfg)?;
    if is_negative && !cfg.is_signed {
        return Err(PackedError::NegativeUnsigned.into());
    }
    let is_zero = scaled == 0;
    validate_sign_override(sign_nibble, cfg.is_signed, is_negative, is_zero)?;
    let sign = match zero_sign {
        ZeroSignPolicy::Canonical if is_zero => canonical_sign(cfg.is_signed, is_negative, is_zero),
        _ => sign_nibble,
    };
    pack_integer(scaled, out, sign, cfg.total_digits)?;
    Ok(())
}

fn encode_with_sign_into_config_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    encode_with_sign_into_policy_strict(value, cfg, sign_nibble, ZeroSignPolicy::Preserve, out)
}

fn encode_with_sign_policy_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    zero_sign: ZeroSignPolicy,
) -> Result<Vec<u8>, StrictPackedError> {
    let mut out = vec![0u8; expected_len(cfg.total_digits)];
    encode_with_sign_into_policy_strict(value, cfg, sign_nibble, zero_sign, &mut out)?;
    Ok(out)
}

fn encode_with_sign_config_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
) -> Result<Vec<u8>, StrictPackedError> {
    encode_with_sign_policy_strict(value, cfg, sign_nibble, ZeroSignPolicy::Preserve)
}

fn encode_lossless_into_with_policy_strict(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    zero_sign: ZeroSignPolicy,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    let expected = expected_len(cfg.total_digits);
    if out.len() != expected {
        return Err(PackedError::InvalidByteLength {
            expected,
            actual: out.len(),
            total_digits: cfg.total_digits,
        }
        .into());
    }
    let (scaled, is_negative) = scale_mantissa_strict(&lossless.value, cfg)?;
    if is_negative && !cfg.is_signed {
        return Err(PackedError::NegativeUnsigned.into());
    }
    let is_zero = scaled == 0;
    validate_sign_override(lossless.sign_nibble, cfg.is_signed, is_negative, is_zero)?;
    let sign = match zero_sign {
        ZeroSignPolicy::Canonical if is_zero => canonical_sign(cfg.is_signed, is_negative, is_zero),
        _ => lossless.sign_nibble,
    };
    pack_integer(scaled, out, sign, cfg.total_digits)?;
    Ok(())
}

fn encode_lossless_into_with_config_strict(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    encode_lossless_into_with_policy_strict(lossless, cfg, ZeroSignPolicy::Preserve, out)
}

fn encode_lossless_with_policy_strict(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    zero_sign: ZeroSignPolicy,
) -> Result<Vec<u8>, StrictPackedError> {
    let mut out = vec![0u8; expected_len(cfg.total_digits)];
    encode_lossless_into_with_policy_strict(lossless, cfg, zero_sign, &mut out)?;
    Ok(out)
}

fn encode_lossless_with_config_strict(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
) -> Result<Vec<u8>, StrictPackedError> {
    encode_lossless_with_policy_strict(lossless, cfg, ZeroSignPolicy::Preserve)
}

/// Decode a packed-decimal byte slice using the scalar reference decoder.
pub fn from_packed_scalar(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
) -> Result<Decimal, PackedError> {
    decode_scalar_core(bytes, cfg, sign_mode)
        .map(|(signed, _)| Decimal::from_i128_with_scale(signed, cfg.scale as u32))
}

/// Decode a packed-decimal byte slice using the scalar reference decoder into a caller-provided slot.
pub fn from_packed_scalar_into(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
    out: &mut Decimal,
) -> Result<(), PackedError> {
    *out = from_packed_scalar(bytes, cfg, sign_mode)?;
    Ok(())
}

/// Decode a packed-decimal byte slice with an explicit policy object.
///
/// Only [`PackedPolicy::sign_mode`] affects decoding. [`PackedPolicy::zero_sign`]
/// is encode-only and is intentionally ignored here.
pub fn from_packed_with_policy(
    bytes: &[u8],
    cfg: &PackedConfig,
    policy: PackedPolicy,
) -> Result<Decimal, PackedError> {
    decode_with_config(bytes, cfg, policy.sign_mode)
}

/// Decode a packed-decimal byte slice.
pub fn from_packed(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
) -> Result<Decimal, PackedError> {
    decode_with_config(bytes, cfg, sign_mode)
}

/// Decode a packed-decimal byte slice into a caller-provided `Decimal` slot.
pub fn from_packed_into(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
    out: &mut Decimal,
) -> Result<(), PackedError> {
    *out = decode_with_config(bytes, cfg, sign_mode)?;
    Ok(())
}

/// Decode a packed-decimal byte slice, preserving the original sign nibble.
pub fn from_packed_lossless(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
) -> Result<LosslessDecimal, PackedError> {
    decode_lossless_with_config(bytes, cfg, sign_mode)
}

/// Decode a packed-decimal byte slice losslessly into a caller-provided slot.
pub fn from_packed_lossless_into(
    bytes: &[u8],
    cfg: &PackedConfig,
    sign_mode: SignMode,
    out: &mut LosslessDecimal,
) -> Result<(), PackedError> {
    *out = decode_lossless_with_config(bytes, cfg, sign_mode)?;
    Ok(())
}

/// Decode a packed-decimal byte slice losslessly under an explicit policy object.
///
/// Only [`PackedPolicy::sign_mode`] affects decoding. [`PackedPolicy::zero_sign`]
/// is encode-only and is intentionally ignored here.
pub fn from_packed_lossless_with_policy(
    bytes: &[u8],
    cfg: &PackedConfig,
    policy: PackedPolicy,
) -> Result<LosslessDecimal, PackedError> {
    decode_lossless_with_config(bytes, cfg, policy.sign_mode)
}

/// Encode a decimal to packed bytes using canonical signs.
///
/// If `value` has more fractional digits than `cfg.scale()`, extra low-order
/// fractional digits are truncated toward zero. Use [`to_packed_strict`] to
/// reject precision loss instead.
pub fn to_packed(value: &Decimal, cfg: &PackedConfig) -> Result<Vec<u8>, PackedError> {
    encode_with_config(value, cfg)
}

/// Encode a decimal to packed bytes using canonical signs and reject precision loss.
pub fn to_packed_strict(value: &Decimal, cfg: &PackedConfig) -> Result<Vec<u8>, StrictPackedError> {
    encode_with_config_strict(value, cfg)
}

/// Encode a decimal to packed bytes under an explicit zero/sign policy.
///
/// If `value` has more fractional digits than `cfg.scale()`, extra low-order
/// fractional digits are truncated toward zero. Use
/// [`to_packed_with_policy_strict`] to reject precision loss instead.
pub fn to_packed_with_policy(
    value: &Decimal,
    cfg: &PackedConfig,
    policy: PackedPolicy,
) -> Result<Vec<u8>, PackedError> {
    encode_with_policy(value, cfg, policy.zero_sign)
}

/// Encode a decimal under an explicit zero/sign policy and reject precision loss.
pub fn to_packed_with_policy_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    policy: PackedPolicy,
) -> Result<Vec<u8>, StrictPackedError> {
    encode_with_policy_strict(value, cfg, policy.zero_sign)
}

/// Encode a decimal into a caller-provided stack buffer.
///
/// If `value` has more fractional digits than `cfg.scale()`, extra low-order
/// fractional digits are truncated toward zero. Use [`to_packed_into_strict`]
/// to reject precision loss instead.
pub fn to_packed_into(
    value: &Decimal,
    cfg: &PackedConfig,
    out: &mut [u8],
) -> Result<(), PackedError> {
    encode_into_with_config(value, cfg, out)
}

/// Encode a decimal into a caller-provided stack buffer and reject precision loss.
pub fn to_packed_into_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    encode_into_with_config_strict(value, cfg, out)
}

/// Encode a decimal into a caller-provided stack buffer under an explicit policy.
///
/// If `value` has more fractional digits than `cfg.scale()`, extra low-order
/// fractional digits are truncated toward zero. Use
/// [`to_packed_into_with_policy_strict`] to reject precision loss instead.
pub fn to_packed_into_with_policy(
    value: &Decimal,
    cfg: &PackedConfig,
    policy: PackedPolicy,
    out: &mut [u8],
) -> Result<(), PackedError> {
    encode_into_with_policy(value, cfg, policy.zero_sign, out)
}

/// Encode into a caller-provided stack buffer under an explicit policy and
/// reject precision loss.
pub fn to_packed_into_with_policy_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    policy: PackedPolicy,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    encode_into_with_policy_strict(value, cfg, policy.zero_sign, out)
}

/// Encode with an explicit sign nibble.
///
/// If `value` has more fractional digits than `cfg.scale()`, extra low-order
/// fractional digits are truncated toward zero. Use
/// [`to_packed_with_sign_strict`] to reject precision loss instead.
pub fn to_packed_with_sign(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
) -> Result<Vec<u8>, PackedError> {
    encode_with_sign_config(value, cfg, sign_nibble)
}

/// Encode with an explicit sign nibble and reject precision loss.
pub fn to_packed_with_sign_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
) -> Result<Vec<u8>, StrictPackedError> {
    encode_with_sign_config_strict(value, cfg, sign_nibble)
}

/// Encode with an explicit sign nibble and zero/sign policy.
///
/// If `value` has more fractional digits than `cfg.scale()`, extra low-order
/// fractional digits are truncated toward zero. Use
/// [`to_packed_with_sign_with_policy_strict`] to reject precision loss instead.
pub fn to_packed_with_sign_with_policy(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    policy: PackedPolicy,
) -> Result<Vec<u8>, PackedError> {
    encode_with_sign_policy(value, cfg, sign_nibble, policy.zero_sign)
}

/// Encode with an explicit sign nibble and zero/sign policy, rejecting precision loss.
pub fn to_packed_with_sign_with_policy_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    policy: PackedPolicy,
) -> Result<Vec<u8>, StrictPackedError> {
    encode_with_sign_policy_strict(value, cfg, sign_nibble, policy.zero_sign)
}

/// Encode with an explicit sign nibble into a caller-provided stack buffer.
///
/// If `value` has more fractional digits than `cfg.scale()`, extra low-order
/// fractional digits are truncated toward zero. Use
/// [`to_packed_with_sign_into_strict`] to reject precision loss instead.
pub fn to_packed_with_sign_into(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    out: &mut [u8],
) -> Result<(), PackedError> {
    encode_with_sign_into_config(value, cfg, sign_nibble, out)
}

/// Encode with an explicit sign nibble into a caller-provided stack buffer and
/// reject precision loss.
pub fn to_packed_with_sign_into_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    encode_with_sign_into_config_strict(value, cfg, sign_nibble, out)
}

/// Encode with an explicit sign nibble into a caller-provided stack buffer and explicit policy.
///
/// If `value` has more fractional digits than `cfg.scale()`, extra low-order
/// fractional digits are truncated toward zero. Use
/// [`to_packed_with_sign_into_with_policy_strict`] to reject precision loss
/// instead.
pub fn to_packed_with_sign_into_with_policy(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    policy: PackedPolicy,
    out: &mut [u8],
) -> Result<(), PackedError> {
    encode_with_sign_into_policy(value, cfg, sign_nibble, policy.zero_sign, out)
}

/// Encode with an explicit sign nibble into a caller-provided stack buffer and
/// explicit policy, rejecting precision loss.
pub fn to_packed_with_sign_into_with_policy_strict(
    value: &Decimal,
    cfg: &PackedConfig,
    sign_nibble: u8,
    policy: PackedPolicy,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    encode_with_sign_into_policy_strict(value, cfg, sign_nibble, policy.zero_sign, out)
}

/// Lossless re-encode using the original sign nibble.
///
/// If the numeric value has more fractional digits than `cfg.scale()`, extra
/// low-order fractional digits are truncated toward zero. Use
/// [`to_packed_lossless_strict`] to reject precision loss instead.
pub fn to_packed_lossless(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
) -> Result<Vec<u8>, PackedError> {
    encode_lossless_with_config(lossless, cfg)
}

/// Lossless re-encode using the original sign nibble and reject precision loss.
pub fn to_packed_lossless_strict(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
) -> Result<Vec<u8>, StrictPackedError> {
    encode_lossless_with_config_strict(lossless, cfg)
}

/// Lossless re-encode under an explicit policy object.
///
/// If the numeric value has more fractional digits than `cfg.scale()`, extra
/// low-order fractional digits are truncated toward zero. Use
/// [`to_packed_lossless_with_policy_strict`] to reject precision loss instead.
pub fn to_packed_lossless_with_policy(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    policy: PackedPolicy,
) -> Result<Vec<u8>, PackedError> {
    encode_lossless_with_policy(lossless, cfg, policy.zero_sign)
}

/// Lossless re-encode under an explicit policy object and reject precision loss.
pub fn to_packed_lossless_with_policy_strict(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    policy: PackedPolicy,
) -> Result<Vec<u8>, StrictPackedError> {
    encode_lossless_with_policy_strict(lossless, cfg, policy.zero_sign)
}

/// Lossless re-encode into a caller-provided stack buffer.
///
/// If the numeric value has more fractional digits than `cfg.scale()`, extra
/// low-order fractional digits are truncated toward zero. Use
/// [`to_packed_lossless_into_strict`] to reject precision loss instead.
pub fn to_packed_lossless_into(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    out: &mut [u8],
) -> Result<(), PackedError> {
    encode_lossless_into_with_config(lossless, cfg, out)
}

/// Lossless re-encode into a caller-provided stack buffer and reject precision loss.
pub fn to_packed_lossless_into_strict(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    encode_lossless_into_with_config_strict(lossless, cfg, out)
}

/// Lossless re-encode into a caller-provided stack buffer under an explicit policy object.
///
/// If the numeric value has more fractional digits than `cfg.scale()`, extra
/// low-order fractional digits are truncated toward zero. Use
/// [`to_packed_lossless_into_with_policy_strict`] to reject precision loss
/// instead.
pub fn to_packed_lossless_into_with_policy(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    policy: PackedPolicy,
    out: &mut [u8],
) -> Result<(), PackedError> {
    encode_lossless_into_with_policy(lossless, cfg, policy.zero_sign, out)
}

/// Lossless re-encode into a caller-provided stack buffer under an explicit
/// policy object and reject precision loss.
pub fn to_packed_lossless_into_with_policy_strict(
    lossless: &LosslessDecimal,
    cfg: &PackedConfig,
    policy: PackedPolicy,
    out: &mut [u8],
) -> Result<(), StrictPackedError> {
    encode_lossless_into_with_policy_strict(lossless, cfg, policy.zero_sign, out)
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use kani::any;

    #[kani::proof]
    pub fn no_panic_decode_bytes() {
        let bytes: [u8; 3] = any();
        let cfg = PackedConfig::new(4, 0, true).unwrap();
        let _ = from_packed(&bytes, &cfg, SignMode::Pfd);
        let _ = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::str::FromStr;

    fn d(v: i64) -> Decimal {
        Decimal::from(v)
    }
    fn ds(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }
    fn cfg(td: u8, sc: u8, signed: bool) -> PackedConfig {
        PackedConfig::new(td, sc, signed).unwrap()
    }

    #[test]
    fn len_table() {
        assert_eq!(expected_len(1), 1);
        assert_eq!(expected_len(2), 2);
        assert_eq!(expected_len(3), 2);
        assert_eq!(expected_len(4), 3);
        assert_eq!(expected_len(18), 10);
    }

    #[test]
    fn config_validation() {
        assert!(PackedConfig::new(0, 0, false).is_err());
        assert!(PackedConfig::new(19, 0, false).is_err());
        assert!(PackedConfig::new(5, 6, false).is_err());
        assert!(PackedConfig::new(5, 29, false).is_err());
    }

    #[test]
    fn config_helpers_work() {
        let c_signed = cfg(4, 2, true);
        assert_eq!(c_signed.byte_len(), 3);
        assert_eq!(c_signed.max_value(), ds("99.99"));
        assert_eq!(c_signed.min_value(), ds("-99.99"));

        let c_unsigned = cfg(4, 2, false);
        assert_eq!(c_unsigned.min_value(), Decimal::ZERO);
    }

    #[test]
    fn decode_into_variants_write_outputs() {
        let c = cfg(4, 2, true);
        let bytes = [0x01, 0x23, 0x4C];
        let mut out = Decimal::ZERO;
        from_packed_into(&bytes, &c, SignMode::Pfd, &mut out).unwrap();
        assert_eq!(out, ds("12.34"));

        let mut scalar_out = Decimal::ZERO;
        from_packed_scalar_into(&bytes, &c, SignMode::Pfd, &mut scalar_out).unwrap();
        assert_eq!(scalar_out, ds("12.34"));

        let mut lossless_out = LosslessDecimal {
            value: Decimal::ZERO,
            sign_nibble: 0,
        };
        from_packed_lossless_into(&bytes, &c, SignMode::Pfd, &mut lossless_out).unwrap();
        assert_eq!(lossless_out.value, ds("12.34"));
        assert_eq!(lossless_out.sign_nibble, 0x0C);
    }

    #[test]
    fn decode_even_works() {
        let c = cfg(4, 2, true);
        assert_eq!(
            from_packed(&[0x01, 0x23, 0x4C], &c, SignMode::Pfd).unwrap(),
            ds("12.34")
        );
    }

    #[test]
    fn encode_even_works() {
        let c = cfg(4, 2, true);
        assert_eq!(to_packed(&ds("12.34"), &c).unwrap(), vec![0x01, 0x23, 0x4C]);
    }

    #[test]
    fn encode_truncates_toward_zero() {
        let c = cfg(4, 2, true);
        assert_eq!(
            to_packed(&ds("12.345"), &c).unwrap(),
            vec![0x01, 0x23, 0x4C]
        );
        assert_eq!(
            to_packed(&ds("-12.345"), &c).unwrap(),
            vec![0x01, 0x23, 0x4D]
        );
    }

    #[test]
    fn strict_encode_rejects_precision_loss() {
        let c = cfg(4, 2, true);
        assert!(matches!(
            to_packed_strict(&ds("12.345"), &c),
            Err(StrictPackedError::PrecisionLoss {
                target_scale: 2,
                current_scale: 3,
                dropped_digit_count: 1
            })
        ));
        assert_eq!(
            to_packed_strict(&ds("12.340"), &c).unwrap(),
            vec![0x01, 0x23, 0x4C]
        );

        let lossless = LosslessDecimal {
            value: ds("12.345"),
            sign_nibble: 0x0C,
        };
        assert!(matches!(
            to_packed_lossless_strict(&lossless, &c),
            Err(StrictPackedError::PrecisionLoss { .. })
        ));
    }

    #[test]
    fn negative_zero_roundtrips_lossless() {
        let c = cfg(3, 0, true);
        let bytes = vec![0x00, 0x0D];
        let loss = from_packed_lossless(&bytes, &c, SignMode::Nopfd).unwrap();
        assert_eq!(loss.value, Decimal::ZERO);
        assert_eq!(loss.sign_nibble, 0x0D);
        assert_eq!(to_packed_lossless(&loss, &c).unwrap(), bytes);
    }

    #[test]
    fn explicit_sign_on_zero_is_preserved_when_valid() {
        let c = cfg(3, 0, true);
        let z = Decimal::ZERO;
        assert_eq!(to_packed_with_sign(&z, &c, 0x0D).unwrap(), vec![0x00, 0x0D]);
        assert_eq!(to_packed_with_sign(&z, &c, 0x0C).unwrap(), vec![0x00, 0x0C]);
    }

    #[test]
    fn canonical_zero_policy_validates_explicit_sign() {
        let c = cfg(3, 0, true);
        let policy = PackedPolicy::canonical(SignMode::Pfd);
        assert!(matches!(
            to_packed_with_sign_with_policy(&Decimal::ZERO, &c, 0xDE, policy),
            Err(PackedError::InvalidSignOverride { nibble: 0xDE, .. })
        ));

        let mut out = [0u8; 2];
        assert!(matches!(
            to_packed_with_sign_into_with_policy(&Decimal::ZERO, &c, 0xDE, policy, &mut out),
            Err(PackedError::InvalidSignOverride { nibble: 0xDE, .. })
        ));

        let lossless = LosslessDecimal {
            value: Decimal::ZERO,
            sign_nibble: 0xDE,
        };
        assert!(matches!(
            to_packed_lossless_with_policy(&lossless, &c, policy),
            Err(PackedError::InvalidSignOverride { nibble: 0xDE, .. })
        ));
    }

    #[test]
    fn stack_only_encoder_writes_into_array() {
        let value = ds("12.34");
        let mut buf = [0u8; Packed::<4, 2, true>::LEN];
        Packed::<4, 2, true>::encode_into(&value, &mut buf).unwrap();
        assert_eq!(buf, [0x01, 0x23, 0x4C]);
    }

    #[test]
    fn public_encode_rejects_bad_output_length_without_panic() {
        let result = std::panic::catch_unwind(|| {
            let c = cfg(4, 2, true);
            let value = ds("12.34");
            let mut out = [0u8; 1];
            to_packed_into(&value, &c, &mut out)
        });
        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            Err(PackedError::InvalidByteLength {
                expected: 3,
                actual: 1,
                total_digits: 4
            })
        ));

        let strict_result = std::panic::catch_unwind(|| {
            let c = cfg(4, 2, true);
            let value = ds("12.34");
            let mut out = [0u8; 1];
            to_packed_into_strict(&value, &c, &mut out)
        });
        assert!(strict_result.is_ok());
        assert!(matches!(
            strict_result.unwrap(),
            Err(StrictPackedError::Codec(PackedError::InvalidByteLength {
                expected: 3,
                actual: 1,
                total_digits: 4
            }))
        ));
    }

    #[test]
    fn explicit_sign_rejects_mismatch_nonzero() {
        let c = cfg(3, 0, true);
        assert!(to_packed_with_sign(&d(123), &c, 0x0D).is_err());
        assert!(to_packed_with_sign(&d(-123), &c, 0x0C).is_err());
    }

    #[test]
    fn decimal_min_is_rejected_without_panic() {
        let c = cfg(18, 0, true);
        assert!(to_packed(&Decimal::MIN, &c).is_err());
    }

    #[test]
    fn policy_object_controls_zero_sign() {
        let c = cfg(3, 0, true);
        let zero = Decimal::ZERO;
        let canonical = PackedPolicy::canonical(SignMode::Pfd);
        let lossless = PackedPolicy::lossless(SignMode::Nopfd);

        assert_eq!(
            to_packed_with_policy(&zero, &c, canonical).unwrap(),
            vec![0x00, 0x0C]
        );
        assert_eq!(
            to_packed_with_policy(&zero, &c, lossless).unwrap(),
            vec![0x00, 0x0C]
        );

        let loss = from_packed_lossless_with_policy(&[0x00, 0x0D], &c, lossless).unwrap();
        assert_eq!(
            to_packed_lossless_with_policy(&loss, &c, lossless).unwrap(),
            vec![0x00, 0x0D]
        );
    }

    #[test]
    fn unsigned_positive_sign_family_roundtrips_lossless() {
        let c = cfg(3, 0, false);
        for (bytes, sign) in [
            (vec![0x12, 0x3A], 0x0A),
            (vec![0x12, 0x3C], 0x0C),
            (vec![0x12, 0x3E], 0x0E),
            (vec![0x12, 0x3F], 0x0F),
        ] {
            let loss = from_packed_lossless(&bytes, &c, SignMode::Nopfd).unwrap();
            assert_eq!(loss.value, ds("123"));
            assert_eq!(loss.sign_nibble, sign);
            assert_eq!(to_packed_lossless(&loss, &c).unwrap(), bytes);
        }
    }

    #[test]
    fn unsigned_explicit_sign_accepts_positive_family() {
        let c = cfg(3, 0, false);
        let value = d(123);
        assert_eq!(
            to_packed_with_sign(&value, &c, 0x0A).unwrap(),
            vec![0x12, 0x3A]
        );
        assert_eq!(
            to_packed_with_sign(&value, &c, 0x0E).unwrap(),
            vec![0x12, 0x3E]
        );
        assert!(to_packed_with_sign(&value, &c, 0x0B).is_err());
        assert!(to_packed_with_sign(&value, &c, 0x0D).is_err());
    }

    #[test]
    fn unsigned_zero_preserves_positive_family_losslessly() {
        let c = cfg(3, 0, false);
        for (bytes, sign) in [
            (vec![0x00, 0x0A], 0x0A),
            (vec![0x00, 0x0C], 0x0C),
            (vec![0x00, 0x0E], 0x0E),
            (vec![0x00, 0x0F], 0x0F),
        ] {
            let loss = from_packed_lossless(&bytes, &c, SignMode::Nopfd).unwrap();
            assert_eq!(loss.value, Decimal::ZERO);
            assert_eq!(loss.sign_nibble, sign);
            assert_eq!(to_packed_lossless(&loss, &c).unwrap(), bytes);
        }
    }

    #[test]
    #[cfg(feature = "simd")]
    fn simd_validation_matches_scalar() {
        assert!(simd_matches_scalar(&[
            0x01, 0x23, 0x4C, 0xAD, 0xEF, 0x00, 0x99, 0x10, 0xFF, 0x7B
        ]));
    }

    #[test]
    #[cfg(feature = "simd")]
    fn simd_decode_rejects_oversized_input_before_semantic_decode() {
        let c = cfg(18, 0, true);
        let bytes = vec![0x12; c.byte_len() + 1];
        assert!(matches!(
            from_packed(&bytes, &c, SignMode::Pfd),
            Err(PackedError::InvalidByteLength {
                expected: 10,
                actual: 11,
                total_digits: 18
            })
        ));
    }

    #[test]
    fn nibble_stream_matches_scalar_expansion() {
        let bytes = [0x01, 0x23, 0x4C, 0xAD];
        let stream: Vec<u8> = stream_nibbles(&bytes).collect();
        assert_eq!(stream, vec![0, 1, 2, 3, 4, 12, 10, 13]);
    }

    #[test]
    fn exhaustive_small_nibble_space() {
        for total_digits in [1u8, 2u8, 3u8, 4u8] {
            for signed in [false, true] {
                let scale = 0u8;
                let cfg = PackedConfig::new(total_digits, scale, signed).unwrap();
                let len = expected_len(total_digits);
                let mut bytes = vec![0u8; len];
                let total_states = 1usize << (len * 8);
                // Keep this exhaustive only for the smallest field lengths.
                if len > 2 {
                    continue;
                }
                for state in 0..total_states {
                    for (i, byte) in bytes.iter_mut().enumerate().take(len) {
                        *byte = ((state >> (i * 8)) & 0xFF) as u8;
                    }
                    let _ = from_packed(&bytes, &cfg, SignMode::Pfd);
                    let _ = from_packed(&bytes, &cfg, SignMode::Nopfd);
                    let _ = from_packed_lossless(&bytes, &cfg, SignMode::Pfd);
                    let _ = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd);
                }
            }
        }
    }

    proptest! {
    #[test]
    fn roundtrip_normalized(
    raw in -999_999_999_999_999_999i128..=999_999_999_999_999_999i128,
    scale in 0u8..=18u8,
    total_digits in 1u8..=18u8,
    signed in any::<bool>(),
    ) {
    if scale > total_digits { return Ok(()); }
    if raw < 0 && !signed { return Ok(()); }
    let digits = if raw == 0 { 1 } else { if raw < 0 { (-raw).to_string().len() as u8 } else { raw.to_string().len() as u8 } };
    if digits > total_digits { return Ok(()); }
    let cfg = PackedConfig::new(total_digits, scale, signed).unwrap();
    let value = Decimal::from_i128_with_scale(raw, scale as u32);
    let packed = to_packed(&value, &cfg).unwrap();
    let decoded = from_packed(&packed, &cfg, SignMode::Nopfd).unwrap();
    assert_eq!(decoded, value);
    }

    #[test]
    fn roundtrip_lossless(
    raw in -999_999_999_999_999_999i128..=999_999_999_999_999_999i128,
    scale in 0u8..=18u8,
    total_digits in 1u8..=18u8,
    signed in any::<bool>(),
    sign_nibble in 0xAu8..=0xFu8,
    ) {
    if scale > total_digits { return Ok(()); }
    let digits = if raw == 0 { 1 } else { if raw < 0 { (-raw).to_string().len() as u8 } else { raw.to_string().len() as u8 } };
    if digits > total_digits { return Ok(()); }
    let cfg = PackedConfig::new(total_digits, scale, signed).unwrap();
    let mut value = Decimal::from_i128_with_scale(raw, scale as u32);
    if raw < 0 {
    value.set_sign_negative(true);
    }
    let bytes = if raw == 0 {
    let mut buf = vec![0u8; expected_len(total_digits)];
    let sign = if signed { sign_nibble } else { 0x0F };
    pack_integer(0, &mut buf, sign, total_digits).unwrap();
    buf
    } else {
    match to_packed_with_sign(&value, &cfg, sign_nibble) {
    Ok(buf) => buf,
    Err(_) => return Ok(()),
    }
    };
    let loss = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd).unwrap();
    assert_eq!(loss.sign_nibble, bytes[bytes.len() - 1] & 0x0F);
    let repacked = to_packed_lossless(&loss, &cfg).unwrap();
    assert_eq!(repacked, bytes);
    }

    #[test]
    fn no_panic_random_bytes(
    bytes in proptest::collection::vec(any::<u8>(), 0..24),
    total_digits in 1u8..=18u8,
    scale in 0u8..=18u8,
    signed in any::<bool>(),
    ) {
    if scale > total_digits { return Ok(()); }
    let cfg = PackedConfig::new(total_digits, scale, signed).unwrap();
    let _ = from_packed(&bytes, &cfg, SignMode::Pfd);
    let _ = from_packed(&bytes, &cfg, SignMode::Nopfd);
    let _ = from_packed_lossless(&bytes, &cfg, SignMode::Pfd);
    let _ = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd);
    }
    }
}
