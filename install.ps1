# Install agentgraph from GitHub Releases (Windows PowerShell).
# Usage: iwr -useb https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.ps1 | iex
# Or:    .\install.ps1 -Version v0.1.0 [-SkipChecksum] [-BinDir path]
param(
    [string]$Version = "latest",
    [string]$BinDir = "$env:USERPROFILE\.local\bin",
    # Explicit opt-out only. Checksum failure always throws (fail closed).
    [switch]$SkipChecksum
)

$ErrorActionPreference = "Stop"
$repo = "jiangwuAwA/agentgraph"
# Release publishes a zip (binary inside is agentgraph-windows-x86_64.exe)
$asset = "agentgraph-windows-x86_64.zip"
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

if ($Version -eq "latest") {
    $url = "https://github.com/$repo/releases/latest/download/$asset"
} else {
    $url = "https://github.com/$repo/releases/download/$Version/$asset"
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("agentgraph-" + [Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$zip = Join-Path $tmp $asset

Write-Host "Downloading $url"
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing

# Checksum verification is mandatory unless -SkipChecksum is passed.
if (-not $SkipChecksum) {
    $sumUrl = "$url.sha256"
    $sumFile = "$zip.sha256"
    try {
        Invoke-WebRequest -Uri $sumUrl -OutFile $sumFile -UseBasicParsing
    } catch {
        throw "checksum asset missing or download failed ($sumUrl): $($_.Exception.Message). Use -SkipChecksum to install without verification."
    }
    $expected = ((Get-Content $sumFile -Raw).Trim() -split '\s+')[0].ToLower()
    if ([string]::IsNullOrEmpty($expected)) {
        throw "checksum file is empty or malformed ($sumUrl). Use -SkipChecksum to install without verification."
    }
    $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
    if ($expected -ne $actual) {
        throw "SHA256 mismatch for ${asset}: expected $expected got $actual"
    }
    Write-Host "SHA256 verified"
} else {
    Write-Warning "checksum verification skipped (-SkipChecksum)"
}

Expand-Archive -Path $zip -DestinationPath $tmp -Force
$exe = Get-ChildItem -Path $tmp -Filter *.exe | Select-Object -First 1
if (-not $exe) { throw "no exe in release archive" }
$dest = Join-Path $BinDir "agentgraph.exe"
Copy-Item $exe.FullName $dest -Force

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ([string]::IsNullOrEmpty($userPath)) {
    [Environment]::SetEnvironmentVariable("Path", $BinDir, "User")
} elseif (($userPath -split ';') -notcontains $BinDir) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$BinDir", "User")
    Write-Host "Added $BinDir to user PATH (restart shell if needed)"
}

Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "Installed $dest"
& $dest --version
