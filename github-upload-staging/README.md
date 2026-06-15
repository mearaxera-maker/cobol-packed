# cobol_packed

`cobol_packed` is a Rust codec and command-line tool for IBM Enterprise COBOL
COMP-3 packed decimal fields.

The library provides safe scalar encode/decode, const-generic field codecs,
lossless forensic round trips, and optional SIMD validation. The
`cobol-packed` binary adds migration-grade operational workflows for field
inspection, schema-driven batch decode, byte-for-byte verification, and audit
reporting.

Schema v1 is the stable COMP-3 batch format. Schema v2 adds an internal COBOL
record-layout engine for mixed fixed-width records: packed decimal, zoned
decimal with EBCDIC or ASCII overpunch signs, binary/native COMP fields with
explicit endian, IBM hexadecimal floats, alphanumeric EBCDIC text, SYNC slack
planning, OCCURS DEPENDING ON, and safe REDEFINES window decoding.

Encoding APIs preserve existing compatibility: when a `Decimal` carries more
fractional digits than the configured packed scale, the legacy `to_packed*`
helpers truncate low-order fractional digits toward zero. Use the additive
`to_packed_strict*` helpers when migration code must reject precision loss.

## CLI quickstart

Install the CLI from crates.io with the application feature enabled:

```text
cargo install cobol_packed --features cli
```

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
