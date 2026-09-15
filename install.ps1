# Install agentgraph from GitHub Releases (Windows PowerShell).
# Usage: iwr -useb https://raw.githubusercontent.com/jiangwuAwA/agentgraph/master/install.ps1 | iex
param(
    [string]$Version = "latest",
    [string]$BinDir = "$env:USERPROFILE\.local\bin"
)

$ErrorActionPreference = "Stop"
$repo = "jiangwuAwA/agentgraph"
$asset = "agentgraph-windows-x86_64.exe"
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

if ($Version -eq "latest") {
    $url = "https://github.com/$repo/releases/latest/download/$asset"
} else {
    $url = "https://github.com/$repo/releases/download/$Version/$asset"
}

$dest = Join-Path $BinDir "agentgraph.exe"
Write-Host "Downloading $url"
Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing

# Ensure user PATH contains BinDir
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$BinDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$BinDir", "User")
    Write-Host "Added $BinDir to user PATH (restart shell if needed)"
}

Write-Host "Installed $dest"
& $dest --version
