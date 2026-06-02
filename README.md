# cobol_packed

### Forensic COMP-3 tooling and a correctness-first COBOL migration workbench

![Rust](https://img.shields.io/badge/rust-stable-orange)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)
![Status](https://img.shields.io/badge/status-active-success)
![SIMD](https://img.shields.io/badge/SIMD-AVX2%20%7C%20SSE2-purple)
![Oracle](https://img.shields.io/badge/oracle-GnuCOBOL-informational)

`cobol_packed` is a Rust codec and command-line tool for IBM Enterprise COBOL
COMP-3 packed decimal fields.

The library provides safe scalar encode/decode, const-generic field codecs,
lossless forensic round trips, and optional SIMD validation. The
`cobol-packed` binary adds migration-grade operational workflows for field
inspection, schema-driven batch decode, byte-for-byte verification, and audit
reporting.

The repository also contains `cobol2rust`, an experimental, correctness-first
COBOL-to-Rust converter for a conservative procedural batch subset. It is not a
dialect-complete COBOL compiler. The current capability matrix lists 10
supported, 74 partial, 35 blocked, and 4 unknown feature areas; unsupported
source constructs are reported instead of being compiled as best-effort Rust.
Data Division layout is shared through the `cobol-record` crate so CLI schemas
and generated converter accessors use the same byte-layout facts.

Schema v1 is the stable COMP-3 batch format. Schema v2 adds an internal COBOL
record-layout engine for mixed fixed-width records: packed decimal, zoned
decimal with EBCDIC or ASCII overpunch signs, binary/native COMP fields with
explicit endian, IBM hexadecimal floats, alphanumeric EBCDIC text, SYNC slack
planning, OCCURS DEPENDING ON, and safe REDEFINES window decoding.

Encoding APIs preserve existing compatibility: when a `Decimal` carries more
fractional digits than the configured packed scale, the legacy `to_packed*`
helpers truncate low-order fractional digits toward zero. Use the additive
`to_packed_strict*` helpers when migration code must reject precision loss.

The core rule is simple: packed decimal bytes are a contract, not a convenience
format. The converter workbench follows the same rule for supported COBOL
semantics: execute the modeled byte behavior, and block the rest explicitly.

## CLI quickstart

Install the published COMP-3 CLI from crates.io with the application feature
enabled:

```text
cargo install cobol_packed --features cli
```

The `cobol2rust` converter is a repository/workspace binary in this branch.
Build it from source with `--features converter`; do not treat it as a
crates.io-published migration compiler unless the release notes for a future
version explicitly say so.

```text
cobol-packed decode --digits 4 --scale 2 --signed --hex 01234C
cobol-packed inspect --digits 3 --scale 0 --signed --sign-mode nopfd --hex 000D --output json
cobol-packed batch verify --schema schema.json --input records.bin --output audit
cobol-packed batch verify --schema schema.json --input records.bin --output audit --strict-record
cobol-packed schema emit-rust --schema schema-v2.json --output record.rs
cobol-packed schema from-copybook --input record.cpy --output schema-v2.json
cobol-packed schema compare --left old.json --right new.json --output json
```

See `docs/cli.md` for the schema format, error model, and batch examples.
For the engineering narrative and public capability map, see
`docs/evolution.md` and `docs/feature-map.md`.
For the converter support inventory, use `migration-capability-matrix.json` and
`docs/migration-capability-matrix.md`.
For fresh-machine setup and release-candidate verification, see
`docs/installation.md`.
For a minimal map of the repository layout and where to start by task, see
`docs/project-navigation.md`.
For redistribution, dependency-license policy, SBOM generation, and packaging
requirements, see `docs/compliance.md`, `docs/packaging.md`, and `NOTICE`.

## COBOL-to-Rust converter

Build the converter binary with:

```text
cargo run --features converter --bin cobol2rust -- convert \
  --input program.cbl \
  --copybook-dir copybooks \
  --out generated-rust \
  --dialect ibm \
  --source-format fixed
```

The generated project vendors the internal `cobol-runtime`, `cobol-record`,
`cobol-dialect`, and `cobol-vm` crates, uses StoragePool-backed byte execution
for COBOL semantics, and emits `DataView` accessors for display text, COMP-3,
and binary fields. Procedure-based in-memory `SORT` is supported for the
documented fixed-record slice; unsupported constructs such as EXEC SQL/CICS/DLI,
line-sequential or indexed files, separately loaded dynamic CALL targets,
edited/national/DBCS edge cases, `MERGE`, physical sort `USING`/`GIVING`,
report writer, and other not-yet-modeled clauses are reported in
`migration-report.json`; blocked migrations exit non-zero rather than emitting
misleading Rust.

See `docs/converter.md` for the current architecture, supported subset, and
known boundaries. The GnuCOBOL oracle validation harness is documented in
`docs/oracle_validation.md`.

Batch workflows are stream-oriented by default. Use `--max-records` for bounded
sampling and `--sample-failures` to control how many forensic failure samples
are retained in audit output.

## Security posture

The codec validates packed field length before optional SIMD parity checks.
The CLI validates schemas before processing data, streams batch inputs, and
emits stable error codes for automation. Fixed-width hex and binary records are
checked against exact schema record length, name-based CSV/JSONL schemas reject
ignored offsets, and fail/skip recovery modes do not emit partial records.
Streaming decode output does not pre-hash the input file unless an audit report
is requested.

Forensic release-candidate features include additive fixed-width `fillers`,
schema `verification_scope`, `--strict-record` verification, separate raw-file
and description-insensitive semantic schema hashes, optional
`--evidence-mode full` runtime metadata with redacted argv by default, explicit
JSONL type errors, and release artifacts with completions, man page, SBOM
metadata, and signed checksum support.

A public security summary is maintained in `docs/security/scan-summary.md`.
Vulnerability reporting and release security gates are documented in
`SECURITY.md`.
