# cobol_packed

`cobol_packed` is a Rust workspace that contains a forensic-grade COMP-3 (packed decimal) codec and an early COBOL→Rust converter preview. This reorganization collects the crates, binaries, tests, and tools into a single workspace and documents how to build and verify the project.

Bad data and migration edge-cases are the reason this project exists: negative-zero handling, overflow boundaries, non-preferred sign nibble acceptance, and SIMD/scalar parity are subtle and easy to get wrong. This workspace prioritizes correctness, reproducible CI, and a developer experience for both library users and migration engineers.

Badges: (status, license, msrv)

- MSRV: Rust 1.74
- License: MIT OR Apache-2.0

Quick start — developer (build the whole workspace):

```bash
# Ensure you have rustup with 1.74 toolchain
rustup toolchain install 1.74
rustup default 1.74

# Build + test workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Install the CLI binary (recommended):

```bash
cargo install --path crates/cobol-packed --features cli
```

Run the converter preview:

```bash
cargo run --package cobol_packed --features converter --bin cobol2rust -- convert \
  --input program.cbl \
  --copybook-dir copybooks \
  --out generated-rust \
  --dialect ibm \
  --source-format fixed
```

Files & layout

- crates/cobol-packed — the packed-decimal codec and CLI
- crates/cobol-codegen-rust — converter frontend and codegen (preview)
- crates/* — other internal crates (ir, platform, record, python bindings)
- examples/ — small usage examples
- fuzz/ — cargo-fuzz targets
- kani/ — Kani proofs harness
- benches/ — Criterion benchmarks

CI & verification notes

- The workspace provides multiple verification levels: fast PR CI (fmt, clippy, unit tests), scheduled deep-verification (fuzz campaigns + Kani proofs), and release workflows that build artifacts and SBOMs.
- The deep verification jobs are expensive; they are run on schedule and by manual dispatch to avoid excessive push-costs.

Security & supply chain

- We keep a repo-level Cargo.lock to improve reproducibility.
- Run `cargo audit` locally or in CI (`cargo install cargo-audit`).

Where to look next

- docs/architecture.md — design and invariant description
- docs/cli.md — CLI usage and schema reference
- docs/converter.md — converter preview status and supported subset
- CHANGELOG.md — notable changes and releases
- CONTRIBUTING.md — contributor requirements and invariants

