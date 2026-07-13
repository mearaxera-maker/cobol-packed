# Packaging And Distribution

Release-candidate archives are produced by `.github/workflows/release.yml` for:

- Linux x86_64: `cobol-packed-linux-x86_64.tar.gz`
- macOS x86_64: `cobol-packed-macos-x86_64.tar.gz`
- Windows x86_64: `cobol-packed-windows-x86_64.zip`

Each archive contains:

- `cobol-packed`
- `cobol2rust`
- shell completions or PowerShell completions where supported
- man page
- `README.md`
- `CHANGELOG.md`
- license and notice files
- `SECURITY.md`
- `SBOM.cargo-metadata.json`

The workflow uploads each archive with a SHA-256 checksum as a GitHub Actions
artifact. It does not publish a GitHub Release and does not publish crates to
crates.io.

## crates.io Boundary

The current upload branch is a full converter workspace, not the old
single-crate COMP-3 package. Internal workspace dependencies carry explicit
versions for package metadata, and the root manifest sets `publish = false` so
`cargo publish` fails closed. Publishing the root crate is not a supported
release step until the first-party workspace crates are published in dependency
order or the converter is split into its own published package.

Use the release-candidate archive workflow for this branch. Use crates.io only
for already-published compatible `cobol_packed` releases whose manifests do not
depend on unpublished workspace crates.

## Local Release Build

```text
cargo build --release --features cli,converter --bin cobol-packed --bin cobol2rust
```

Then package the binaries with:

```text
README.md
CHANGELOG.md
LICENSE
LICENSE-APACHE
LICENSE-MIT
NOTICE
SECURITY.md
```

Generate an SBOM with:

```text
cargo metadata --format-version 1 --locked > SBOM.cargo-metadata.json
```

## Versioning

The project follows semantic versioning:

- MAJOR: breaking API, schema, CLI, or generated-project compatibility changes.
- MINOR: additive supported features and new stable diagnostics.
- PATCH: compatible fixes, documentation, and performance work.

Capability matrix rows marked supported define the stable converter surface for
the release. Experimental rows, chaos modes, and exploratory oracle fixtures are
not stability commitments.

## Installer Work

The current supported redistribution unit is a tarball or zip archive. Native
`.deb`, `.rpm`, and MSI installers are not yet produced by CI. When those are
added, they must include the same license, notice, security, and SBOM artifacts
as the tarball/zip archives.

## Docker Boundary

A Docker image may bundle GnuCOBOL for oracle testing, but such an image is a
separate distribution artifact from the standard binary archives. If built, its
image labels and `/licenses` directory must identify GnuCOBOL and every
additional OS package included in the image.
