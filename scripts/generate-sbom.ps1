param(
    [string]$Out = "target/sbom/SBOM.cargo-metadata.json"
)

$ErrorActionPreference = "Stop"

$Parent = Split-Path -Parent $Out
if ($Parent) {
    New-Item -ItemType Directory -Force -Path $Parent | Out-Null
}

cargo metadata --format-version 1 --locked | Set-Content -Encoding UTF8 -Path $Out
Write-Host "wrote $Out"
