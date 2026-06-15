# Contributing to cobol_packed

Thank you for contributing. This document explains the process, invariants that must not be broken, and expectations for pull requests.

---

## Before You Start

1. Read `docs/formal_spec.md` — the laws in that document are axioms. A PR that violates them will not be merged.
2. Read `docs/architecture.md` — understand the scalar truth model and why SIMD is a validation layer.
3. Open an issue for changes larger than a bug fix or docs improvement.

---

## Invariants That Must Not Break

These invariants are enforced by unit tests, fuzz harnesses, and Kani proofs. Any PR touching `src/` must not weaken them:

1. Scalar truth: `from_packed == from_packed_scalar`
2. SIMD parity: `simd_matches_scalar` always true
3. Overflow: max = `10^digits - 1`
4. No panic: public API must not unwind on arbitrary input
5. Lossless identity: `encode(decode(b)) == b`
6. Policy separation: canonical and lossless zero paths are separate

---

## Developer Setup

```bash
# Install MSRV toolchain
rustup toolchain install 1.74

# Format check
cargo fmt --all -- --check

# Linting
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run tests
cargo test --workspace --all-features

# Fuzzing (requires nightly + cargo-fuzz)
cargo +nightly fuzz run fuzz_decode -- -max_len=32 -runs=100000
cargo +nightly fuzz run fuzz_roundtrip -- -max_len=32 -runs=100000

# Kani proofs (requires cargo-kani)
cargo kani --harness proof_no_panic
cargo kani --harness proof_lossless_roundtrip
```

---

## Pull Request Requirements

- [ ] `cargo test` passes with no warnings
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] New public API items have `///` docs
- [ ] New behaviour is covered by at least one unit test
- [ ] If touching `simd.rs`: verify `simd_and_scalar_agree` test still passes
- [ ] If touching encode/decode paths: run the fuzz harnesses for ≥ 60 seconds
- [ ] CHANGELOG.md updated under `[Unreleased]`

---

## Semver Policy

Follow Semantic Versioning (https://semver.org):

- Patch: bug fixes, docs, performance
- Minor: new public items (additive)
- Major: removal or behavioral changes to existing public API

---

## Code Style

- No `unwrap()` / `expect()` in library code
- No `.abs()` on externally-supplied `Decimal` values
- SIMD blocks must be gated by runtime detection and/or `#[target_feature(...)]`
- All `unsafe` blocks require a `// SAFETY:` comment

---

## Licensing

By submitting a pull request you agree your contribution is dual-licensed under MIT and Apache-2.0.
