# Changelog

All notable changes to `cobol_packed` are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- `cobol-packed` CLI for single-field decode/encode/inspect, schema-driven
  batch decode, forensic verify, schema validation, and profile/audit reports
- Versioned JSON/TOML schema model for fixed-width binary, hex, CSV, and JSONL
  migration inputs
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
- Strict precision-safe encode APIs reject downscale precision loss without
  changing the legacy truncating `to_packed*` behavior
- Schema v2 record-layout engine with declared/sequential layout modes,
  planned absolute byte ranges, coverage ranges, and v1 lowering into the new
  internal layout model
- Schema v2 field codecs for `packed-decimal`, `zoned-decimal`, `binary`,
  `native-binary`, `ibm-float32`, `ibm-float64`, `alphanumeric`, and `filler`
- Zoned decimal decoding for EBCDIC zone bytes and ASCII overpunch signs,
  including preferred, non-preferred, and explicit permissive sign policies
- Binary/native COMP decoding with explicit endian and unsigned 8-byte values
  represented without truncating into signed integers
- Safe REDEFINES decoding by immutable byte-window reinterpretation, with
  coverage counted once for the base range
- OCCURS DEPENDING ON binary streaming support with fail-fast counter bounds
  validation and no default clamping
- `schema emit-rust` command that generates safe Rust record views with checked
  raw/hex accessors and no unsafe code
- `batch verify --coverage-report` flag reserved for explicit coverage-focused
  verification workflows; audit output already includes the coverage report
- Full evidence mode redacts argv by default, with explicit `--evidence-argv`
  controls for `redacted`, `raw`, and `omit`
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
- `kani/proofs.rs` - bounded Kani proof harnesses for core invariants,
  including representative 18-digit/10-byte no-panic decode coverage
- `examples/basic.rs` - annotated canonical encode/decode walkthrough
- `examples/forensic_zero.rs` - lossless/negative-zero demonstration
- Feature flags: `simd`, `avx2`, `kani`
- `[package.metadata.docs.rs]` all-features config

### Changed
- MSRV is now Rust 1.74 for the CLI application dependency stack
- `Packed::<D,S,SIGNED>::len()` is deprecated in favor of `Packed::<D,S,SIGNED>::LEN` (associated const)
- `simd_matches_scalar` is available when the `simd` feature is enabled
- `scale` validation is documented as `0..=total_digits`; `total_digits` is
  bounded to `1..=18`
- Semantic schema hashes now exclude human-readable field/filler descriptions
- Release packaging now includes SBOM metadata, generated operator artifacts,
  release artifact smoke testing, and signed checksum support

### Fixed
- SIMD-enabled decode now checks exact field length before parity expansion and
  uses debug-only scalar parity assertions, preventing oversized malformed input
  from causing full-slice expansion or production panics
- Encode paths return typed errors for bad caller-provided output lengths
  instead of relying on internal assertions
- Explicit sign nibbles are validated even when canonical-zero policy later
  normalizes zero
- Canonical CLI field decoding no longer creates an invalid synthetic lossless
  sign nibble
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
- Schema parse errors now identify the expected JSON/TOML format and path

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
