# HostLens Release Readiness

This workspace is an extracted local release-candidate archive, not a connected
GitHub checkout. Ready-only release verification can be performed here, but a
live release cannot be completed from this workspace without external setup.

## Confirmed Locally

- `.github/workflows/release.yml` validates tag/version parity.
- The release workflow contains `cargo publish --token
  ${{ secrets.CARGO_REGISTRY_TOKEN }}`.
- Release artifacts are configured for Linux, macOS, and Windows.
- The workflow packages README, changelog, MIT/Apache license files, third-party
  notices, shell completions, a man page, cargo metadata SBOM, SHA256 checksums,
  and Sigstore checksum signatures.

## Not Verifiable From This Archive

- No Git remote is configured in this local workspace.
- No release tags exist in this local workspace.
- GitHub repository secrets cannot be inspected locally, so
  `CARGO_REGISTRY_TOKEN` must be verified in the repository settings before a
  real release.

## Ready-Only Commands

```text
cargo fmt --all --check
cargo test --all-features
cargo test --doc --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo package --allow-dirty
cargo publish --dry-run --allow-dirty
```

Do not create a tag, push, publish to crates.io, or create a GitHub release from
this archive unless that live release is explicitly approved.
