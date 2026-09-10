# Changelog

All notable changes to `cobol_packed` are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- `PackedConfig::max_value()` and `min_value()` convenience methods
- `PackedConfig::byte_len()` method
- `from_packed_into`, `from_packed_scalar_into`, and `from_packed_lossless_into` stack-only decode APIs
- `docs/architecture.md` — full layer diagram and invariant reference
- `docs/migration_guide.md` — v0.6 → v1.0 upgrade path
- `fuzz/fuzz_targets/fuzz_roundtrip.rs` — encode→decode roundtrip harness
- `kani/proofs.rs` — four Kani proof harnesses for core invariants
- `examples/basic.rs` — annotated canonical encode/decode walkthrough
- `examples/forensic_zero.rs` — lossless/negative-zero demonstration
- Feature flags: `simd`, `avx2`, `kani`
- `[package.metadata.docs.rs]` all-features config

### Changed
- `Packed::<D,S,SIGNED>::len()` is deprecated in favor of `Packed::<D,S,SIGNED>::LEN` (associated const)
- `simd_matches_scalar` is available when the `simd` feature is enabled
- `scale` validation tightened: max accepted scale is now `min(total_digits, 18)`

### Fixed
- `PackedConfig::new` now rejects `scale > 18` which `rust_decimal` previously
  silently clamped, masking encoding bugs

---

## [0.6.1] — 2026-05-12

### Added
- `to_packed_with_policy` and `from_packed_lossless_with_policy` accepting
  an explicit `PackedPolicy` object
- `to_packed_lossless_with_policy` for symmetric policy-driven lossless encode
- `NibbleIter` zero-allocation nibble streaming via `nibble_iter()`
- `stream_nibbles()` iterator (alias for `nibble_iter()`)
- `encode_array` and `encode_lossless_array` const-generic array encode APIs
- AVX2 nibble-expansion path (`expand_nibbles_avx2`) with debug parity check
- `ZeroSignPolicy` enum (`Canonical` / `Preserve`)
- `PackedPolicy` struct combining `SignMode` and `ZeroSignPolicy`
- Exhaustive small-nibble-space test (`exhaustive_small_nibble_space`)
- `docs/spec.md` formal specification sketch

### Changed
- SIMD nibble expander is now a validation layer only; scalar decoder remains
  the single source of truth

---

## [0.6.0] — 2026-04-20

### Added
- Initial public release
- `PackedConfig` runtime codec
- `Packed<DIGITS, SCALE, SIGNED>` const-generic zero-sized codec
- `LosslessDecimal` forensic wrapper
- `from_packed` / `to_packed` with `SignMode::Pfd` and `SignMode::Nopfd`
- `from_packed_lossless` / `to_packed_lossless`
- `to_packed_with_sign` explicit sign nibble encoder
- `to_packed_into` stack-only (no-alloc) encode
- SSE2 nibble expansion with scalar fallback
- `from_packed_scalar` reference decoder
- `simd_matches_scalar` validation helper
- Proptest roundtrip and no-panic harnesses
- Criterion benchmarks: decode, encode-into, SIMD validation
- MIT OR Apache-2.0 dual license
