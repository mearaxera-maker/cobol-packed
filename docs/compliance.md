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
cargo deny check
cargo audit
cargo metadata --format-version 1 --locked
```

`cargo metadata` output is retained as `SBOM.cargo-metadata.json`. It records
the exact resolved package graph, dependency edges, declared licenses, features,
targets, and source locations for the lockfile used in the build.

## GnuCOBOL Boundary

GnuCOBOL is used only by the oracle test job as an external executable
(`cobc`). The converter does not link to GnuCOBOL libraries, does not vendor
GnuCOBOL, and release archives do not bundle it.

If future work links to `libcob` or bundles GnuCOBOL, that is a different
distribution model and must trigger LGPL compliance review before release.

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

## Release Compliance Checklist

Before publishing or redistributing:

1. Run `cargo metadata --format-version 1 --locked` and archive the SBOM.
2. Run `cargo deny check`.
3. Run `cargo audit`.
4. Confirm release packages include `LICENSE`, `LICENSE-APACHE`, `LICENSE-MIT`,
   `NOTICE`, `SECURITY.md`, `README.md`, and `CHANGELOG.md`.
5. Confirm the GnuCOBOL oracle job records `cobc --version` and does not bundle
   GnuCOBOL into release archives.
6. Review `migration-capability-matrix.json` changes for supported-feature
   regressions.
7. Complete trademark clearance before using a new external product name.
