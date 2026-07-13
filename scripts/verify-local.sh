#!/usr/bin/env bash
set -euo pipefail

profile="smoke"
with_oracle=0
with_fuzz=0
with_audit=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --profile)
      profile="${2:?missing value for --profile}"
      shift 2
      ;;
    --with-oracle)
      with_oracle=1
      shift
      ;;
    --with-fuzz)
      with_fuzz=1
      shift
      ;;
    --with-audit)
      with_audit=1
      shift
      ;;
    -h|--help)
      echo "usage: scripts/verify-local.sh [--profile smoke|full] [--with-oracle] [--with-fuzz] [--with-audit]"
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      exit 2
      ;;
  esac
done

if [ "$profile" != "smoke" ] && [ "$profile" != "full" ]; then
  echo "invalid --profile: $profile" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

step() {
  echo "==> $1"
  shift
  "$@"
}

step "toolchain" rustc --version
step "cargo" cargo --version
step "format" cargo fmt --all --check

workspace_args=(--workspace --all-targets --all-features)
test_args=(--workspace --all-features)
if ! command -v python >/dev/null 2>&1 && ! command -v python3 >/dev/null 2>&1; then
  if [ "$profile" = "full" ]; then
    echo "full verification requires Python 3.x for the cobol-python workspace crate" >&2
    exit 1
  fi
  echo "warning: Python 3.x not found; smoke profile excludes cobol-python" >&2
  workspace_args=(--workspace --all-targets --all-features --exclude cobol-python)
  test_args=(--workspace --all-features --exclude cobol-python)
fi

step "clippy" cargo clippy "${workspace_args[@]}" -- -D warnings
step "workspace tests" cargo test "${test_args[@]}"

echo "==> generated converter project compiles offline"
root="$repo_root/target/local-verify"
input="$root/hello.cbl"
out="$root/generated"
mkdir -p "$root"
cat > "$input" <<'EOF'
IDENTIFICATION DIVISION.
PROGRAM-ID. HELLO.
PROCEDURE DIVISION.
MAIN.
    DISPLAY "HELLO".
    STOP RUN.
EOF
cargo run --features converter --bin cobol2rust -- convert --input "$input" --out "$out" --dialect ibm --source-format free
cargo check --manifest-path "$out/Cargo.toml" --offline

if [ "$with_oracle" -eq 1 ] || [ "$profile" = "full" ]; then
  if ! command -v cobc >/dev/null 2>&1; then
    echo "GnuCOBOL oracle requested, but cobc is not on PATH" >&2
    exit 1
  fi
  step "GnuCOBOL oracle tests" cargo test --features converter --test oracle_gnucobol -- --ignored --nocapture
fi

if [ "$with_audit" -eq 1 ] || [ "$profile" = "full" ]; then
  if command -v cargo-audit >/dev/null 2>&1; then
    step "cargo audit" cargo audit
  else
    echo "warning: cargo-audit is not installed; install with: cargo install cargo-audit --locked" >&2
  fi
  if command -v cargo-deny >/dev/null 2>&1; then
    step "cargo deny" cargo deny check
  else
    echo "warning: cargo-deny is not installed; install with: cargo install cargo-deny --locked" >&2
  fi
  step "Cargo metadata SBOM" bash scripts/generate-sbom.sh
fi

if [ "$with_fuzz" -eq 1 ] || [ "$profile" = "full" ]; then
  if ! command -v cargo-fuzz >/dev/null 2>&1; then
    echo "cargo-fuzz requested, but cargo-fuzz is not installed" >&2
    exit 1
  fi
  step "fuzz decode build" cargo fuzz build fuzz_decode
  step "fuzz record-codec build" cargo fuzz build fuzz_record_codecs
  step "fuzz record-layout build" cargo fuzz build fuzz_record_layout
  step "fuzz source-parser build" cargo fuzz build fuzz_source_parser
fi

echo "local verification passed ($profile)"
