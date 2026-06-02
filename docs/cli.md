# `cobol-packed` CLI

`cobol-packed` is the operational CLI for the `cobol_packed` codec. It is
designed for migration engineers and forensic reviewers working with IBM
Enterprise COBOL COMP-3 packed decimal fields.

Install from crates.io:

```text
cargo install cobol_packed --features cli
```

## Field Workbench

Decode a single field:

```text
cobol-packed decode --digits 4 --scale 2 --signed --hex 01234C
```

Single-field commands require exactly one input source: `--hex`, `--file`, or
`--stdin`.

Inspect nibbles and sign handling:

```text
cobol-packed inspect --digits 3 --scale 0 --signed --sign-mode nopfd --hex 000D --output json
```

Encode a value:

```text
cobol-packed encode --digits 6 --scale 2 --signed --value -99.99
```

## Fixed-Width Migration

Schema files are versioned JSON or TOML. A minimal fixed-width schema:

```json
{
  "version": 1,
  "record_length": 4,
  "input_encoding": "binary",
  "on_error": "emit-error-row",
  "verification_scope": "field",
  "output": "jsonl",
  "fields": [
    {
      "name": "amount",
      "offset": 0,
      "length": 3,
      "total_digits": 4,
      "scale": 2,
      "signed": true,
      "sign_mode": "pfd",
      "mode": "lossless"
    }
  ],
  "fillers": [
    {
      "name": "unused-tail",
      "offset": 3,
      "length": 1
    }
  ]
}
```

Run a batch decode:

```text
cobol-packed batch decode --schema schema.json --input records.bin --output jsonl
```

## Schema v2 Record Engine

Schema v1 remains the stable COMP-3 schema. Schema v2 introduces a real COBOL
record-layout model with `layout_mode`:

- `declared`: offsets are explicit absolute byte offsets. `sync` does not move
  fields; coverage reports expose any gaps.
- `sequential`: offsets are omitted and computed by the layout planner. `sync`
  can insert synthetic `sync-slack` ranges.

Supported v2 field types are `packed-decimal`, `zoned-decimal`, `binary`,
`native-binary`, `ibm-float32`, `ibm-float64`, `alphanumeric`, and `filler`.
Binary, native binary, and IBM floats require explicit `endian`. Zoned and
EBCDIC alphanumeric fields require explicit `encoding` and code page where
text conversion is needed; supported code pages are `cp037`, `cp500`,
`cp1140`, and `cp1148`.

Minimal mixed-record v2 schema:

```json
{
  "version": 2,
  "layout_mode": "declared",
  "record_length": 14,
  "input_encoding": "binary",
  "on_error": "emit-error-row",
  "layout": [
    {
      "kind": "field",
      "name": "amount",
      "offset": 0,
      "field_type": "packed-decimal",
      "total_digits": 4,
      "scale": 2,
      "signed": true,
      "sign_mode": "pfd",
      "mode": "lossless"
    },
    {
      "kind": "field",
      "name": "display_amount",
      "offset": 3,
      "field_type": "zoned-decimal",
      "total_digits": 4,
      "scale": 2,
      "signed": true,
      "encoding": "ebcdic",
      "codepage": "cp037",
      "sign_policy": "preferred"
    },
    {
      "kind": "field",
      "name": "count",
      "offset": 7,
      "field_type": "binary",
      "total_digits": 5,
      "signed": false,
      "endian": "big"
    },
    {
      "kind": "field",
      "name": "name",
      "offset": 11,
      "length": 3,
      "field_type": "alphanumeric",
      "encoding": "ebcdic",
      "codepage": "cp037"
    }
  ]
}
```

`kind: "occurs"` supports OCCURS DEPENDING ON for binary streaming. The counter
field must be a preceding scalar field; out-of-range counts fail with
`E_OCCURS_COUNT` and are not clamped. `count = 0` consumes zero element bytes.
Sequential ODO groups must currently be terminal because following offsets
would depend on the runtime count. Declared layouts may place fixed-offset
suffix fields after the ODO group's maximum reserved range.

