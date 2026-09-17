#!/usr/bin/env pwsh
# Expand as many stock-trading-app crates as possible into shadow tree.
# Usage: powershell -File scripts/stock_expand_all.ps1
param(
    [string]$Corpus = "D:\projects\eval-corpus\stock-trading-app",
    [string]$Shadow = "D:\projects\eval-corpus\stock-trading-app-expanded\real-expanded",
    [int]$TimeoutSec = 240
)
$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force -Path $Shadow | Out-Null
$log = Join-Path $Shadow "expand-all.log"
"$(Get-Date -Format o) start crates from $Corpus" | Set-Content $log

Push-Location $Corpus
$crates = Get-ChildItem crates -Directory | Select-Object -ExpandProperty Name
$ok = @()
$fail = @()
foreach ($c in $crates) {
    $outDir = Join-Path $Shadow $c
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $out = Join-Path $outDir "lib_expanded.rs"
    $err = Join-Path $Shadow "$c.expand.err"
    Write-Host "=== $c ==="
    $p = Start-Process -FilePath "cargo" -ArgumentList @("expand","-p",$c,"--lib") `
        -RedirectStandardOutput $out -RedirectStandardError $err `
        -NoNewWindow -PassThru
    $done = $p.WaitForExit($TimeoutSec * 1000)
    if (-not $done) { try { $p.Kill() } catch {}; $fail += $c; "TIMEOUT $c" | Add-Content $log; continue }
    $len = 0
    if (Test-Path $out) { $len = (Get-Item $out).Length }
    if ($p.ExitCode -eq 0 -and $len -gt 500) {
        $ok += $c
        "OK $c bytes=$len" | Add-Content $log
        Write-Host "OK $c bytes=$len"
    } else {
        $fail += $c
        "FAIL $c exit=$($p.ExitCode) bytes=$len" | Add-Content $log
        Write-Host "FAIL $c"
    }
}
Pop-Location
"OK=$($ok.Count) FAIL=$($fail.Count)" | Add-Content $log
Write-Host "DONE OK=$($ok.Count) FAIL=$($fail.Count)"
Write-Host "OK list:"; $ok -join ", "
Write-Host "FAIL list:"; $fail -join ", "
