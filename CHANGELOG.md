# Changelog

All notable changes to `cobol_packed` are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

## [1.1.0] - 2026-05-21

### Added
- `hostlens schema from-copybook` to bootstrap schema v2 JSON from a
  deliberately limited fixed-width COBOL copybook subset.
- `hostlens schema emit-rust` to generate decoded-value Rust structs plus
  record/field offset constants from a validated schema.
- `hostlens schema compare` to diff two schemas while ignoring descriptions
  and output-format preferences.
- Schema workflow documentation covering copybook import limits, Rust emission,
  schema comparison, and safe release boundaries.

### Fixed
- Copybook import now honors `USAGE` clauses that appear before `PIC`.
- Copybook declaration splitting now ignores periods inside quoted literals and
  strips inline `*>` comments outside quoted literals.
- Copybook-generated duplicate normalized field names now receive deterministic
  suffixes instead of failing schema validation.
- Copybook-generated binary field descriptions now call out the IBM
  big-endian COMP/BINARY sizing assumption.
- `schema emit-rust` now treats omitted binary `signed` as the effective schema
  default of `true`.
- `schema compare` now compares effective field defaults for derived packed
  lengths, zero scale, and binary signedness.
- `schema from-copybook` now rejects name-based CSV/JSONL input encodings
  because generated schemas are fixed-width layouts.

## [1.0.0] - 2026-05-20

### Added
- HostLens CLI for single-field decode/encode/inspect, schema-driven batch
  decode, forensic verify, schema validation, and profile/audit reports
- Versioned JSON/TOML schema model for fixed-width binary, hex, CSV, and JSONL
  migration inputs
- Schema v2 field typing for packed decimal, EBCDIC display text, zoned
  decimal, binary integers, and raw-byte fields while preserving schema v1
  packed-decimal compatibility
- Explicit EBCDIC display-text support for 34 audited SBCS IBM/Windows
  codepage tables plus common `cpNNN`, `ibmNNN`, `ccsidNNN`, leading-zero, and
  Windows codepage-number aliases
- `mixed-dbcs-text` schema field type for SO/SI-stateful mixed DBCS fields,
  supporting `cp930`, `cp933`, `cp935`, `cp937`, and `cp939` with generated
  Unicode ICU DBCS glyph tables
- Stable CLI error codes and process exit codes for automation
- Streaming row sink architecture for JSONL, CSV, table, and audit output
- Validated field planning layer connecting schema facts to decode, verify, and
  error reporting
- Audit reports now include tool/version metadata, schema/input metadata,
  pass/fail/empty status, configurable failure sample limits, and optional
  record processing limits
- Audit/profile reports now include global error distributions and per-field
  profiles with min/max values, validity counters, sign distributions, and
  anomaly counters
- Forensic record evidence: fixed-width schemas can declare `fillers`, schemas
  can select `verification_scope`, and `batch verify --strict-record` proves
  both lossless field round trips and full record-byte coverage
- Audit reports now include raw schema file SHA-256, canonical semantic schema
  hash, field/filler counts, record coverage gaps/overlaps, field-level and
  record-level byte-for-byte verdicts, and optional `--evidence-mode full`
  runtime metadata
- Audit/profile reports include elapsed time and throughput metrics for records,
  fields, and input bytes
- Batch commands support stdin input with `--input -`, one-based output record
  indices, progress reporting, dry runs, quiet table output, fail-on-empty CI
  gates, fixed-width binary `--parallel`, and configurable buffered JSON row caps
- `completions` and `man` commands generate shell completions and a man page
  from the CLI definition
- `--max-records` bounded sampling and `--sample-failures` audit sample control
  for batch workflows
- `cli` feature for the application dependency stack; install with
  `cargo install cobol_packed --features cli`
- `docs/cli.md` with CLI quickstart, schema reference, and security notes
- `PackedConfig::max_value()` and `min_value()` convenience methods
- `PackedConfig::byte_len()` method
- `from_packed_into`, `from_packed_scalar_into`, and `from_packed_lossless_into` stack-only decode APIs
- `docs/architecture.md` - full layer diagram and invariant reference
- `docs/migration_guide.md` - v0.6 to v1.0 upgrade path
- `fuzz/fuzz_targets/fuzz_roundtrip.rs` - encode-to-decode roundtrip harness
- `fuzz/fuzz_targets/fuzz_schema_batch.rs` - schema-aware offset/record-layout
  fuzz harness
- `kani/proofs.rs` - four Kani proof harnesses for core invariants
- `examples/basic.rs` - annotated canonical encode/decode walkthrough
- `examples/forensic_zero.rs` - lossless/negative-zero demonstration
- Feature flags: `simd`, `avx2`, `kani`
- `[package.metadata.docs.rs]` all-features config

### Changed
- MSRV is now Rust 1.74 for the CLI application dependency stack
- `Packed::<D,S,SIGNED>::len()` is deprecated in favor of `Packed::<D,S,SIGNED>::LEN` (associated const)
- `simd_matches_scalar` is available when the `simd` feature is enabled
- `scale` validation tightened: max accepted scale is now `min(total_digits, 18)`
- Release packaging now includes SBOM metadata, generated operator artifacts,
  release artifact smoke testing, and signed checksum support

### Fixed
- SIMD-enabled decode now checks exact field length before parity expansion,
  preventing oversized malformed input from causing full-slice allocation
- Batch JSON output is capped; streaming formats avoid collecting full datasets
  in memory
- `fail` and `skip-record` now process records atomically so a later bad field
  cannot leak earlier decoded fields as partial migration output
- Malformed hex, CSV, and JSONL records now follow the schema `on_error` policy
- Malformed record-level rows retain bounded raw previews in error rows instead
  of emitting empty `raw_hex` samples
- JSONL fields that are present but not strings now emit `E_JSON_TYPE` instead
  of being treated as silently missing
- Hex fixed-width records must match schema `record_length` exactly
- Name-based CSV/JSONL schemas reject ignored offsets at schema-check time
- Name-based CSV/JSONL schemas reject fixed-width `record_length`; schema field
  count is capped at 1024
- Single-field commands require exactly one of `--hex`, `--file`, or `--stdin`
- Streaming `batch decode` outputs no longer pre-hash the input file unless an
  audit report is requested
- `batch verify` exits with data error code `1` when verification completes with
  failed audit status under `emit-error-row`
- CSV input is read through bounded physical-line parsing instead of the
  unbounded CSV streaming reader
- Structured error rows include raw byte length and raw-hex truncation status
- Panic output is JSON-escaped for machine-readable automation
- `PackedConfig::new` now rejects `scale > 18` which `rust_decimal` previously
  silently clamped, masking encoding bugs

---

## [0.6.1] - 2026-05-12

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

## [0.6.0] - 2026-04-20

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
