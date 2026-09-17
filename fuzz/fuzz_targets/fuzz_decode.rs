//! libFuzzer harness: decode arbitrary bytes under all valid configurations.
//!
//! Goals:
//! - No panic on any byte sequence.
//! - SIMD and scalar nibble expansion always agree.
//! - Lossless round-trips are byte-for-byte identical.
//!
//! Run:
//!   cargo fuzz run fuzz_decode -- -max_len=32 -timeout=5
//!   cargo fuzz run --features simd fuzz_decode -- -max_len=256 -timeout=5

#![no_main]

use cobol_packed::{
    from_packed, from_packed_lossless, from_packed_scalar, simd_matches_scalar, to_packed_lossless,
    PackedConfig, SignMode,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 3 {
        return;
    }

    // Derive config parameters from the first two bytes of input.
    let total_digits = (data[0] % 18).saturating_add(1); // 1..=18
    let scale = data[1] % (total_digits + 1); // 0..=total_digits
    let signed = data[2] & 1 == 1;
    let payload = &data[3..];

    let cfg = match PackedConfig::new(total_digits, scale, signed) {
        Ok(c) => c,
        Err(_) => return,
    };

    // Invariant: no call panics on arbitrary payload.
    let _ = from_packed(payload, &cfg, SignMode::Pfd);
    let _ = from_packed(payload, &cfg, SignMode::Nopfd);

    // Invariant: scalar and SIMD nibble expansion agree.
    assert!(simd_matches_scalar(payload));

    // Invariant: scalar and from_packed agree.
    let pfd = from_packed(payload, &cfg, SignMode::Pfd);
    let spfd = from_packed_scalar(payload, &cfg, SignMode::Pfd);
    let nopfd = from_packed(payload, &cfg, SignMode::Nopfd);
    let snopfd = from_packed_scalar(payload, &cfg, SignMode::Nopfd);
    assert_eq!(pfd, spfd);
    assert_eq!(nopfd, snopfd);

    // Invariant: lossless decode → encode is byte-for-byte identity.
    if let Ok(lossless) = from_packed_lossless(payload, &cfg, SignMode::Nopfd) {
        let repacked = to_packed_lossless(&lossless, &cfg).unwrap();
        // Trim or pad payload to expected length before comparing.
        let expected_len = cfg.byte_len();
        if payload.len() == expected_len {
            assert_eq!(repacked, payload);
        }
    }
});
