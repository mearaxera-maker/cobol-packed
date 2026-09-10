//! Basic encode/decode example for cobol_packed.
//!
//! Demonstrates the two primary codec paths:
//! - `Packed<D, S, SIGNED>` — const-generic, zero-sized, stack-only.
//! - `PackedConfig` — runtime configuration for dynamic schema.
//!
//! Run with:
//!   cargo run --example basic

use cobol_packed::{from_packed, to_packed, Packed, PackedConfig, SignMode};
use rust_decimal::Decimal;
use std::str::FromStr;

fn main() {
    // ── Const-generic codec ─────────────────────────────────────────────────
    //
    // PIC S9(4)V99 COMP-3  (4 integer digits, 2 decimal, signed)
    // Field byte length = ⌊(4+2)/2⌋ + 1 = 4 bytes  [even digit count → 1 pad nibble]
    //
    // Actually total_digits = 6, so len = ⌊6/2⌋ + 1 = 4 bytes? No:
    // expected_len(6) = 6/2 + 1 = 4.  Correct.

    println!("=== Const-generic codec (Packed<6,2,true>) ===");

    let value = Decimal::from_str("12.34").unwrap();
    let encoded: Vec<u8> = Packed::<6, 2, true>::encode(&value).unwrap();

    println!("Value   : {value}");
    println!("Encoded : {:02X?}", encoded);
    // Expected: [0x00, 0x12, 0x34, 0x0C]  (PFD positive sign = 0xC)

    let decoded = Packed::<6, 2, true>::decode(&encoded, SignMode::Pfd).unwrap();
    println!("Decoded : {decoded}");
    assert_eq!(value, decoded);

    // Negative value
    let neg = Decimal::from_str("-99.99").unwrap();
    let enc_neg = Packed::<6, 2, true>::encode(&neg).unwrap();
    println!("\nNegative: {neg}  →  {:02X?}", enc_neg);
    // Sign nibble = 0xD for PFD negative

    // Stack-only path: encode directly into a fixed-size array
    let mut buf = [0u8; Packed::<6, 2, true>::LEN];
    Packed::<6, 2, true>::encode_into(&value, &mut buf).unwrap();
    println!("Stack buf: {:02X?}", buf);

    // ── Runtime config codec ────────────────────────────────────────────────
    println!("\n=== Runtime config (PackedConfig) ===");

    // PIC 9(10) COMP-3 — 10 digits, unsigned, no decimal places
    let cfg = PackedConfig::unsigned(10, 0).unwrap();
    println!("Config  : {cfg:?}");
    println!("Byte len: {}", cfg.byte_len());

    let account_id = Decimal::from(1_234_567_890u64);
    let raw = to_packed(&account_id, &cfg).unwrap();
    println!("Account ID {account_id}  →  {:02X?}", raw);

    let back = from_packed(&raw, &cfg, SignMode::Pfd).unwrap();
    println!("Decoded: {back}");
    assert_eq!(account_id, back);

    // ── Zero ────────────────────────────────────────────────────────────────
    println!("\n=== Zero handling ===");

    let zero = Decimal::ZERO;
    let cfg3 = PackedConfig::signed(3, 0).unwrap();
    let zero_bytes = to_packed(&zero, &cfg3).unwrap();
    println!("Canonical zero: {:02X?}", zero_bytes);
    // 0x0C = canonical positive zero (PFD)

    println!("\nAll assertions passed.");
}
