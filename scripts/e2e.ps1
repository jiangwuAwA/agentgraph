# One-shot local E2E for agentgraph (Windows PowerShell).
# Usage: .\scripts\e2e.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

Write-Host "== cargo test (all, includes e2e_cli) =="
cargo test --all-targets
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "== cargo clippy =="
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "== fixture CLI smoke =="
$bin = Join-Path $root "target\debug\agentgraph.exe"
$fix = Join-Path $root "fixtures\sample-app"
& $bin --root $fix index --force
& $bin --root $fix find createUser --limit 3 | Out-Null
& $bin --root $fix importers src/auth.ts | Out-Null
$out = Join-Path $root "out\e2e.scip"
New-Item -ItemType Directory -Force -Path (Join-Path $root "out") | Out-Null
& $bin --root $fix export scip --out $out
if ($LASTEXITCODE -ne 0) { exit 1 }

$scip = Get-Command scip -ErrorAction SilentlyContinue
if ($scip) {
    Write-Host "== scip lint =="
    & scip lint $out
    if ($LASTEXITCODE -ne 0) { exit 1 }
} else {
    Write-Host "skip scip lint (CLI not on PATH)"
}

Write-Host "E2E OK"
