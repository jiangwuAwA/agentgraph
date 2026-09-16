#!/usr/bin/env powershell
# In-process query p95 hard acceptance (PLAN §6).
# Usage: powershell -File scripts/bench_query_p95.ps1 [-N 5000] [-Samples 200]
param(
    [int]$N = 5000,
    [int]$Samples = 200,
    [string]$Root = ""
)
$ErrorActionPreference = "Continue"
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $repo
$bin = Join-Path $repo "target\release\agentgraph.exe"
if (-not (Test-Path $bin)) { cargo build --release | Out-Host }
if (-not $Root) {
    $Root = & (Join-Path $repo "scripts\gen_fixture.ps1") -N $N | Select-Object -Last 1
}
cmd /c "`"$bin`" --root `"$Root`" index --force >NUL 2>&1"
$out = cmd /c "`"$bin`" --root `"$Root`" bench-query --samples $Samples --prefix helper 2>&1"
Write-Host $out
if ($LASTEXITCODE -ne 0) {
    Write-Error "p95 SLO fail"
    exit 1
}
Write-Host "P95 OK"
