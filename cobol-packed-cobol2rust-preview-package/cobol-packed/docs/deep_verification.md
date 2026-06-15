Deep verification
=================

This project has two verification tiers.

Local smoke checks
------------------

These checks are expected to run on normal developer machines:

```powershell
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --lib --no-default-features
cargo test --features simd
cargo check --test kani_proofs --features kani
cargo check --manifest-path fuzz\Cargo.toml
cargo check --manifest-path fuzz\Cargo.toml --features simd
```

The last two commands compile the fuzz harnesses. They do not run fuzz
campaigns.

Hosted deep checks
------------------

Actual cargo-fuzz campaigns and Kani proofs are run by:

```text
.github/workflows/deep-verification.yml
```

Use GitHub Actions `workflow_dispatch` to run it on demand. The workflow runs on
Linux because that is the practical target for AddressSanitizer-backed
libFuzzer and Kani proof execution.

The workflow runs:

- scalar `fuzz_decode`
- SIMD `fuzz_decode`
- `fuzz_roundtrip`
- `fuzz_schema_batch`
- Kani proof harnesses from `kani/proofs.rs`

Linux local equivalent
----------------------

On a Linux host with Rustup:

```bash
rustup toolchain install nightly
cargo +nightly install cargo-fuzz --locked
cargo +nightly fuzz run fuzz_decode -- -runs=20000 -max_total_time=300
cargo +nightly fuzz run --features simd fuzz_decode -- -runs=10000 -max_total_time=300
cargo +nightly fuzz run fuzz_roundtrip -- -runs=20000 -max_total_time=300
cargo +nightly fuzz run fuzz_schema_batch -- -runs=20000 -max_total_time=300
cargo install --locked kani-verifier
cargo kani setup
cargo kani --features kani --test kani_proofs --harness proof_no_panic
cargo kani --features kani --test kani_proofs --harness proof_no_panic_full_width_decode
cargo kani --features kani --test kani_proofs --harness proof_lossless_roundtrip
cargo kani --features kani --test kani_proofs --harness proof_scalar_parity
cargo kani --features kani,simd --test kani_proofs --harness proof_scalar_simd_agree
```

Windows note
------------

On Windows/MSVC, cargo-fuzz can build a target but may fail to start when the
MSVC AddressSanitizer runtime DLL is not installed. Kani also does not provide
the same direct Windows setup path as Linux/macOS. For this repository, treat the
Linux GitHub Actions workflow as the authoritative deep verification runner.
