#!/usr/bin/env powershell
# Generate a synthetic N-file source tree for index benchmarks (perf-plan).
# -HotName M: also emit M files that all call a shared `run` symbol (high fan-in).
param(
    [string]$Out = "",
    [int]$N = 1000,
    [int]$HotName = 0
)
$ErrorActionPreference = "Stop"
if (-not $Out) {
    $Out = Join-Path $env:TEMP "agentgraph-fixture-$N"
    if ($HotName -gt 0) { $Out = "$Out-hot$HotName" }
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
if ($HotName -gt 0) {
    $hotDir = Join-Path $Out "hot"
    New-Item -ItemType Directory -Force -Path $hotDir | Out-Null
    Set-Content -Path (Join-Path $hotDir "run.ts") -Value "export function run(x: number) { return x; }`n" -Encoding utf8
    for ($i = 0; $i -lt $HotName; $i++) {
        $path = Join-Path $hotDir ("caller{0:D4}.ts" -f $i)
        $body = @"
import { run } from './run';
export function c$i() {
  return run($i);
}
"@
        Set-Content -Path $path -Value $body -Encoding utf8
    }
}
Write-Host "generated $N files (hot=$HotName) at $Out"
Write-Output $Out
