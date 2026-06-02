# Contributing to cobol_packed

Thank you for contributing. This document covers the process, invariants that
must not be broken, and the expectations for pull requests.

---

## Before You Start

1. **Read `docs/formal_spec.md`.**  The laws in that document are axioms.  A
   PR that violates any of them will not be merged, regardless of performance
   or convenience arguments.

2. **Read `docs/ARCHITECTURE.md`.**  Understand the scalar truth model and
   why SIMD is a validation layer, not a co-equal decoder.

3. **Open an issue first** for any change larger than a bug fix or docs
   improvement.  Architectural changes, new public APIs, and feature-flag
   additions all need upfront discussion.

---

## Invariants That Must Not Break

These are verified by the test suite, fuzz harnesses, and Kani proofs.  Any
PR touching `src/` must not weaken them:

| # | Law | Verified by |
|---|-----|-------------|
| 1 | Scalar truth: `from_packed == from_packed_scalar` | unit tests, proptest |
| 2 | SIMD parity: `simd_matches_scalar` always true | unit tests, fuzz |
| 3 | Overflow: max = `10^digits - 1`, not `10^digits` | unit tests |
| 4 | No panic: no public API unwinds on any input | fuzz, Kani |
| 5 | Lossless identity: `encode(decode(b)) == b` | proptest, Kani |
| 6 | Policy separation: canonical and lossless zero paths are separate | unit tests |

---

## Development Setup

```bash
# Standard test suite
cargo test

# Property-based tests (longer run)
cargo test -- --include-ignored

# Criterion benchmarks
cargo bench

# Fuzzing (requires nightly + cargo-fuzz)
cargo +nightly fuzz run fuzz_decode -- -max_len=32 -runs=1000000
cargo +nightly fuzz run fuzz_roundtrip -- -max_len=32 -runs=1000000

# Kani proofs (requires cargo-kani)
cargo kani --harness proof_no_panic
cargo kani --harness proof_lossless_roundtrip
```

---

## Pull Request Requirements

- [ ] `cargo test` passes with no warnings
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo audit` passes for dependency advisories
- [ ] `cargo deny check` passes for license, source, and dependency policy
- [ ] `cargo metadata --format-version 1 --locked` can generate the release SBOM
- [ ] New public API items have `///` documentation
- [ ] New behaviour is covered by at least one unit test
- [ ] If touching `simd.rs`: verify `simd_and_scalar_agree` test still passes
- [ ] If touching encode/decode paths: run the fuzz harnesses for ≥ 60 seconds
- [ ] CHANGELOG.md updated under `[Unreleased]`
- [ ] If touching converter capability status: confirm `cargo test --test capability_matrix` passes and no supported feature regressed to blocked
- [ ] If touching oracle fixtures: record `cobc --version` in the evidence or CI log

---

## Semver Policy

`cobol_packed` follows [Semantic Versioning](https://semver.org).

- **Patch**: bug fixes, documentation, performance.
- **Minor**: new public items (additive only).
- **Major**: any removal or behavioural change to existing public API.

The `PackedPolicy`, `SignMode`, `ZeroSignPolicy`, `PackedConfig`, `Packed`,
and `PackedError` types are treated as the core public API for the current
0.x line. Additive APIs are preferred; breaking API changes must be called out
in the changelog and migration guide. New variants may be added to
`PackedError`; match arms should use `_ => …` for forward-compatibility.

---

## Code Style

- No `unwrap()` or `expect()` in library code (`src/lib.rs`, `src/simd.rs`).
- No `abs()` on externally-supplied `Decimal` values (see spec §5).
- SIMD blocks must be gated by runtime feature detection and/or
  `#[target_feature(enable = "...")]`.
- All unsafe blocks require a `// SAFETY:` comment.

---

## Licensing

By submitting a pull request you agree that your contribution is licensed
under both MIT and Apache-2.0, consistent with the project's dual license.

Do not add a dependency, generated artifact, bundled binary, or external tool
runtime without checking `deny.toml`, `NOTICE`, and `docs/compliance.md`.
GnuCOBOL must remain an external oracle executable unless a separate LGPL
compliance review is completed.
