//! Shared record-codec fuzz harness.
//!
//! Exercises the internal `cobol-record` packed, zoned, binary, and IBM
//! hexadecimal float decoders directly so converter/record-engine codec
//! behavior is fuzzed without shelling out through the CLI.
//!
//! Run:
//!   cargo fuzz run fuzz_record_codecs -- -max_len=256 -timeout=5

#![no_main]

use cobol_record::{
    decode_binary_integer, decode_ibm_float32, decode_ibm_float64, decode_packed_decimal,
    decode_zoned_decimal, packed_decimal_len, Endian, SignPolicy, ZonedEncoding,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }

    let total_digits = usize::from((data[0] % 18).saturating_add(1));
    let scale = u32::from(data[1] % (u8::try_from(total_digits).unwrap_or(18) + 1));
    let signed = data[2] & 1 == 1;
    let packed_len = packed_decimal_len(total_digits);
    if data.len() >= 4 + packed_len {
        let packed = &data[4..4 + packed_len];
        let _ = decode_packed_decimal(packed, total_digits, scale, signed);
    }

    for width in [2usize, 4, 8] {
        if data.len() >= width {
            let bytes = &data[..width];
            let _ = decode_binary_integer(bytes, signed, Endian::Big);
            let _ = decode_binary_integer(bytes, signed, Endian::Little);
            let _ = decode_binary_integer(bytes, false, Endian::Big);
            let _ = decode_binary_integer(bytes, false, Endian::Little);
        }
    }

    let zoned_len = usize::from((data[3] % 18).saturating_add(1));
    if data.len() >= 8 + zoned_len {
        let zoned = &data[8..8 + zoned_len];
        let policy = match data[4] % 3 {
            0 => SignPolicy::Preferred,
            1 => SignPolicy::NonPreferred,
            _ => SignPolicy::Permissive {
                blank_as_positive: data[5] & 1 == 1,
                zero_nibble_as_positive: data[6] & 1 == 1,
            },
        };
        let _ = decode_zoned_decimal(zoned, scale, signed, ZonedEncoding::Ebcdic, policy);
        let _ = decode_zoned_decimal(
            zoned,
            scale,
            signed,
            ZonedEncoding::AsciiOverpunch,
            policy,
        );
    }

    if data.len() >= 4 {
        let _ = decode_ibm_float32(&data[..4], Endian::Big);
        let _ = decode_ibm_float32(&data[..4], Endian::Little);
    }
    if data.len() >= 8 {
        let _ = decode_ibm_float64(&data[..8], Endian::Big);
        let _ = decode_ibm_float64(&data[..8], Endian::Little);
    }
});
