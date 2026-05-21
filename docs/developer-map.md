# Developer Map

HostLens keeps the stable COMP-3 codec in the library and the operational
record decoder in the `cli` feature. Start here when changing behavior:

- `src/lib.rs`: public Rust API, packed-decimal encode/decode, lossless
  round trips, and core invariants.
- `src/simd.rs`: optional SIMD nibble expansion used as a validation aid.
- `src/cli/args.rs`: Clap command tree and operator-facing flags.
- `src/cli/audit.rs`: decoded row evidence, audit reports, counters,
  throughput metrics, progress summaries, and runtime evidence.
- `src/cli/batch.rs`: batch command handlers and record readers/processors for
  binary, hex, CSV, JSONL, dry-run, stdin, and parallel fixed-width binary.
- `src/cli/constants.rs`: output contract versions, limits, and docs URLs.
- `src/cli/copybook.rs`: limited COBOL copybook declaration parser and schema
  v2 bootstrapper.
- `src/cli/ebcdic.rs`: explicit display-text byte mapping for supported
  EBCDIC identifiers.
- `src/cli/ebcdic_tables.rs`: generated 256-byte Unicode tables for supported
  SBCS EBCDIC codepages.
- `src/cli/encoding_catalog.rs`: user-facing catalog for supported text
  encodings, accepted aliases, and required schema field types.
- `src/cli/error.rs`: machine-readable CLI errors and process exit codes.
- `src/cli/field_decode.rs`: per-field decode dispatch for packed decimal,
  display text, zoned decimal, binary, and raw bytes.
- `src/cli/mixed_dbcs.rs`: SO/SI stateful mixed DBCS display-field decoder.
- `src/cli/mixed_dbcs_tables.rs`: generated DBCS pair-to-Unicode tables from
  Unicode ICU `.ucm` mapping files.
- `src/cli/render.rs`: row sinks and table, JSON, JSONL, CSV, inspect, and
  audit rendering.
- `src/cli/schema.rs`: schema structs, semantic hashing, validation, field
  planning, and record coverage summaries.
- `src/cli/schema_compare.rs`: semantic schema diff engine and compare output
  rendering.
- `src/cli/schema_emit.rs`: Rust source generator for schema constants and
  decoded-value structs.
- `src/cli/mod.rs`: command dispatch, shared CLI enums, single-field helpers,
  hex/sign utilities, and packed-error mapping.
- `tests/cli_smoke.rs`: end-to-end CLI contract tests for schema validation,
  mixed-record decoding, audit output, stdin, row caps, and release artifacts.
- `benches/packed_bench.rs`: Criterion latency and throughput benchmarks.
- `fuzz/fuzz_targets/`: libFuzzer harnesses for codec and schema/record layout
  stress cases.

Keep future behavior changes in the owning module above. `src/cli/mod.rs`
should remain glue and shared definitions rather than accumulating schema,
batch, audit, or rendering logic again.
