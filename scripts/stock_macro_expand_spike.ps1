# Track B: stock-trading-app macro-expand spike (operator machine).
# Expands (or synthesically expands) derive/async_trait-heavy crates into a
# SHADOW tree outside agentgraph, then indexes source vs expanded roots.
#
# NEVER commits corpus source or expanded output into agentgraph.
# Shadow default: D:\projects\eval-corpus\stock-trading-app-expanded\
#
# Usage:
#   powershell -File scripts\stock_macro_expand_spike.ps1
#   powershell -File scripts\stock_macro_expand_spike.ps1 -Crates event-engine,model-selection-replay
#   powershell -File scripts\stock_macro_expand_spike.ps1 -SkipIndex
#
# Tooling priority:
#   1) cargo expand -p <crate>
#   2) RUSTC_BOOTSTRAP=1 cargo rustc -p <crate> -- -Zunpretty=expanded  (stable hack)
#   3) nightly: cargo +nightly rustc -p <crate> -- -Zunpretty=expanded
#   4) fallback: scripts/expand_index_diff.py prepare (synthetic; labeled)

param(
    [string]$Corpus = "D:\projects\eval-corpus\stock-trading-app",
    [string]$Shadow = "D:\projects\eval-corpus\stock-trading-app-expanded",
    [string]$Crates = "event-engine,model-selection-replay,repository",
    [string]$Agentgraph = "D:\projects\agentgraph\target\release\agentgraph.exe",
    [switch]$SkipIndex,
    [switch]$SkipRealExpand
)

$ErrorActionPreference = "Continue"
$agentgraphRepo = Split-Path -Parent $PSScriptRoot
$logDir = Join-Path $Shadow "metrics"
New-Item -ItemType Directory -Force -Path $Shadow, $logDir | Out-Null

function Write-Log([string]$msg) {
    $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $msg
    Write-Host $line
    Add-Content -Path (Join-Path $logDir "spike.log") -Value $line
}

Write-Log "corpus=$Corpus"
Write-Log "shadow=$Shadow"
Write-Log "crates=$Crates"

# ── Tool checks ──────────────────────────────────────────────────────────
$tooling = @{}
$cargoExpand = Get-Command cargo-expand -ErrorAction SilentlyContinue
$cargoExpandViaCargo = $false
try {
    $null = & cargo expand --version 2>&1
    if ($LASTEXITCODE -eq 0) { $cargoExpandViaCargo = $true }
} catch { $cargoExpandViaCargo = $false }
$tooling["cargo_expand"] = [bool]($cargoExpand -or $cargoExpandViaCargo)

$nightlyOk = $false
try {
    $v = & rustc +nightly --version 2>&1
    if ($LASTEXITCODE -eq 0 -and $v -notmatch "missing manifest") { $nightlyOk = $true }
} catch { $nightlyOk = $false }
$tooling["nightly_rustc"] = $nightlyOk

$bootstrapOk = $true  # verified on this operator machine; see docs/eval-macro-expand.md
$tooling["rustc_bootstrap_unpretty"] = $bootstrapOk

