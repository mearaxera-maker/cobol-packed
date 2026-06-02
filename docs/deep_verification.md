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

Use GitHub Actions `workflow_dispatch` to run it on demand. The workflow is
manual-only until the repository owner decides that scheduled fuzz/Kani cost
and flakiness are acceptable. It runs on Linux because that is the practical
target for AddressSanitizer-backed libFuzzer and Kani proof execution.

Profiles:

- `smoke`: short manual confidence check.
- `rc`: release-candidate gate and default for manual dispatch.
- `nightly`: longer campaign profile for explicit maintainer runs.

Manual RC command after pushing a branch:

```bash
gh workflow run deep-verification.yml --ref <branch> \
  -f profile=rc \
  -f upload_success_evidence=true
```

If `gh` is not available, use the GitHub web UI:

1. Open the repository Actions tab.
2. Select **Deep Verification**.
3. Choose **Run workflow**.
4. Select the branch and `profile=smoke` first.
5. Re-run with `profile=rc` after smoke passes.

Optional inputs:

- `runs_per_target`: overrides libFuzzer `-runs` for every target.
- `max_total_time`: overrides libFuzzer `-max_total_time`.
- `upload_success_evidence`: uploads full success logs in addition to summaries.

The workflow runs:

- scalar `fuzz_decode`
- SIMD `fuzz_decode`
- `fuzz_roundtrip`
- `fuzz_schema_batch`
- scalar and SIMD `fuzz_record_codecs`
- `fuzz_record_layout`
- Kani proof harnesses from `kani/proofs.rs`

Every run uploads a `deep-verification-evidence` artifact containing
`deep-verification-summary.json` and `deep-verification-summary.md`. Failed fuzz
jobs also upload `fuzz-artifacts-<target>-<variant>`, and failed Kani jobs upload
`kani-logs-<harness>`.

Each evidence part records the exact shell-escaped command and toolchain
metadata. The workflow avoids shell `eval`; fuzz and Kani commands are executed
as argument arrays so feature flags and target names cannot alter the command
shape.

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
cargo +nightly fuzz run fuzz_record_codecs -- -runs=20000 -max_total_time=300
cargo +nightly fuzz run --features simd fuzz_record_codecs -- -runs=10000 -max_total_time=300
cargo +nightly fuzz run fuzz_record_layout -- -runs=20000 -max_total_time=300
cargo install --locked kani-verifier
cargo kani setup
cargo kani --features kani --test kani_proofs --harness proof_no_panic
cargo kani --features kani --test kani_proofs --harness proof_no_panic_full_width_decode
cargo kani --features kani --test kani_proofs --harness proof_lossless_roundtrip
cargo kani --features kani --test kani_proofs --harness proof_scalar_parity
cargo kani --features kani,simd --test kani_proofs --harness proof_scalar_simd_agree
cargo kani --features kani --test kani_proofs --harness proof_record_binary_no_panic
cargo kani --features kani --test kani_proofs --harness proof_record_zoned_no_panic
cargo kani --features kani --test kani_proofs --harness proof_record_ibm_float_no_panic
```

Windows note
------------

On Windows/MSVC, cargo-fuzz can build a target but may fail to start when the
MSVC AddressSanitizer runtime DLL is not installed. Kani also does not provide
the same direct Windows setup path as Linux/macOS. For this repository, treat the
Linux GitHub Actions workflow as the authoritative deep verification runner.
