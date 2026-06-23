# Security Policy

## Reporting Vulnerabilities

If you discover a correctness, panic-safety, overflow, SIMD divergence,
forensic-integrity, converter miscompilation, fail-open migration, supply-chain,
or packaging issue in this repository, please report it privately before public
disclosure.

Priority classes:

1. Scalar/SIMD divergence
2. Overflow law violations
3. Negative-zero corruption
4. Invalid nibble acceptance
5. Panic paths
6. Endianness inconsistencies
7. Converter emits Rust for a source construct that should have been blocked
8. Generated Rust changes observable COBOL behavior for a supported feature
9. Release artifact omits required license, notice, SBOM, or checksum evidence
10. Dependency advisory or license-policy regression

Open a GitHub security advisory or contact the maintainer directly.

## Supported Security Gates

CI is expected to run:

- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`
- `cargo audit`
- `cargo deny check`
- `python scripts/check-release-readiness.py`
- GnuCOBOL oracle tests on Linux with `cobc --version` captured as evidence
- fuzz harness build checks, with longer campaigns in deep verification

Release archives should include license files, `NOTICE`, `SECURITY.md`,
`ENTERPRISE.md`, trademark review guidance, a Cargo metadata SBOM, and the
third-party inventory generated from the locked dependency graph.
