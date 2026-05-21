---
name: Bug report
about: Something produces incorrect output, rejects valid input, or panics
title: "[BUG] "
labels: bug
assignees: ''
---

## Summary

<!-- One-line description of what went wrong. -->

## Area

<!-- Codec, HostLens CLI, schema validation, copybook import, EBCDIC/DBCS,
     audit output, release packaging, docs, or other. -->

## Reproduction

```text
# Minimal command, schema, input bytes, or Rust snippet that reproduces it.
```

## Expected behavior

<!-- What should have happened? -->

## Actual behavior

<!-- What happened instead? Include full error JSON or panic text. -->

## Invariant violated, if known

<!-- Example: lossless identity `encode(decode(bytes)) == bytes`, no panic on
     decode, schema hash ignores descriptions/output, or exact record coverage. -->

## Environment

- `cobol_packed` version:
- HostLens version (`hostlens --version`):
- Rust toolchain (`rustc --version`):
- OS + architecture:
- Feature flags:

## Data sensitivity

<!-- Do not paste real customer host data. Redact or synthesize fixtures. -->
