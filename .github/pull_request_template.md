## Summary

<!-- What does this PR do? One or two sentences. -->

## Motivation

<!-- Why is this change needed? Link the relevant issue if applicable.
     Closes #... -->

## Changes

<!-- List the main changes. -->

-

## Checklist

- [ ] `cargo test --all-features` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] New public items have `///` documentation
- [ ] CHANGELOG.md updated under `[Unreleased]` or the target release
- [ ] Release archives and `.crate` files are not committed to git
- [ ] If touching `simd.rs`: `simd_and_scalar_agree` test passes
- [ ] If touching encode/decode: fuzz harness run for at least 60 seconds

## Invariants

<!-- Which laws in docs/formal_spec.md or docs/architecture.md does this PR
     touch? How do you verify they still hold? -->

## Test coverage

<!-- Which new tests were added? What edge cases do they cover? -->

## Performance impact

<!-- Did you run `cargo bench`? Paste relevant before/after numbers if so. -->

## Release impact

<!-- State whether this affects crates.io, GitHub Release assets, CLI output,
     schema compatibility, or docs.rs. -->
