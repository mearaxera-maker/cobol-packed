# Compliance And SBOM

This repository uses the SPDX license expression `MIT OR Apache-2.0` for all
first-party workspace crates, including `cobol-record`.

This document is an engineering compliance checklist, not legal advice.
Redistributors should run their own legal review before shipping binaries or
managed-service offerings.

## First-Party License Position

- Project license expression: `MIT OR Apache-2.0`.
- Full license texts: `LICENSE`, `LICENSE-APACHE`, and `LICENSE-MIT`.
- Attribution summary: `NOTICE`.
- Contribution terms: `CONTRIBUTING.md`.
- Vulnerability reporting: `SECURITY.md`.
- Trademark review gate: `docs/trademark.md`.

`cobol-record`, the packed-decimal and record-layout crate used by generated
converter projects, is a first-party workspace crate and declares
`MIT OR Apache-2.0` in its `Cargo.toml`.

## Third-Party Dependency Policy

`deny.toml` allows the current permissive dependency set:

- Apache-2.0
- BSD-2-Clause
- MIT
- Unicode-3.0
- Zlib

The policy denies unknown registries and unknown git dependencies. CI runs:

```text
cargo metadata --format-version 1 --locked
python scripts/check-release-readiness.py
python scripts/check-licenses.py
python scripts/check-gnucobol-boundary.py
python scripts/check-docker-boundary.py
python scripts/generate-third-party-inventory.py
python scripts/check-release-package.py --path dist --platform linux
cargo deny check
cargo audit
```

`cargo metadata` output is retained as `SBOM.cargo-metadata.json`. The
third-party inventory is retained as `third-party-inventory.json`. Together they
record the exact resolved package graph, per-workspace direct dependencies,
transitive dependency edges, declared licenses, features, targets, source
locations, and external tools used by CI/release verification.

## GnuCOBOL Boundary

GnuCOBOL is used only by the oracle test job as an external executable
(`cobc`). The converter does not link to GnuCOBOL libraries, does not vendor
GnuCOBOL, and release archives do not bundle it.

If future work links to `libcob` or bundles GnuCOBOL, that is a different
distribution model and must trigger LGPL compliance review before release.
`scripts/check-gnucobol-boundary.py` is the automated guard for accidental
linking or crate dependency drift.

Reference material:

- GNU license overview: https://www.gnu.org/licenses/
- GnuCOBOL project documentation: https://gnucobol.sourceforge.io/
- SPDX license expressions: https://spdx.dev/learn/handling-license-info/

## Future Storage Backends

Do not add a VSAM or indexed-file backend dependency without a license review.
Known candidates such as RocksDB or sled must be evaluated for:

- license compatibility with `MIT OR Apache-2.0`
- bundled native libraries
- transitive C/C++ dependencies
- binary redistribution notices
- platform support and security update policy

`scripts/check-licenses.py` fails closed for `rocksdb`, `librocksdb-sys`, and
`sled`. If one of those crates becomes a deliberate backend dependency, update
the policy only after recording the legal review, native-library redistribution
model, required notices, and supported platform matrix.

## Release Compliance Checklist

Before publishing or redistributing:

1. Run `cargo metadata --format-version 1 --locked` and archive the SBOM.
2. Run `python scripts/generate-third-party-inventory.py` and archive the
   third-party inventory.
3. Run `python scripts/check-licenses.py`.
4. Run `python scripts/check-gnucobol-boundary.py`.
5. Run `cargo deny check`.
6. Run `cargo audit`.
7. Confirm release packages include `LICENSE`, `LICENSE-APACHE`, `LICENSE-MIT`,
   `NOTICE`, `SECURITY.md`, `README.md`, `CHANGELOG.md`,
   `SBOM.cargo-metadata.json`, `third-party-inventory.json`, installation
   instructions, oracle-boundary documentation, trademark review guidance, and
   generated Rust sample source.
   `scripts/check-release-package.py` validates the archived Rust package
   licenses, `cobol-record` licensing, SBOM/inventory consistency, and the
   standard-release GnuCOBOL non-bundling boundary.
8. Confirm the GnuCOBOL oracle job records `cobc --version` and does not bundle
   GnuCOBOL into release archives.
9. Review `migration-capability-matrix.json` changes for supported-feature
   regressions.
10. Complete the trademark review in `docs/trademark.md` before using a new
    external product name.