`kind: "redefines"` decodes every variant from the same immutable byte window.
No generated or runtime code uses `unsafe`, `union`, `transmute`, or raw
pointers for REDEFINES.

Generate a safe Rust record view:

```text
cobol-packed schema emit-rust --schema schema-v2.json --output record.rs
```

The generated view stores `&[u8]` and exposes safe raw, hex, and typed
accessors. Packed and zoned decimals return `rust_decimal::Decimal`, binary
fields return `i64`, `u64`, or `Decimal` when scaled, IBM hexadecimal floats
return `f64`, and text fields return owned `String` values. REDEFINES groups
generate safe byte-window views for each variant. Selector-backed REDEFINES
views retain the full record slice so `selected()` can evaluate discriminator
fields outside the overlay window. Top-level OCCURS groups generate
`Vec<Element>` accessors that validate the counter range and actual record
length before slicing occurrences. Generated Rust intentionally rejects nested
group shapes it cannot yet represent, such as OCCURS inside a REDEFINES
variant, instead of emitting partial accessors.

Generate a strict flat Schema v2 draft from a simple copybook:

```text
cobol-packed schema from-copybook --input record.cpy --output schema-v2.json --record-length 120
```

The importer handles flat `PIC X`, DISPLAY numeric/zoned decimal, `COMP-3`, and
mainframe binary `COMP`/`COMP-4`/`BINARY` fields. It rejects clauses that need
compiler-grade layout semantics, including `REDEFINES`, `OCCURS`, `SYNC`,
separate signs, and justified fields. Model those directly in Schema v2.

Compare two schemas before migration:

```text
cobol-packed schema compare --left old.json --right new.json --output json
```

The compare command reports added, removed, and changed field paths using the
planned layout and codec semantics, not source formatting.

For bounded sampling of very large files:

```text
cobol-packed batch decode --schema schema.json --input records.bin --output audit --max-records 1000
```

Run forensic verification:

```text
cobol-packed batch verify --schema schema.json --input records.bin --output audit
```

For record-level proof:

```text
cobol-packed batch verify --schema schema.json --input records.bin --output audit --strict-record
```

`--strict-record` requires every packed field to round-trip losslessly and every
fixed-width byte to be covered by either a packed field or a `fillers` range.
Schemas may also set `"verification_scope": "record"` to require full coverage
at schema-check time. The default remains `"field"` for compatibility.

`--sample-failures N` controls how many failure samples are retained in audit
output. The default is 25 and the maximum is 1000.

## Schema Check

`schema check` validates schema structure before any data is processed. The
machine output includes a derived field plan so operators can confirm the
relationship between record layout and codec settings:

- effective packed byte length for each field;
- offset and end offset for fixed-width binary/hex inputs;
- digit count, scale, signedness, sign mode, and canonical/lossless mode;
- filler ranges and record coverage summary, including covered bytes, gap
  ranges, overlap count, and full-coverage verdict.

Name-based inputs (`csv` and `jsonl`) reject offsets because offsets would be
ignored at runtime. They also reject `record_length` because records are keyed
by field name, not fixed byte position. Fixed-width inputs (`binary` and `hex`)
require exact `record_length`, non-overlapping offsets, and field lengths that
match `total_digits`. A schema can define at most 1024 fields.

## Audit Output

Audit output is versioned and deterministic. It includes:

- `tool` and `tool_version`;
- canonical semantic schema hash, raw schema file SHA-256, field/filler count,
  record length, and input encoding. Human-readable field and filler
  `description` text is excluded from the semantic hash;
- input path, byte size, and SHA-256;
- optional runtime evidence when `--evidence-mode full` is selected;
- optional `record_limit`;
- record and field counters;
- `status`: `passed`, `failed`, or `empty`;
- `field_byte_for_byte_verified`, `record_byte_for_byte_verified`, and
  compatibility `byte_for_byte_verified` for verify runs;
- fixed-width record coverage, gaps, overlaps, and coverage verdict;
- sign distribution, negative zero count, non-preferred sign count;
- global error-code distribution;
- per-field profiles with valid/invalid counters, min/max values, sign
  distribution, error distribution, negative zero count, and non-preferred sign
  count;
