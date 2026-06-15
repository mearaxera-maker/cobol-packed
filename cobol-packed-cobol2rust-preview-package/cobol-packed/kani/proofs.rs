//! Kani proof harnesses for cobol_packed.
//!
//! These harnesses use bounded model checking to verify the core invariants
//! hold for ALL inputs within the verified bounds — not just sampled ones.
//!
//! Run:
//!   cargo kani --features kani --test kani_proofs --harness proof_no_panic
//!   cargo kani --features kani --test kani_proofs --harness proof_no_panic_full_width_decode
//!   cargo kani --features kani,simd --test kani_proofs --harness proof_scalar_simd_agree
//!   cargo kani --features kani --test kani_proofs --harness proof_lossless_roundtrip
//!
//! Requires: `cargo install --locked kani-verifier && cargo kani setup`

#![cfg(kani)]

use cobol_packed::{
    from_packed, from_packed_lossless, from_packed_scalar, to_packed_lossless, PackedConfig,
    SignMode,
};

/// Verify: no public decode call panics for symbolic 2-byte inputs under
/// all configs up to 4 digits. Wider fields are covered by representative
/// full-width harnesses below to keep model-checking state bounded.
#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn proof_no_panic() {
    // Symbolic 2-byte payload covering all 65536 byte-pair combinations.
    let b0: u8 = kani::any();
    let b1: u8 = kani::any();
    let bytes = [b0, b1];

    // Symbolic config within the 1-byte field range (1–2 digits → 1–2 bytes).
    let total_digits: u8 = kani::any();
    kani::assume(total_digits >= 1 && total_digits <= 4);
    let scale: u8 = kani::any();
    kani::assume(scale <= total_digits);
    let signed: bool = kani::any();

    if let Ok(cfg) = PackedConfig::new(total_digits, scale, signed) {
        // These must not panic for any input.
        let _ = from_packed(&bytes, &cfg, SignMode::Pfd);
        let _ = from_packed(&bytes, &cfg, SignMode::Nopfd);
        let _ = from_packed_scalar(&bytes, &cfg, SignMode::Pfd);
        let _ = from_packed_scalar(&bytes, &cfg, SignMode::Nopfd);
        let _ = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd);
    }
}

/// Verify: no public decode call panics for a representative maximum-width
/// 18-digit / 10-byte field.
#[cfg(kani)]
#[kani::proof]
#[kani::unwind(80)]
fn proof_no_panic_full_width_decode() {
    let bytes: [u8; 10] = kani::any();
    let cfg = PackedConfig::new(18, 0, true).unwrap();

    let _ = from_packed(&bytes, &cfg, SignMode::Pfd);
    let _ = from_packed(&bytes, &cfg, SignMode::Nopfd);
    let _ = from_packed_scalar(&bytes, &cfg, SignMode::Pfd);
    let _ = from_packed_scalar(&bytes, &cfg, SignMode::Nopfd);
    let _ = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd);
}

/// Verify: scalar and SIMD nibble expansion agree for 4-byte inputs.
#[cfg(all(kani, feature = "simd"))]
#[kani::proof]
#[kani::unwind(32)]
fn proof_scalar_simd_agree() {
    let bytes: [u8; 4] = kani::any();
    assert!(cobol_packed::simd_matches_scalar(&bytes));
}

/// Verify: lossless decode-encode is byte-for-byte identity for 2-byte fields.
#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn proof_lossless_roundtrip() {
    let b0: u8 = kani::any();
    let b1: u8 = kani::any();
    let bytes = [b0, b1];

    // 2-byte field: 3 or 4 total digits.
    let total_digits: u8 = kani::any();
    kani::assume(total_digits == 3 || total_digits == 4);
    let scale: u8 = kani::any();
    kani::assume(scale <= total_digits);
    let signed: bool = kani::any();

    if let Ok(cfg) = PackedConfig::new(total_digits, scale, signed) {
        if let Ok(lossless) = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd) {
            let repacked = to_packed_lossless(&lossless, &cfg).unwrap();
            // Byte-for-byte identity law.
            assert_eq!(repacked.as_slice(), &bytes[..]);
        }
    }
}

/// Verify: scalar and from_packed agree for all 1-byte inputs.
#[cfg(kani)]
#[kani::proof]
#[kani::unwind(8)]
fn proof_scalar_parity() {
    let b: u8 = kani::any();
    let bytes = [b];
    let cfg = PackedConfig::new(1, 0, true).unwrap();

    let a = from_packed(&bytes, &cfg, SignMode::Pfd);
    let b_res = from_packed_scalar(&bytes, &cfg, SignMode::Pfd);
    assert_eq!(a, b_res);

    let c = from_packed(&bytes, &cfg, SignMode::Nopfd);
    let d = from_packed_scalar(&bytes, &cfg, SignMode::Nopfd);
    assert_eq!(c, d);
}
