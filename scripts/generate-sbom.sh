#!/usr/bin/env bash
set -euo pipefail

out="${1:-target/sbom/SBOM.cargo-metadata.json}"
mkdir -p "$(dirname "$out")"
cargo metadata --format-version 1 --locked > "$out"
echo "wrote $out"
