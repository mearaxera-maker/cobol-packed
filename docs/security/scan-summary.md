# Security Scan Summary

This summary is safe for public repository use. Raw local paths from the
working environment are intentionally omitted.

## Threat Model

The primary trust boundary is untrusted mainframe data entering codec and CLI
parsing paths. Attackers or bad upstream systems can provide malformed packed
decimal bytes, oversized fields, invalid sign nibbles, malformed schemas,
truncated records, wrong JSONL/CSV types, hostile OCCURS counters, and records
whose layout claims do not match their byte contents.

Assets worth protecting:

- Host process availability for migration and CI workflows.
- Correctness of decoded financial or compliance data.
- Byte-for-byte forensic evidence in audit reports.
- Release integrity for crates.io and binary artifacts.
- Deterministic machine-readable output used by automation.

## Fixed Findings

| Finding | Risk | Resolution |
| --- | --- | --- |
| Oversized packed input reached SIMD parity validation before exact length rejection | Resource exhaustion and inconsistent error behavior | Decode now validates expected byte length before SIMD/scalar expansion and returns `InvalidByteLength` early |
| Public encode path could panic on wrong output buffer length | No-panic API guarantee violation | Packing is fallible and public APIs return errors instead of panicking |
| SIMD scalar parity assertion existed in production path | Runtime panic under feature builds | Converted to debug-only assertion behavior |
| Legacy downscale truncation was under-documented | Financial migration ambiguity | Strict encode APIs reject precision loss; legacy truncating APIs are documented |
| Explicit sign override on canonical zero could ignore invalid nibble | API surprise and bad forensic input handling | Explicit signs are validated before zero canonicalization |
| Canonical CLI decode created synthetic invalid sign state | Corrupt intermediate evidence risk | CLI decode no longer constructs fake lossless sign state |
| Semantic schema hash included descriptions | False semantic diffs | Semantic hash excludes human descriptions |
| Full evidence mode emitted raw argv by default | Sensitive command-line leakage | Full evidence argv is redacted unless raw argv is explicitly requested |
| JSONL wrong field types were treated like missing data | Silent data-quality loss | Wrong types now emit explicit `E_JSON_TYPE` errors |
| Strict record verification only covered field bytes | Incomplete forensic proof | Coverage map reports fields, fillers, SYNC slack, OCCURS, REDEFINES bases, gaps, and overlaps |

## Current Hardening Controls

- Exact byte-length validation before decode.
- Checked offset arithmetic in Schema v2 planning and decode paths.
- Bounded line and record processing.
- No `unwrap` or `expect` in production CLI paths.
- Stable process exit-code categories.
- Deterministic versioned machine output.
- Recovery modes avoid partial-row emission when records fail.
- Schema validation rejects unsupported or contradictory codec options.
- Generated Rust uses safe slicing and rejects unsupported nested group shapes.

## Verification Performed Locally

The following checks passed in the local Windows environment:

```text
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --lib --no-default-features
cargo test --features simd,avx2
cargo check --test kani_proofs --features kani
cargo check --manifest-path fuzz/Cargo.toml --offline
cargo check --manifest-path fuzz/Cargo.toml --features simd --offline
cargo doc --all-features --no-deps
```

The fuzz and Kani commands above are compile-smoke checks for harnesses. Actual
time-boxed `cargo fuzz run` campaigns and real Kani proofs are configured in
the hosted Linux deep-verification workflow.

## Remaining Security Work

- Run scheduled cargo-fuzz campaigns in Linux CI and archive crash artifacts.
- Run Kani proof harnesses through the Kani GitHub Action.
- Add deeper schema-aware fuzzing for nested layouts as those features expand.
- Review full copybook parsing once that follow-up layer exists.
