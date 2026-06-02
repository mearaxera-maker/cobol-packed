# Installation And Verification

This repository contains two product surfaces:

- `cobol-packed`: the record codec and migration CLI.
- `cobol2rust`: the conservative COBOL-to-Rust converter.

Both are Rust binaries. The converter also generates standalone Rust projects
that vendor the runtime crates needed to execute the supported COBOL subset.

## Prerequisites

- Rust stable, 1.74 or newer.
- A platform linker and C/C++ build tools for crates that compile native test
  dependencies.
- Git, when working from the repository.
- Optional: GnuCOBOL (`cobc`) for oracle validation.
- Optional: `cargo-audit`, `cargo-fuzz`, and Kani for deeper assurance work.

On Windows, use a Rust toolchain that matches the installed Visual Studio build
tools. On Linux CI, install the normal build-essential package set before
running verification.

## Local Build

From the repository root:

```text
cargo build --features cli --bin cobol-packed
cargo build --features converter --bin cobol2rust
```

Install the CLIs from a checked-out repository:

```text
cargo install --path . --features cli --bin cobol-packed --locked
cargo install --path . --features converter --bin cobol2rust --locked
```

The crates.io installation path for `cobol-packed` remains:

```text
cargo install cobol_packed --features cli
```

Use repository installation for `cobol2rust` until the converter is published
as a stable external tool.

## Verification Scripts

Run the smoke profile before opening a PR or packaging a release candidate:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify-local.ps1 -Profile smoke
```

```bash
bash scripts/verify-local.sh --profile smoke
```

The smoke profile checks formatting, clippy, the all-feature test suite, and
an offline compile of a generated Rust project. The all-feature suite includes
the CLI and converter integration tests. The generated-project check verifies
that generated code builds against the vendored internal runtime crates. It
still requires normal crates.io dependencies such as `rust_decimal`,
`serde_json`, and `thiserror` to be present in the local Cargo cache when run
with `--offline`.

Run the fuller profile when optional tools are installed:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify-local.ps1 -Profile full
```

```bash
bash scripts/verify-local.sh --profile full
```

The full profile requires `cobc` and `cargo-fuzz`. `cargo-audit` is used when
available and otherwise reported as a warning.

## Manual Test Commands

```text
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --features cli --test cli_smoke
cargo test --features converter --test converter_smoke
cargo test --features converter --test oracle_gnucobol -- --nocapture
```

The oracle suite skips external fixtures when `cobc` is missing in local runs.
CI installs GnuCOBOL and treats a missing `cobc` as a failure so the oracle job
cannot pass without executing the oracle fixtures. To run it locally, install
GnuCOBOL and confirm:

```text
cobc --version
```

## Generated Project Smoke Test

```text
cargo run --features converter --bin cobol2rust -- convert \
  --input program.cbl \
  --out generated-rust \
  --dialect ibm \
  --source-format free

cargo check --manifest-path generated-rust/Cargo.toml --offline
```

Generated programs that use assigned OS files resolve those files relative to
the generated program's working directory unless a generated file map overrides
the logical names. Keep production file maps outside source control when they
contain customer paths.

## Optional Assurance Tools

Install optional tools only when you need the corresponding checks:

```text
cargo install cargo-audit --locked
cargo install cargo-fuzz --locked
```

Fuzz smoke builds:

```text
cargo fuzz build fuzz_decode
cargo fuzz build fuzz_record_codecs
cargo fuzz build fuzz_record_layout
```

Kani proofs require the Kani toolchain and are normally run through the deep
verification workflow rather than as part of the local smoke profile.

## Troubleshooting

- `cobc is not on PATH`: install GnuCOBOL or run the smoke profile without the
  oracle checks.
- Generated project cannot build offline: check that the generated `vendor`
  directory contains `cobol-runtime`, `cobol-record`, `cobol-dialect`, and
  `cobol-vm`, and that third-party dependencies have already been downloaded
  into the local Cargo cache.
- Windows linker errors: install the Visual Studio C++ build tools matching
  the active Rust toolchain.
- Cargo network errors: rerun with dependencies already cached, or avoid the
  optional tool installation steps in restricted environments.
- File I/O oracle divergence: confirm the fixture uses fixed-length
  `ORGANIZATION IS SEQUENTIAL`; line sequential, variable-length records, and
  platform file-disposition behavior are intentionally outside this slice.
