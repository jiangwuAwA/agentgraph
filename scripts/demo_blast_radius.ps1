# P0-2 onboarding demo: index sample fixture -> blast-radius JSON.
# Offline from repo fixtures. Exit 0 on success.
# Usage: powershell -File scripts/demo_blast_radius.ps1
#        pwsh -File scripts/demo_blast_radius.ps1
param(
    [string]$Bin = "",
    [string]$Symbol = "createUser",
    [switch]$KeepTemp
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

function Resolve-AgentgraphBin {
    param([string]$Explicit)
    if ($Explicit -and (Test-Path $Explicit)) { return (Resolve-Path $Explicit).Path }
    if ($env:AGENTGRAPH_BIN -and (Test-Path $env:AGENTGRAPH_BIN)) {
        return (Resolve-Path $env:AGENTGRAPH_BIN).Path
    }
    $cands = @(
        (Join-Path $root "target\debug\agentgraph.exe"),
        (Join-Path $root "target\debug\agentgraph"),
        (Join-Path $root "target\release\agentgraph.exe"),
        (Join-Path $root "target\release\agentgraph")
    )
    foreach ($c in $cands) {
        if (-not (Test-Path $c)) { continue }
        try {
            $help = & $c blast-radius --help 2>&1 | Out-String
            if ($help -match "blast-radius") { return $c }
        } catch { }
    }
    $cmd = Get-Command agentgraph -ErrorAction SilentlyContinue
    if ($cmd) {
        $help = & $cmd.Source blast-radius --help 2>&1 | Out-String
        if ($help -match "blast-radius") { return $cmd.Source }
    }
    Write-Host "No agentgraph binary with blast-radius; building debug..."
    Push-Location $root
    try {
        cargo build --bin agentgraph
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
    } finally { Pop-Location }
    $built = Join-Path $root "target\debug\agentgraph.exe"
    if (-not (Test-Path $built)) { $built = Join-Path $root "target\debug\agentgraph" }
    if (-not (Test-Path $built)) { throw "built binary not found at $built" }
    return $built
}

$ag = Resolve-AgentgraphBin -Explicit $Bin
Write-Host "Using binary: $ag"
& $ag --version

$fixture = Join-Path $root "fixtures\sample-app"
if (-not (Test-Path (Join-Path $fixture "src\auth.ts"))) {
    throw "missing fixture $fixture (need fixtures/sample-app)"
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("agentgraph-demo-br-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
Write-Host "Temp sample: $tmp"

try {
    Copy-Item -Path (Join-Path $fixture "src") -Destination (Join-Path $tmp "src") -Recurse -Force

    Write-Host "== index =="
    & $ag --root $tmp index --force
    if ($LASTEXITCODE -ne 0) { throw "index failed (exit $LASTEXITCODE)" }

    Write-Host "== blast-radius $Symbol =="
    $jsonRaw = & $ag --root $tmp blast-radius $Symbol --depth 3
    if ($LASTEXITCODE -ne 0) { throw "blast-radius failed (exit $LASTEXITCODE)" }
    $jsonText = ($jsonRaw | Out-String).Trim()
    if ([string]::IsNullOrWhiteSpace($jsonText)) { throw "blast-radius produced empty stdout" }
    $payload = $jsonText | ConvertFrom-Json

    Write-Host ""
    Write-Host "== recommendation / window =="
    Write-Host ("window:          " + $payload.window)
    Write-Host ("subset_ok:       " + $payload.subset_ok)
    Write-Host ("promise_tier:    " + $payload.promise_tier)
    Write-Host ("recommendation:  " + $payload.recommendation)
    Write-Host ("note:            " + $payload.note)
    Write-Host ""
    Write-Host "== full blast_radius JSON =="
    Write-Host $jsonText
    Write-Host ""

    if (-not $payload.window) { throw "payload missing window" }
    if (-not $payload.recommendation) { throw "payload missing recommendation" }
    if (-not $payload.note) { throw "payload missing note" }

    Write-Host "== who-calls validateEmail (smoke) =="
    & $ag --root $tmp who-calls validateEmail | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "who-calls failed (exit $LASTEXITCODE)" }

    Write-Host "DEMO OK"
    exit 0
} finally {
    if (-not $KeepTemp) {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    } else {
        Write-Host "Kept temp: $tmp"
    }
}