Write-Log ("tooling cargo_expand={0} nightly={1} RUSTC_BOOTSTRAP_unpretty={2}" -f `
    $tooling.cargo_expand, $tooling.nightly_rustc, $tooling.rustc_bootstrap_unpretty)
$tooling | ConvertTo-Json | Set-Content -Path (Join-Path $logDir "tooling.json") -Encoding UTF8

# ── Optional real expand (per-crate, may fail offline / missing deps) ────
$expandResults = @()
if (-not $SkipRealExpand) {
    foreach ($crate in ($Crates -split "," | ForEach-Object { $_.Trim() } | Where-Object { $_ })) {
        Write-Log "real-expand attempt crate=$crate"
        $crateShadow = Join-Path $Shadow "real-expanded\$crate"
        New-Item -ItemType Directory -Force -Path $crateShadow | Out-Null
        $outFile = Join-Path $crateShadow "lib_expanded.rs"
        $ok = $false
        $method = "none"
        Push-Location $Corpus
        try {
            if ($tooling.cargo_expand) {
                $method = "cargo-expand"
                # Capture as bytes/stdout then write UTF-8 — PowerShell `>` is UTF-16 and agentgraph rejects it.
                $errFile = Join-Path $crateShadow "expand.err"
                $raw = & cargo expand -p $crate --lib 2> $errFile
                if ($LASTEXITCODE -eq 0 -and $raw) {
                    [System.IO.File]::WriteAllText($outFile, (($raw | Out-String)), (New-Object System.Text.UTF8Encoding($false)))
                }
                if ((Test-Path $outFile) -and ((Get-Item $outFile).Length -gt 100)) {
                    $head = [System.IO.File]::ReadAllBytes($outFile)
                    if (-not ($head[0] -eq 0xFF -and $head[1] -eq 0xFE)) { $ok = $true }
                }
            }
            if (-not $ok) {
                $method = "RUSTC_BOOTSTRAP-unpretty"
                $env:RUSTC_BOOTSTRAP = "1"
                $errFile = Join-Path $crateShadow "expand.err"
                $raw = & cargo rustc -p $crate --lib -- -Zunpretty=expanded 2> $errFile
                if ($LASTEXITCODE -eq 0 -and $raw) {
                    [System.IO.File]::WriteAllText($outFile, (($raw | Out-String)), (New-Object System.Text.UTF8Encoding($false)))
                }
                if ((Test-Path $outFile) -and ((Get-Item $outFile).Length -gt 100)) { $ok = $true }
            }
            if (-not $ok -and $tooling.nightly_rustc) {
                $method = "nightly-unpretty"
                $errFile = Join-Path $crateShadow "expand.err"
                $raw = & cargo +nightly rustc -p $crate --lib -- -Zunpretty=expanded 2> $errFile
                if ($LASTEXITCODE -eq 0 -and $raw) {
                    [System.IO.File]::WriteAllText($outFile, (($raw | Out-String)), (New-Object System.Text.UTF8Encoding($false)))
                }
                if ((Test-Path $outFile) -and ((Get-Item $outFile).Length -gt 100)) { $ok = $true }
            }
        } catch {
            Write-Log "expand exception crate=$crate : $($_.Exception.Message)"
        } finally {
            Remove-Item Env:RUSTC_BOOTSTRAP -ErrorAction SilentlyContinue
            Pop-Location
        }
        $size = 0
        if (Test-Path $outFile) { $size = (Get-Item $outFile).Length }
        Write-Log "real-expand crate=$crate method=$method ok=$ok bytes=$size"
        $expandResults += [pscustomobject]@{ crate = $crate; method = $method; ok = $ok; bytes = $size }
    }
}
$expandResults | ConvertTo-Json | Set-Content -Path (Join-Path $logDir "real_expand_results.json") -Encoding UTF8

# ── Synthetic / measurement shadow tree ──────────────────────────────────
Write-Log "prepare synthetic/source shadow via expand_index_diff.py"
$py = Get-Command python -ErrorAction SilentlyContinue
if (-not $py) { $py = Get-Command python3 -ErrorAction SilentlyContinue }
if (-not $py) {
    Write-Log "FATAL: python not found; cannot prepare synthetic shadow"
    exit 3
}
& $py.Source (Join-Path $agentgraphRepo "scripts\expand_index_diff.py") prepare `
    --corpus $Corpus --shadow $Shadow --crates $Crates
Write-Log "prepare exit=$LASTEXITCODE"

if ($SkipIndex) {
    Write-Log "SkipIndex set; done"
    exit 0
}

# ── Index source-view and expanded-view as separate roots ────────────────
$sourceRoot = Join-Path $Shadow "source-view"
$expandRoot = Join-Path $Shadow "expanded-view"
if (-not (Test-Path $Agentgraph)) {
    Write-Log "FATAL: agentgraph binary missing at $Agentgraph (cargo build --release first)"
    exit 4
}

Write-Log "index source-view"
$t0 = Get-Date
& $Agentgraph --root $sourceRoot index --force
$srcIdx = $LASTEXITCODE
$srcSec = ((Get-Date) - $t0).TotalSeconds
Write-Log "index source-view exit=$srcIdx seconds=$srcSec"

Write-Log "index expanded-view"
$t1 = Get-Date
& $Agentgraph --root $expandRoot index --force
$expIdx = $LASTEXITCODE
$expSec = ((Get-Date) - $t1).TotalSeconds
Write-Log "index expanded-view exit=$expIdx seconds=$expSec"

& $Agentgraph --root $sourceRoot stats | Set-Content -Path (Join-Path $logDir "stats_source.json") -Encoding UTF8
& $Agentgraph --root $expandRoot stats | Set-Content -Path (Join-Path $logDir "stats_expanded.json") -Encoding UTF8

Write-Log "diff via expand_index_diff.py"
& $py.Source (Join-Path $agentgraphRepo "scripts\expand_index_diff.py") diff `
    --source-root $sourceRoot --expanded-root $expandRoot | Tee-Object -FilePath (Join-Path $logDir "diff.txt")

Write-Log "spike complete"
Write-Host ""
Write-Host "Shadow tree: $Shadow"
Write-Host "Metrics:     $logDir"
Write-Host "Docs:        docs\eval-macro-expand.md"
Write-Host "NOTE: never commit expanded/source shadow into agentgraph."