- first N structured failure samples.

Failure samples include `record_index`, `field`, `offset`, `raw_hex`,
`raw_byte_len`, `raw_hex_truncated`, `error_code`, `message`, and
`recoverable`.

Streaming `batch decode` output (`jsonl`, `csv`, and `table`) does not pre-hash
the input file. The input SHA-256 is computed for audit/profile/verify reports
where evidence metadata is part of the output contract.

Default audit output is deterministic. `--evidence-mode full` intentionally
adds current directory, platform, generation time, and argv evidence. Argv is
redacted by default because command lines can contain file paths, field names,
or credentials. Use `--evidence-argv raw` only for controlled evidence bundles;
use `--evidence-argv omit` to suppress argv entirely.

## COMP-3 Sign Notes

PFD decoding accepts `0xC` as a positive sign for unsigned fields because this
appears in real COBOL datasets. Canonical unsigned encoding emits `0xF`.
Therefore canonical verification can flag unsigned fields whose original bytes
use `0xC`; use lossless mode for byte-for-byte forensic preservation, or
canonical mode when normalization to preferred unsigned signs is intended.

## Error Model

Machine-readable errors include a stable `error_code`.

- `E_LENGTH`: byte length mismatch.
- `E_SIGN`: invalid sign nibble or negative unsigned value.
- `E_DIGIT`: invalid digit nibble.
- `E_PADDING`: invalid even-digit padding nibble.
- `E_SCHEMA`: invalid schema or field configuration.
- `E_CONFIG`: invalid CLI configuration.
- `E_CSV`: malformed CSV input.
- `E_JSON`: malformed JSONL input.
- `E_JSON_TYPE`: JSONL field value is present but is not a string.
- `E_RECORD_LENGTH`: record length does not match schema.
- `E_OCCURS_COUNT`: OCCURS DEPENDING ON counter is invalid or out of range.
- `E_FLOAT`: IBM hexadecimal floating-point decode failed.
- `E_ENCODING`: text/codepage conversion failed.
- `E_IO`: input/output failure.
- `E_INTERNAL`: unexpected internal failure.

Exit codes:

- `0`: success.
- `1`: data validation error.
- `2`: schema/configuration error.
- `3`: I/O error.
- `4`: internal error.

## Security Notes

The CLI validates expected field and record lengths before decoding. Hex
records must match schema `record_length`; single-field input is capped by the
expected packed length; CSV and JSONL records are read with bounded physical
line buffers. The codec also validates field length before optional SIMD parity
checks, so oversized malformed input is rejected before full-slice nibble
expansion.

`batch verify` exits with code `1` when verification completes but the audit
status is `failed`, including when `on_error` is `emit-error-row`.
Malformed record-level samples retain bounded raw previews in `raw_hex` so
reviewers can identify the offending source bytes without retaining whole
records.

## Internal Pipeline

The CLI is intentionally structured as a pipeline:

```text
command args
  -> schema load
  -> schema validation
  -> field plan derivation
  -> streaming record reader
  -> field extraction
  -> codec decode / verify
  -> audit observer
  -> row sink
```

The field plan is the relationship point between schema and codec. It derives
the validated `PackedConfig`, expected byte length, field offset, sign mode, and
lossless/canonical behavior once, then all decode, verify, and error reporting
paths use those same facts.

Row sinks keep output behavior explicit. JSONL, CSV, and table output stream
row-by-row; JSON output intentionally buffers so it can emit one valid JSON
array and is capped to prevent accidental memory exhaustion; audit output
observes rows without storing every successful field.

Malformed record-level inputs, such as invalid hex lines or bad JSONL objects,
are routed through the same `on_error` policy as field-level decode errors.
For `fail` and `skip-record`, successful field rows are buffered until the
entire record passes, so a later bad field cannot leak partial migration output.

## Operator Artifacts

The binary can generate shell completions and a man page directly from the Clap
command definition:

```text
cobol-packed completions bash > cobol-packed.bash
cobol-packed completions powershell > cobol-packed.ps1
cobol-packed man > cobol-packed.1
```
