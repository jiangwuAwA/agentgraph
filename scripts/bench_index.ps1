#!/usr/bin/env powershell
# L0.2 / perf-plan bench: full / noop / 1-file on any tree.
# Usage:
#   powershell -File scripts/bench_index.ps1
#   powershell -File scripts/bench_index.ps1 -Root C:\path\to\stock-trading-app
#   powershell -File scripts/bench_index.ps1 -Generate 1000
param(
    [string]$Root = "",
    [int]$Generate = 0,
    [switch]$Trace
)
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $repo
$bin = Join-Path $repo "target\release\agentgraph.exe"
if (-not (Test-Path $bin)) { cargo build --release | Out-Host }
if ($Generate -gt 0) {
    $Root = & (Join-Path $repo "scripts\gen_fixture.ps1") -N $Generate | Select-Object -Last 1
}
if (-not $Root) { $Root = Join-Path $repo "fixtures\sample-app" }

function Invoke-Index([string]$label, [string[]]$indexArgs) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $prevTrace = $env:AGENTGRAPH_TRACE
    if ($Trace) { $env:AGENTGRAPH_TRACE = "1" }
    cmd /c "`"$bin`" --root `"$Root`" index $($indexArgs -join ' ') >NUL 2>&1"
    $sw.Stop()
    if ($Trace) { $env:AGENTGRAPH_TRACE = $prevTrace }
    Write-Host ("{0}: {1} ms" -f $label, [int]$sw.Elapsed.TotalMilliseconds)
    return [int]$sw.Elapsed.TotalMilliseconds
}

Write-Host "=== agentgraph bench root=$Root ==="
& $bin --root $Root stats 2>$null | Out-Null
$force = Invoke-Index "full --force" @("--force")
$inc = Invoke-Index "incremental noop" @()
$touch = Get-ChildItem -Path $Root -Recurse -Include *.ts,*.rs,*.py,*.go -File |
    Where-Object { $_.FullName -notmatch '\\.agentgraph\\|\\target\\' } |
    Select-Object -First 1
$one = -1
if ($touch) {
    Add-Content -Path $touch.FullName -Value "`n// bench-touch"
    $one = Invoke-Index "1 file change" @()
    $c = Get-Content $touch.FullName
    Set-Content -Path $touch.FullName -Value ($c | Where-Object { $_ -ne "// bench-touch" })
    cmd /c "`"$bin`" --root `"$Root`" index >NUL 2>&1"
}
Write-Host "budgets: full soft (large-repo < 240s); 1k-file noop target < 2000ms (perf-plan P0)"
if ($inc -gt 5000) { Write-Error "noop incremental too slow: $inc ms" }
Write-Host "BENCH OK"
