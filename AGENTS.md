# Agent Guide

HostLens is a mainframe record decoder and forensic audit CLI backed by the
`cobol_packed` Rust crate. It is not a general COBOL program converter.

## Boundaries

- Do not add README or crate claims about translating COBOL programs,
  `COMPUTE`, `PERFORM`, procedure division codegen, or migration reports.
- Schema v2 is the public record-layout interface. Keep schema v1 compatible for
  packed-decimal-only layouts.
- `display-text` is single-byte EBCDIC only. Stateful or mixed DBCS encodings
  must go through `mixed-dbcs-text` with explicit table support.
- Release archives, `.crate` files, checksums, and signed binaries belong in
  GitHub Releases, not in git.

## Verification

Run these before declaring release readiness:

```text
cargo fmt --all --check
cargo test --all-features
cargo test --doc --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo package --allow-dirty
cargo publish --dry-run --allow-dirty
```

Run `cargo test --features cli --test cli_smoke` after CLI or schema workflow
changes. Run Criterion on a quiet machine before publishing performance claims.

## Ownership Map

- CLI command parsing and dispatch: `src/cli/args.rs`, `src/cli/mod.rs`
- Schema validation, planning, hashes: `src/cli/schema.rs`
- Batch decode/verify flow: `src/cli/batch.rs`
- Rendering and row sinks: `src/cli/render.rs`
- Copybook import, Rust emission, schema diff: `src/cli/copybook.rs`,
  `src/cli/schema_emit.rs`, `src/cli/schema_compare.rs`
- Generated codepage data: `src/cli/ebcdic_tables.rs`,
  `src/cli/mixed_dbcs_tables.rs`
