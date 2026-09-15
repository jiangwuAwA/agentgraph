#!/usr/bin/env powershell
# L0.2 performance budget check for agentgraph.
# Usage: powershell -File scripts/bench_index.ps1
param([string]$Root = "")
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $repo
$bin = Join-Path $repo "target\release\agentgraph.exe"
if (-not (Test-Path $bin)) { cargo build --release | Out-Host }
if (-not $Root) { $Root = Join-Path $repo "fixtures\sample-app" }

function Invoke-Index([string]$label, [string[]]$indexArgs) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    # agentgraph prints progress on stderr; don't treat as error
    $null = & $bin --root $Root index @indexArgs 2>&1
    $sw.Stop()
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
    & $bin --root $Root index 2>$null | Out-Null
}
Write-Host "budgets: full soft (large-repo < 240s); noop/inc target < 200ms on SSD for small files"
if ($inc -gt 5000) { Write-Error "noop incremental too slow: $inc ms" }
Write-Host "BENCH OK"
