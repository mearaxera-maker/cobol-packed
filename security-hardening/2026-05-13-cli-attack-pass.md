# CLI Attack And Hardening Pass

Date: 2026-05-13

Scope: upgraded `cobol-packed` CLI and supporting codec integration.

## Findings Fixed

### 1. Batch output could still become non-streaming

`batch decode --output json` buffered all decoded rows. That is useful for a
single valid JSON array, but dangerous for real migration files. The CLI now
uses explicit row sinks:

- JSONL, CSV, and table output stream row-by-row.
- Audit output observes rows without storing successful rows.
- JSON output remains buffered but is capped by `MAX_BUFFERED_ROWS`.

### 2. Schema, slicing, decode, and verify were loosely coupled

The old flow repeatedly re-derived field facts in multiple places. The CLI now
uses `FieldPlan` as the relationship point between schema validation and codec
execution. The same derived `PackedConfig`, expected byte length, mode, sign
policy, and offset/length facts drive decode, verify, and error output.

### 3. `skip-record` could emit partial records

Field-level errors previously allowed the rest of the same record to continue.
For migration output that is a data-integrity flaw. Error handling now returns
a record-processing disposition so `skip-record` stops the current record. A
second pass fixed the subtler variant: valid fields decoded before a later bad
field are now buffered and flushed only after the entire record succeeds.

### 4. Malformed record-level input bypassed recovery policy

Bad hex lines, malformed JSONL, and malformed CSV records could bypass
schema-level `on_error` handling. These are now converted into structured
`<record>` error rows and routed through the same policy as field failures.

### 5. Line and JSON output limits were too loose

Hex and JSONL records now use bounded line reads. Hex parsing also takes an
expected-byte limit where the schema or field shape provides one. Buffered JSON
output is capped and points users to streaming formats. Schema files also have
a size cap before they are read into memory.

### 6. Panic output was not safely machine-readable

The panic hook now emits JSON through `serde_json`, so panic messages are
escaped instead of interpolated into a raw JSON string.

### 7. Fixed-width hex records could be shorter than the schema

Hex records were capped by maximum length, but a short record could still
decode if the declared fields happened to fit. Fixed-width hex now goes through
the same exact `record_length` relationship as binary input, and mismatches
produce `E_RECORD_LENGTH`.

### 8. Name-based schemas could carry ignored offsets

CSV and JSONL fields are selected by name, so offsets and fixed-width
`record_length` were misleading metadata. Schema validation now rejects both
for name-based inputs instead of allowing operators to believe positional
constraints are enforced.

### 9. CSV parsing could allocate outside the bounded line reader

The CSV crate's streaming reader owns record buffering. The CLI now reads CSV
through the same bounded physical-line reader used by JSONL/hex, parses one
CSV record at a time, rejects multiline CSV records, and validates the record
field count against the header before field decoding.

### 10. Audit output lacked an explicit verdict

Audit reports now carry tool/version metadata, schema/input metadata,
`record_limit`, configurable `failure_sample_limit`, and a deterministic
`status` of `passed`, `failed`, or `empty`.

### 11. Profile output was too coarse for dirty-file triage

Audit/profile reports now maintain deterministic global error distributions
and per-field profiles. Each field profile tracks validity counters, min/max
decoded values, sign distribution, error distribution, negative zero count, and
non-preferred sign count.

### 12. Streaming decode pre-read the input for evidence hash

`batch decode` constructed a full audit report before emitting streaming rows,
which forced a complete SHA-256 pass over the input even for JSONL/CSV/table
output. Streaming decode now skips input hashing unless audit output is
requested. Audit/profile/verify still compute the hash because evidence
metadata is part of their output contract.

### 13. Field workbench accepted ambiguous input source flags

`--stdin` could be combined with `--hex` or `--file`, and then branch ordering
silently ignored stdin. Clap conflicts and a runtime source-count guard now
require exactly one of `--hex`, `--file`, or `--stdin`.

### 14. Verify could report failure but exit success

Under `emit-error-row`, `batch verify` could finish processing, render a failed
audit report, and still return process exit code `0`. It now exits with data
error code `1` whenever the completed verification audit status is `failed`.

### 15. Field verification did not prove record coverage

Fixed-width forensic workflows can now declare non-COMP-3 `fillers`, request
record-level verification through schema `verification_scope` or
`--strict-record`, and receive explicit audit evidence for field verification,
record coverage, gaps, overlaps, and record byte-for-byte verdict.

### 16. Schema and runtime evidence were not separated

Audit output now separates the raw schema file SHA-256 from a canonical semantic
schema hash. Default audit output stays deterministic; `--evidence-mode full`
adds argv, platform, cwd, and generation timestamp for evidence bundles.

### 17. Hostile record samples lost source bytes

Malformed hex, CSV, and JSONL record errors now retain bounded raw previews.
JSONL fields with non-string values now emit explicit `E_JSON_TYPE` errors
instead of being treated as missing fields.

## Remaining Proof Gap

Earlier local validation was blocked before the Rust toolchain was installed.
After installing Rustup and the Windows C++ build tools, the focused CLI and
library validation suites were run locally.

## Required Validation On A Rust Host

```text
cargo generate-lockfile
cargo test --all-features
cargo test --test cli_smoke
cargo clippy --all-targets --all-features -- -D warnings
cargo run --bin cobol-packed -- decode --digits 4 --scale 2 --signed --hex 01234C
```
