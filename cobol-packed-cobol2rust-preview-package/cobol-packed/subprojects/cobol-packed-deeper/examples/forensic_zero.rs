//! Forensic lossless round-trip example for cobol_packed.
//!
//! Demonstrates why `LosslessDecimal` exists and how the lossless encode/decode
//! cycle differs from the canonical (normalizing) cycle.
//!
//! Background
//! ----------
//! In IBM Enterprise COBOL, negative zero (`000D`) is a legal field value in
//! forensic contexts (e.g., EBCDIC audit records, pre-migration snapshots).
//! The canonical codec normalises it to positive zero (`000C`).  The lossless
//! codec preserves the original sign nibble so that a byte-for-byte comparison
//! against the mainframe source remains valid.
//!
//! Run with:
//!   cargo run --example forensic_zero

use cobol_packed::{
    from_packed_lossless, to_packed_lossless, to_packed_with_sign,
    PackedConfig, SignMode,
};
use rust_decimal::Decimal;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    let cfg = PackedConfig::signed(3, 0).unwrap();

    // ── Canonical codec normalises negative zero ─────────────────────────────
    println!("=== Canonical codec ===");

    let neg_zero_bytes = vec![0x00u8, 0x0D]; // 000D — negative zero
    println!("Input bytes (negative zero): {}", hex(&neg_zero_bytes));

    let canonical = cobol_packed::from_packed(&neg_zero_bytes, &cfg, SignMode::Nopfd).unwrap();
    println!("Canonical decode: {canonical}");

    let re_encoded = cobol_packed::to_packed(&canonical, &cfg).unwrap();
    println!("Re-encoded: {}", hex(&re_encoded));
    // 00 0C — canonical positive zero; sign nibble is LOST
    assert_eq!(re_encoded, vec![0x00, 0x0C]);
    println!("→ Sign nibble normalised from 0xD → 0xC (negative zero → positive zero)\n");

    // ── Lossless codec preserves the sign nibble ─────────────────────────────
    println!("=== Lossless codec ===");

    let lossless = from_packed_lossless(&neg_zero_bytes, &cfg, SignMode::Nopfd).unwrap();
    println!("Lossless decode : value = {}, sign_nibble = 0x{:X}", lossless.value, lossless.sign_nibble);

    let lossless_re = to_packed_lossless(&lossless, &cfg).unwrap();
    println!("Lossless re-encode: {}", hex(&lossless_re));
    assert_eq!(lossless_re, neg_zero_bytes, "byte-for-byte identity required");
    println!("→ Byte-for-byte identity preserved ✓\n");

    // ── Explicit sign nibble encoding ────────────────────────────────────────
    println!("=== Explicit sign nibble ===");

    // Encode zero with an explicit non-preferred sign nibble (0x0B).
    // Useful when reading from a mainframe that used non-standard sign encoding.
    let zero_b = to_packed_with_sign(&Decimal::ZERO, &cfg, 0x0B).unwrap();
    println!("Zero with sign 0xB: {}", hex(&zero_b));
    assert_eq!(zero_b[1] & 0x0F, 0x0B);

    let decoded_b = from_packed_lossless(&zero_b, &cfg, SignMode::Nopfd).unwrap();
    assert_eq!(decoded_b.sign_nibble, 0x0B);
    let repacked_b = to_packed_lossless(&decoded_b, &cfg).unwrap();
    assert_eq!(repacked_b, zero_b);
    println!("Round-trip with explicit sign 0xB ✓\n");

    // ── Non-preferred signs (NOPFD mode) ─────────────────────────────────────
    println!("=== Non-preferred signs (Nopfd) ===");

    // IBM mainframes sometimes write values with sign nibbles 0xA–0xE
    // for positive or negative depending on convention.
    // Nopfd mode accepts all nibbles A–F.
    for &nibble in &[0x0A, 0x0B, 0x0E] {
        let bytes = to_packed_with_sign(&Decimal::from(42), &cfg, nibble).unwrap();
        let result = from_packed_lossless(&bytes, &cfg, SignMode::Nopfd);
        println!("  nibble 0x{nibble:X} → {:?}", result.map(|l| (l.value, l.sign_nibble)));
    }

    println!("\nAll assertions passed.");
}
