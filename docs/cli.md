# HostLens CLI

`hostlens` is the operational CLI for the `cobol_packed` codec. It is designed
for migration engineers and forensic reviewers working with mixed mainframe
records: COMP-3 packed decimal, EBCDIC display text, mixed DBCS text, zoned
decimal, binary integers, and raw audit bytes. The older `cobol-packed` binary
is still shipped as a compatibility alias.

Install from crates.io:

```text
cargo install cobol_packed --features cli
```

## Field Workbench

Decode a single field:

```text
hostlens decode --digits 4 --scale 2 --signed --hex 01234C
```

Single-field commands require exactly one input source: `--hex`, `--file`, or
`--stdin`.

Inspect nibbles and sign handling:

```text
hostlens inspect --digits 3 --scale 0 --signed --sign-mode nopfd --hex 000D --output json
```

Encode a value:

```text
hostlens encode --digits 6 --scale 2 --signed --value -99.99
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

Schema v2 adds mixed-record field typing. Schema v1 fields default to
`"packed-decimal"` so existing schemas continue to work.

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

`display-text` fields require an explicit EBCDIC `encoding`. Supported
canonical identifiers are `cp037`, `cp273`, `cp277`, `cp278`, `cp280`,
`cp284`, `cp285`, `cp290`, `cp297`, `cp420`, `cp423`, `cp424`, `cp500`,
`cp833`, `cp838`, `cp870`, `cp871`, `cp875`, `cp880`, `cp905`, `cp924`,
`cp1025`, `cp1026`, `cp1047`, and `cp1140` through `cp1149`; `ibmNNN`,
`ccsidNNN`, leading-zero, and Windows codepage-number aliases are also
accepted. Unsupported encodings fail schema validation. Undefined bytes in a
table fail as `E_ENCODING` instead of being silently guessed.

Mixed DBCS fields must use `field_type: "mixed-dbcs-text"` with `cp930`,
`cp933`, `cp935`, `cp937`, or `cp939`. That path validates SO/SI shift state,
decodes the SBCS side through the paired EBCDIC table, and decodes DBCS pairs
through generated Unicode ICU `.ucm` glyph tables. These encodings are rejected
from single-byte `display-text` fields so host DBCS data cannot be silently
treated as byte-per-character text.

Use the encoding catalog when writing or reviewing schemas:

```text
hostlens encodings list
hostlens encodings list --output json
```

The catalog reports each canonical encoding, the required `field_type`, the
byte model, accepted aliases, and notes. In particular, `cp930`, `cp933`,
`cp935`, `cp937`, and `cp939` are listed as `mixed-dbcs-text`, not
`display-text`.

Other IBM DBCS or stateful CCSIDs, such as CP942 or CP5026-style profiles, are
not exposed in 1.0. They must be added explicitly from authoritative mapping
tables under the mixed DBCS decoder path; they must not be treated as generic
single-byte `display-text`.

`zoned-decimal` decodes EBCDIC zoned numeric bytes, `binary` decodes 1-, 2-,
4-, or 8-byte big-endian COMP/COMP-4 style integers, and `raw-bytes` fields
emit uppercase hex in the `value` field and can participate in record coverage.

For the schema above, this hex-encoded record represents account `ACCT`,
amount `12.34`, zoned tax `-12.34`, sequence `1000`, and raw flags `AA`:

```text
C1C3C3E301234CF1F2F3D4000003E8AA
```

If the schema uses `"input_encoding": "hex"`, it can be decoded directly:

```text
hostlens batch decode --schema mixed-schema.json --input records.hex --output jsonl
```

Run a batch decode:

```text
hostlens batch decode --schema schema.json --input records.bin --output jsonl
```

Use `--input -` to read batch data from stdin.

For bounded sampling of very large files:

```text
hostlens batch decode --schema schema.json --input records.bin --output audit --max-records 1000
```

Run forensic verification:

```text
hostlens batch verify --schema schema.json --input records.bin --output audit
```

For record-level proof:

```text
hostlens batch verify --schema schema.json --input records.bin --output audit --strict-record
```

`--strict-record` requires every decoded field to round-trip to the original
bytes and every fixed-width byte to be covered by either a field or a `fillers`
range. Packed decimal, zoned decimal, binary, display text, mixed DBCS text,
and raw-byte fields all participate in verification; display text and mixed
DBCS text must re-encode to the exact source bytes, including SO/SI shift
placement for mixed DBCS. Schemas may also set `"verification_scope": "record"`
to require full coverage at schema-check time. The default remains `"field"`
for compatibility.

`--sample-failures N` controls how many failure samples are retained in audit
output. The default is 25 and the maximum is 1000.

`--fail-on-empty` turns an otherwise successful empty audit into exit code `1`.
`--one-based-index` emits record indices starting at `1` instead of `0`.
`--progress` writes a final progress summary to stderr. `--dry-run` validates
record framing and counts records without field decode. `--quiet` suppresses
table headers. `--parallel N` parallelizes fixed-width binary field decode and
preserves output order; it reads that binary input into memory before chunking,
so keep the default streaming path for files larger than available RAM.
Buffered JSON output is capped at 100,000 rows by default; `--max-rows N`
changes the cap and `--max-rows 0` removes it. Prefer `jsonl` or `csv` for very
large outputs.

## Schema Check

`schema check` validates schema structure before any data is processed. The
machine output includes a derived field plan so operators can confirm the
relationship between record layout and codec settings:

- effective byte length and field type for each field;
- offset and end offset for fixed-width binary/hex inputs;
- digit count, scale, signedness, sign mode, and canonical/lossless mode;
- filler ranges and record coverage summary, including covered bytes, gap
  ranges, overlap count, and full-coverage verdict.

Name-based inputs (`csv` and `jsonl`) reject offsets because offsets would be
ignored at runtime. They also reject `record_length` because records are keyed
by field name, not fixed byte position. Fixed-width inputs (`binary` and `hex`)
require exact `record_length` and non-overlapping offsets. Packed-decimal
lengths must match `total_digits`; zoned-decimal length must match
`total_digits`; display-text, binary, and raw-byte fields require explicit
`length`. A schema can define at most 1024 fields plus fillers.

## Schema Workflow

Bootstrap a schema v2 draft from a limited COBOL copybook subset:

```text
hostlens schema from-copybook --copybook customer.cpy --encoding cp037
```

The generated schema is written to stdout by default. Use `--output
customer.schema.json` to write a file. `--record-name` selects one level 01
record when a copybook contains multiple records. `--input-encoding`,
`--on-error`, and `--verification-scope` set the corresponding generated schema
fields. Diagnostics are written to stderr; `--diagnostics json` emits
newline-delimited diagnostic objects.

The copybook importer is intentionally conservative. It supports level 01
records, nested groups, elementary `PIC X`, `PIC A`, display numeric,
`COMP-3`/`PACKED-DECIMAL`, IBM-style big-endian `COMP`/`BINARY`/`COMP-4`, and
safe fillers. It rejects constructs that need a fuller COBOL layout model:
`REDEFINES`, `OCCURS`, `OCCURS DEPENDING ON`, `SYNC`, level 66/77/88,
`SIGN IS SEPARATE`, edited pictures, `COPY`, and compiler directives.

Generate Rust scaffolding from a validated schema:

```text
hostlens schema emit-rust --schema customer.schema.json --struct-name CustomerRecord
```

The emitted Rust is a decoded-value struct plus offset/length constants. It is
not a zero-copy COBOL memory layout, does not use `unsafe`, and does not emit
`#[repr(C)]`.

