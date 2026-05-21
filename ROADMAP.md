# Roadmap

This roadmap tracks work after the HostLens 1.1.0 release candidate. It is not
a promise that every item ships in the next release.

## Near Term

- Expand `schema from-copybook` for more fixed-width data-description entries
  without attempting full COBOL procedure semantics.
- Replace memory-buffered parallel binary decode with bounded streaming worker
  queues before recommending it for very large extracts.
- Add regression fixtures for real customer-like mixed records with text,
  packed decimal, zoned decimal, binary, raw bytes, and fillers.
- Re-run throughput benchmarks on quiet CI hardware and publish conservative
  records/sec and MiB/sec numbers.

## Data Engineering Outputs

- Add Parquet output for warehouse ingestion.
- Add SQL `INSERT` output for small migrations and fixture generation.
- Keep JSONL and CSV streaming-safe; avoid output modes that require buffering
  full multi-GB files unless the user opts in.

## Language And Platform Bindings

- Add Python bindings with PyO3 for migration scripts.
- Add WebAssembly packaging for browser-side record inspection.
- Keep the Rust crate API as the source of truth for packed decimal behavior.

## Encoding Coverage

- Keep single-byte EBCDIC under `display-text`.
- Add additional IBM DBCS or stateful encodings only through explicit
  `mixed-dbcs-text` tables and golden tests.
- Do not silently map unknown CCSIDs to CP037.

## Repository And Release Operations

- Move generated archives, `.crate` packages, checksums, and signed binaries to
  GitHub Releases.
- Require CI before merging to `main`.
- Verify `CARGO_REGISTRY_TOKEN` before creating a release tag.
- Run CodeRabbit from a Linux, macOS, or Codespace environment where the CLI and
  auth flow are available.
