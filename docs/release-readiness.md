# HostLens Release Readiness

HostLens releases are reviewed through pull requests, merged to `main`, and
published by tag-driven GitHub Actions. Do not publish directly from a local
workspace unless the GitHub release workflow is unavailable and the maintainer
explicitly approves the fallback.

## Current 1.1.0 Gate

- PR #1 is the review gate for HostLens 1.1.0.
- The PR must stay mergeable and pass CI before merge.
- Binary archives, `.crate` files, checksums, and signatures belong in GitHub
  Release assets, not in git history.
- CodeRabbit is best-effort and should be run from Linux, macOS, or Codespaces
  if the local Windows shell cannot install the CLI.

## Required Repository Settings

These settings require repository owner or administrator access:

- Protect `main`.
- Require pull requests before merging.
- Require CI checks before merging.
- Disallow direct pushes to `main`.
- Add or verify the `CARGO_REGISTRY_TOKEN` Actions secret.
- Enable Dependabot alerts and updates where available.

## Local Verification

Run these commands before marking the release PR ready for review:

```text
cargo fmt --all --check
cargo test --all-features
cargo test --doc --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo package --allow-dirty
cargo publish --dry-run --allow-dirty
```

Run Criterion on a quiet machine before publishing performance claims:

```text
cargo bench --all-features
```

## Release Workflow

1. Merge the reviewed release PR into `main`.
2. Confirm `Cargo.toml` version and changelog entry.
3. Create tag `v1.1.0` from the merged `main` commit.
4. Push the tag.
5. Let `.github/workflows/release.yml` validate the tag, publish to crates.io,
   build platform artifacts, smoke-test the Linux artifact, sign checksums, and
   create the GitHub Release.

If `CARGO_REGISTRY_TOKEN` is missing, do not push the release tag until the
secret is added or the publish job is intentionally disabled for that release.
