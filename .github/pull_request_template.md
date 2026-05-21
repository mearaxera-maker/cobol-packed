## Summary

<!-- What does this PR do?  One or two sentences. -->

## Motivation

<!-- Why is this change needed?  Link the relevant issue if applicable.
     Closes #... -->

## Changes

<!-- List the main changes. -->

- 

## Checklist

- [ ] `cargo test --all-features` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] New public items have `///` documentation
- [ ] CHANGELOG.md updated under `[Unreleased]`
- [ ] If touching `simd.rs`: `simd_and_scalar_agree` test passes
- [ ] If touching encode/decode: fuzz harness run for ≥ 60 seconds

## Invariants

<!-- Which of the six laws in CONTRIBUTING.md does this PR touch?
     How do you verify they still hold? -->

## Test coverage

<!-- Which new tests were added?  What edge cases do they cover? -->

## Performance impact

<!-- Did you run `cargo bench`?  Paste relevant before/after numbers if so. -->
