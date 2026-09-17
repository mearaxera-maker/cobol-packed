# cobol-packed-deeper Subproject

This directory preserves the earlier `cobol-packed-deeper.zip` package as an
independent provenance snapshot. It is intentionally not a workspace member and
is not fused into the main crate.

Purpose:

- keep the earlier codec/formal-methods snapshot available for comparison;
- avoid mixing old package metadata with the upload-ready product;
- let future work mine proofs, fuzz harnesses, or documentation without
  rewriting history.

It has an empty `[workspace]` table in its manifest so Cargo treats it as a
separate project. The snapshot is not part of the supported build matrix: the
old code currently relies on generic const patterns that are no longer accepted
by the stable toolchain used by the upload-ready project. Treat it as archived
evidence unless a future pass intentionally ports it.

```text
cargo check --manifest-path subprojects/cobol-packed-deeper/Cargo.toml --offline
```

The command above is expected to fail on the current stable toolchain until that
porting work is done.
