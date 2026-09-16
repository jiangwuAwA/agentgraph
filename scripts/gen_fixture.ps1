#!/usr/bin/env powershell
# Generate a synthetic N-file source tree for index benchmarks (perf-plan).
param(
    [string]$Out = "",
    [int]$N = 1000
)
$ErrorActionPreference = "Stop"
if (-not $Out) {
    $Out = Join-Path $env:TEMP "agentgraph-fixture-$N"
}
if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
for ($i = 0; $i -lt $N; $i++) {
    $dir = Join-Path $Out ("pkg{0:D3}" -f [int]($i / 50))
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $path = Join-Path $dir ("f{0:D4}.ts" -f $i)
    $body = @"
export function helper$i(x: number) {
  return x + $i;
}
export function main$i() {
  return helper$i($i);
}
"@
    Set-Content -Path $path -Value $body -Encoding utf8
}
Write-Host "generated $N files at $Out"
Write-Output $Out
