# GnuCOBOL Oracle Validation

The converter uses oracle validation to compare observable behavior between a
reference COBOL compiler and generated Rust. This is a trust-building layer, not
a proof of full dialect equivalence.

`migration-capability-matrix.json` is the canonical feature-status inventory.
Oracle fixtures provide evidence for selected rows in that matrix.

## Running The Oracle Suite

```powershell
cargo test --features converter --test oracle_gnucobol -- --nocapture
```

The tests require `cobc` on `PATH`. If GnuCOBOL is unavailable, external oracle
fixtures are skipped and the harness structure still compiles.

CI treats GnuCOBOL as a required oracle dependency for the oracle job. The job
captures `cobc --version` in the `gnucobol-oracle-evidence` artifact so a
passing oracle result is tied to the exact compiler binary reported by the
runner.

## Current Observables

Oracle coverage is intentionally small today. It compares normalized stdout for:

- condition-name control flow (`IF` with level-88 predicates)
- dynamic `CALL ... USING` by reference
- fixed-length sequential file input
- computed `GO TO DEPENDING ON`
- `STRING ... DELIMITED BY SIZE` into an alphanumeric target
- numeric codec behavior through generated programs
- file status behavior for OS-backed sequential files
- procedure-based `SORT`
- ODO table access and `SEARCH` behavior
- fixed-format COPY expansion

This is not enough to establish migration correctness for the supported VM
surface. Treat the suite as a seed oracle, not a certification suite.

Each fixture is compiled and run twice:

1. GnuCOBOL via `cobc -x -free`
2. `cobol2rust convert --dialect gnucobol --source-format free`, followed by
   `cargo run --offline` in the generated project

The harness writes identical fixture source and file inputs into isolated temp
directories before comparing outputs.

GnuCOBOL is not linked into `cobol2rust`, not vendored, and not bundled in
standard release archives. It is an external executable used to produce oracle
evidence.

## Scope Boundaries

This suite intentionally compares only observable behavior that both runtimes
currently model. It does not claim equivalence for platform-specific behavior
such as VSAM, record locking, EBCDIC datasets, JCL file disposition, or
compiler-specific undefined behavior.

Future oracle slices should add:

- exact stdout comparisons for `ALTER`, `GO TO DEPENDING ON`, procedure-based
  `SORT`, `INSPECT` / `EXAMINE`, `STRING` / `UNSTRING`, and `COMPUTE ... ON
  SIZE ERROR`
- output file byte comparisons
- file status comparisons for OS error cases
- RETURN-CODE / PROGRAM-STATUS comparisons
- generated random programs within the supported subset
- dialect-specific fixture groups for IBM, GnuCOBOL, and Micro Focus behavior
