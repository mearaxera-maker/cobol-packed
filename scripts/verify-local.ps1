param(
    [ValidateSet("smoke", "full")]
    [string]$Profile = "smoke",
    [switch]$WithOracle,
    [switch]$WithFuzz,
    [switch]$WithAudit
)

$ErrorActionPreference = "Stop"

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [scriptblock]$Command
    )

    Write-Host "==> $Name"
    & $Command
}

function Test-Command {
    param([Parameter(Mandatory = $true)][string]$Name)
    $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

Invoke-Step "toolchain" {
    rustc --version
    cargo --version
}

Invoke-Step "format" {
    cargo fmt --all --check
}

Invoke-Step "clippy" {
    cargo clippy --workspace --all-targets --all-features -- -D warnings
}

Invoke-Step "workspace tests" {
    cargo test --workspace --all-features
}

Invoke-Step "generated converter project compiles offline" {
    $Root = Join-Path $RepoRoot "target/local-verify"
    $Input = Join-Path $Root "hello.cbl"
    $Out = Join-Path $Root "generated"
    New-Item -ItemType Directory -Force -Path $Root | Out-Null
    @"
IDENTIFICATION DIVISION.
PROGRAM-ID. HELLO.
PROCEDURE DIVISION.
MAIN.
    DISPLAY "HELLO".
    STOP RUN.
"@ | Set-Content -NoNewline -Encoding ASCII -Path $Input
    cargo run --features converter --bin cobol2rust -- convert --input $Input --out $Out --dialect ibm --source-format free
    cargo check --manifest-path (Join-Path $Out "Cargo.toml") --offline
}

if ($WithOracle -or $Profile -eq "full") {
    if (-not (Test-Command "cobc")) {
        throw "GnuCOBOL oracle requested, but cobc is not on PATH"
    }
    Invoke-Step "GnuCOBOL oracle tests" {
        cargo test --features converter --test oracle_gnucobol -- --nocapture
    }
}

if ($WithAudit -or $Profile -eq "full") {
    if (Test-Command "cargo-audit") {
        Invoke-Step "cargo audit" {
            cargo audit
        }
    } else {
        Write-Warning "cargo-audit is not installed; install with: cargo install cargo-audit --locked"
    }
    if (Test-Command "cargo-deny") {
        Invoke-Step "cargo deny" {
            cargo deny check
        }
    } else {
        Write-Warning "cargo-deny is not installed; install with: cargo install cargo-deny --locked"
    }
    Invoke-Step "Cargo metadata SBOM" {
        & "$PSScriptRoot/generate-sbom.ps1"
    }
}

if ($WithFuzz -or $Profile -eq "full") {
    if (-not (Test-Command "cargo-fuzz")) {
        throw "cargo-fuzz requested, but cargo-fuzz is not installed"
    }
    Invoke-Step "fuzz harness build" {
        cargo fuzz build fuzz_decode
        cargo fuzz build fuzz_record_codecs
        cargo fuzz build fuzz_record_layout
        cargo fuzz build fuzz_source_parser
    }
}

Write-Host "local verification passed ($Profile)"
