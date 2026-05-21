# HostLens

[![CI](https://github.com/mearaxera-maker/cobol-packed/actions/workflows/ci.yml/badge.svg)](https://github.com/mearaxera-maker/cobol-packed/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cobol_packed.svg)](https://crates.io/crates/cobol_packed)
[![docs.rs](https://docs.rs/cobol_packed/badge.svg)](https://docs.rs/cobol_packed)

HostLens is a forensic mainframe record decoder and audit CLI backed by the
`cobol_packed` Rust crate. It is not a COBOL program converter. Its job is to
decode fixed-width host data, preserve byte evidence, and report exactly which
records and fields were accepted, rejected, or round-tripped.

The crate still publishes as `cobol_packed` for Rust compatibility. The primary
application binary is `hostlens`; `cobol-packed` remains as a compatibility
alias for existing scripts.

## What It Handles

- COMP-3 packed decimal encode/decode with canonical and lossless modes.
- Schema-driven batch decode and verify for binary, hex, CSV, and JSONL inputs.
- Schema v2 mixed records: `packed-decimal`, `display-text`,
  `mixed-dbcs-text`, `zoned-decimal`, `binary`, and `raw-bytes`.
- Explicit EBCDIC display text decoding for 34 audited SBCS IBM/Windows
  codepage tables, including `cp037`, `cp273`, `cp297`, `cp424`, `cp500`,
  `cp870`, `cp875`, `cp1025`, `cp1026`, `cp1047`, and `cp1140` through
  `cp1149`.
- Separate mixed DBCS text decoding for `cp930`, `cp933`, `cp935`, `cp937`,
  and `cp939` fields using SO/SI shift-state validation and generated Unicode
  ICU DBCS glyph tables.
- Audit reports with semantic schema hashes, source file hashes, coverage,
  failure samples, sign distributions, elapsed time, bytes/sec, records/sec,
  and fields/sec.

## CLI Quickstart

```text
cargo install cobol_packed --features cli
```

Decode one packed field:

```text
hostlens decode --digits 4 --scale 2 --signed --hex 01234C
```

Verify a fixed-width extract and produce an audit report:

```text
hostlens batch verify --schema schema.json --input records.bin --output audit --strict-record
```

Pipeline hex records from stdin:

```text
mainframe-extract | hostlens batch decode --schema schema.json --input - --output jsonl
```

Run a bounded dry run before a full decode:

```text
hostlens batch decode --schema schema.json --input records.bin --output audit --dry-run --max-records 1000
```

## Schema v2

Schema v1 remains compatible and defaults every field to `packed-decimal`.
Schema v2 adds explicit field typing:

```json
{
  "version": 2,
  "record_length": 16,
  "input_encoding": "binary",
  "verification_scope": "record",
  "fields": [
    {
      "name": "account",
      "field_type": "display-text",
      "offset": 0,
      "length": 4,
      "encoding": "cp037"
    },
    {
      "name": "amount",
      "field_type": "packed-decimal",
      "offset": 4,
      "length": 3,
      "total_digits": 4,
      "scale": 2,
      "signed": true,
      "sign_mode": "pfd",
      "mode": "lossless"
    },
    {
      "name": "zoned-tax",
      "field_type": "zoned-decimal",
      "offset": 7,
      "length": 4,
      "total_digits": 4,
      "scale": 2,
      "signed": true
    },
    {
      "name": "sequence",
      "field_type": "binary",
      "offset": 11,
      "length": 4,
      "signed": false
    },
    {
      "name": "flags",
      "field_type": "raw-bytes",
      "offset": 15,
      "length": 1
    }
  ]
}
```

`display-text` fields require `encoding`; unsupported codepages fail during
schema validation rather than falling back silently. Undefined bytes in a table
fail as `E_ENCODING`. `raw-bytes` preserves opaque fields as uppercase hex.
`binary` fields are big-endian COMP/COMP-4 style integers. `batch verify`
requires every decoded field type to re-encode to the exact source bytes; mixed
DBCS verification includes exact SO/SI shift placement.
Mixed DBCS fields must use `field_type: "mixed-dbcs-text"`; they are not
accepted through single-byte `display-text`. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for Unicode ICU mapping data
attribution. Other IBM DBCS or stateful CCSIDs must be added explicitly under
the mixed DBCS decoder path before they are accepted.

List the exact supported text encodings and their required schema field type:

```text
hostlens encodings list
hostlens encodings list --output json
```

## Schema Workflow

HostLens can bootstrap and review schemas without claiming to be a COBOL
compiler:

```text
hostlens schema from-copybook --copybook customer.cpy --encoding cp037
hostlens schema emit-rust --schema customer.schema.json --struct-name CustomerRecord
hostlens schema compare --old old.schema.json --new new.schema.json --output json
```

`schema from-copybook` supports a conservative subset of fixed-width copybook
declarations: level 01 records, nested groups, elementary `PIC X`, display
numeric, `COMP-3`/`PACKED-DECIMAL`, IBM-style `COMP`/`BINARY`/`COMP-4`, and
safe fillers. It rejects constructs that would make byte layout ambiguous in
this release, including `REDEFINES`, `OCCURS`, `SYNC`, level 88 condition
names, separate signs, and COPY expansion.

## Audit And Limits

Important limits:

- Schema files are capped at 1 MiB.
- A schema can contain at most 1,024 fields plus fillers.
- Fixed-width records are capped at 16 MiB.
- Buffered JSON output defaults to 100,000 rows; use `--max-rows 0` for
  unbounded JSON or prefer `jsonl`/`csv` for streaming.
- Single-field COMP-3 operations are capped at 10 bytes, the encoded width of
  the supported 18-digit maximum.

`AuditStatus::Empty` means no records or no fields were processed. Use
`--fail-on-empty` for CI gates. `record_index` is zero-based by default; use
`--one-based-index` for line-number-oriented tools. `--progress` writes to
stderr only, so JSON/CSV/stdout pipelines stay parseable. `--parallel N`
parallelizes fixed-width binary decoding after reading the input into memory;
use the default streaming path for files larger than available RAM.

## Security Posture

The CLI validates schemas before processing data, requires explicit text
encodings, streams default batch inputs, routes malformed records through the
configured `on_error` policy, and emits stable error codes with documentation
URLs. `fail` and `skip-record` modes buffer field rows until the whole record
succeeds, so partial records do not leak.

Release artifacts are built for Linux, macOS, and Windows, with shell
completions, a man page, SBOM metadata, SHA256 checksums, and Sigstore-backed
checksum signatures.

See [docs/cli.md](docs/cli.md) for the full schema format, output contract,
error model, limits, and operational examples. See
[docs/developer-map.md](docs/developer-map.md) for the source layout and the
module ownership map. See [docs/release-readiness.md](docs/release-readiness.md)
for ready-only release checks and the local archive limitations.
