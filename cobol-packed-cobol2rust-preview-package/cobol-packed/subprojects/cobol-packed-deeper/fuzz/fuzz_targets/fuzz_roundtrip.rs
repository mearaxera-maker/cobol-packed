//! libFuzzer harness: encode a structured value and verify decode inverts it.
//!
//! Derives a valid `Decimal` from the fuzz input, encodes it, decodes it,
//! and asserts equality.  Also verifies the lossless path round-trips.
//!
//! Run:
//!   cargo fuzz run fuzz_roundtrip -- -max_len=32 -timeout=5

#![no_main]

use cobol_packed::{
    from_packed, from_packed_lossless, to_packed, to_packed_lossless,
    PackedConfig, SignMode,
};
use libfuzzer_sys::fuzz_target;
use rust_decimal::Decimal;

fuzz_target!(|data: &[u8]| {
    if data.len() < 12 {
        return;
    }

    // Config parameters from first 3 bytes.
    let total_digits = (data[0] % 18).saturating_add(1);
    let scale = data[1] % (total_digits + 1);
    let signed = data[2] & 1 == 1;

    // Raw i64 magnitude from next 8 bytes.
    let raw = i64::from_le_bytes(data[3..11].try_into().unwrap());

    let cfg = match PackedConfig::new(total_digits, scale, signed) {
        Ok(c) => c,
        Err(_) => return,
    };

    let value = Decimal::from_i128_with_scale(raw as i128, scale as u32);

    // Attempt encode; may fail for out-of-range magnitudes — that's expected.
    let encoded = match to_packed(&value, &cfg) {
        Ok(b) => b,
        Err(_) => return,
    };

    // Decode must succeed and produce the same value (after scale normalisation).
    let decoded = from_packed(&encoded, &cfg, SignMode::Nopfd).unwrap();
    assert_eq!(decoded, value);

    // Lossless round-trip must be byte-for-byte identical.
    if let Ok(lossless) = from_packed_lossless(&encoded, &cfg, SignMode::Nopfd) {
        let repacked = to_packed_lossless(&lossless, &cfg).unwrap();
        assert_eq!(repacked, encoded);
    }
});
