---
name: Bug report
about: Something produces incorrect output or panics
title: "[BUG] "
labels: bug
assignees: ''
---

## Summary

<!-- One-line description of what went wrong. -->

## Invariant violated (if known)

<!-- Which law from docs/formal_spec.md or docs/architecture.md is broken?
     E.g. "Lossless identity: encode(decode(b)) ≠ b" or "panic on decode" -->

## Reproduction

```rust
// Minimal, self-contained example that demonstrates the bug.
use cobol_packed::{...};

fn main() {
    // ...
}
```

## Expected behavior

<!-- What should have happened? -->

## Actual behavior

<!-- What actually happened? Include the full panic message or wrong output. -->

## Environment

- `cobol_packed` version:
- Rust toolchain (`rustc --version`):
- OS + architecture:
- Feature flags enabled:
- Target CPU features (`RUSTFLAGS`):

## Additional context

<!-- SIMD-related bugs: note whether AVX2 or SSE2 is active.
     Lossless bugs: include the raw hex bytes and the config used. -->
