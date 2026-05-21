# Repository Guide

This repository is the source for HostLens and the `cobol_packed` Rust crate.
HostLens decodes and audits mainframe data records. It does not translate COBOL
program logic.

## Code

- `main` is the release branch. Changes should arrive through pull requests.
- `codex/hostlens-1.1.0-ready` is the HostLens 1.1.0 release-candidate branch.
- `upgrade/v1.0.0` is an older upgrade branch and should not receive new 1.1
  work unless it is deliberately backported.
- Use `docs/developer-map.md` and `AGENTS.md` to find the right subsystem before
  editing.

## Issues

Use issues for reproducible bugs, planned enhancements, and release follow-up.
Recommended labels:

- `release` for release blockers and packaging work.
- `security` for disclosure-process and hardening work.
- `schema` for schema validation, hashing, and compatibility.
- `copybook` for `schema from-copybook`.
- `performance` for throughput, memory, and benchmark work.
- `docs` for README, CLI guide, and examples.
- `good first issue` for small, bounded changes.
- `post-1.1` for roadmap work after the current release.

## Pull Requests

Every PR should describe the behavioral change, validation commands, and any
release impact. Keep binary artifacts out of PRs; attach them to GitHub
Releases. For schema changes, state whether schema v1 compatibility is affected.

## Actions

CI runs formatting, clippy, full-feature tests, doc tests, feature-combination
checks, CLI smoke tests, benchmark compile checks, fuzz build smoke checks, and
minimal-version checks. A release tag runs the release workflow.

If a PR shows no checks, verify that Actions are enabled for the repository and
that the workflow files are present on the PR branch.

## Security

Use `SECURITY.md` for vulnerability reporting. Do not discuss private
vulnerabilities in public issues before coordinated disclosure. Host data,
copybooks, schemas, and audit reports can contain sensitive business data; do
not add customer data as fixtures.

## Releases

Releases are tag-driven. The tag must match `Cargo.toml`, for example `v1.1.0`
for version `1.1.0`. Before tagging:

- Merge the reviewed release PR into `main`.
- Confirm branch protection and CI status.
- Confirm the repository secret `CARGO_REGISTRY_TOKEN`.
- Run or confirm `cargo publish --dry-run`.

GitHub Release assets should contain platform binaries, completions, man page,
SBOM metadata, SHA256 checksums, and Sigstore checksum signatures.

## Settings

Repository administrators should enable:

- Branch protection on `main`.
- Required PR review and required CI checks.
- Dependabot alerts and updates.
- Secret scanning where available.
- Write access for maintainers who are expected to push release branches.

Keep the Wiki disabled unless docs need to be edited outside pull requests.
The checked-in `docs/` directory is the source of truth.
