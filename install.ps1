# Install agentgraph from GitHub Releases (Windows PowerShell).
# Usage: iwr -useb https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.ps1 | iex
param(
    [string]$Version = "latest",
    [string]$BinDir = "$env:USERPROFILE\.local\bin"
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

# Optional checksum
$sumUrl = "$url.sha256"
try {
    Invoke-WebRequest -Uri $sumUrl -OutFile "$zip.sha256" -UseBasicParsing
    $expected = (Get-Content "$zip.sha256" -Raw).Trim().Split(" ")[0].ToLower()
    $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
    if ($expected -ne $actual) {
        throw "SHA256 mismatch: expected $expected got $actual"
    }
    Write-Host "SHA256 verified"
} catch {
    Write-Warning "checksum not verified: $($_.Exception.Message)"
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