Compare schema versions semantically:

```text
hostlens schema compare --old old.schema.json --new new.schema.json --output json
```

`schema compare` ignores descriptions, output preferences, and JSON/TOML
formatting. It reports record length, input encoding, verification scope,
field, filler, offset, length, type, encoding, digit, scale, sign, and required
flag differences. It exits `0` for no relevant diff and `1` when valid schemas
differ according to `--fail-on`.

## Audit Output

Audit output is versioned. It includes:

- `tool` and `tool_version`;
- canonical semantic schema hash, raw schema file SHA-256, field/filler count,
  record length, and input encoding;
- input path, byte size, and SHA-256;
- optional runtime evidence when `--evidence-mode full` is selected;
- optional `record_limit`;
- `elapsed_ms`, `bytes_per_sec`, `records_per_sec`, and `fields_per_sec`;
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
`raw_byte_len`, `raw_hex_truncated`, `error_code`, `error_docs_url`,
`message`, and `recoverable`. `recoverable` means the source bytes were
preserved in a structured row so processing can continue according to
`on_error`; for `E_VERIFY`, the decoded value is readable but did not reproduce
the exact original bytes. `sign_class` values are `positive`, `negative`,
`unsigned-positive`, or `invalid`; non-preferred positive and negative sign
nibbles are counted separately in `non_preferred_sign_count`.

Streaming `batch decode` output (`jsonl`, `csv`, and `table`) does not pre-hash
the input file. The input SHA-256 is computed for audit/profile/verify reports
where evidence metadata is part of the output contract.

`status: "empty"` means no records or no fields were processed. Use
`--fail-on-empty` for CI jobs where empty input should fail.

## Error Model

Machine-readable errors include a stable `error_code` and `error_docs_url`.

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
- `E_OUTPUT_LIMIT`: buffered JSON output reached its row cap.
- `E_EMPTY`: `--fail-on-empty` was requested and no records were processed.
- `E_ENCODING`: display-text bytes could not be decoded by the selected
  EBCDIC encoding.
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

Other hard limits: schema files are capped at 1 MiB, fixed-width records at
16 MiB, individual input lines at 1 MiB, and single-field COMP-3 operations at
10 bytes, the packed width of the supported 18-digit maximum.

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

The field plan is the relationship point between schema and decode behavior. It
derives the field type, expected byte length, field offset, packed-decimal
configuration, EBCDIC encoding, sign mode, and lossless/canonical behavior
once, then all decode, verify, and error reporting paths use those same facts.

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
hostlens completions bash > hostlens.bash
hostlens completions powershell > hostlens.ps1
hostlens man > hostlens.1
```
