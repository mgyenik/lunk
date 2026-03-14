# Lunk installer for Windows
# Usage: irm https://raw.githubusercontent.com/mgyenik/lunk/main/scripts/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo = "mgyenik/lunk"
$InstallDir = if ($env:LUNK_INSTALL_DIR) { $env:LUNK_INSTALL_DIR } else { "$env:LOCALAPPDATA\lunk\bin" }

function Write-Info { Write-Host "==> $args" -ForegroundColor Cyan }
function Write-Ok   { Write-Host "==> $args" -ForegroundColor Green }

# Resolve latest version
Write-Info "Fetching latest release..."
try {
    $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $release.tag_name
} catch {
    Write-Host "Could not fetch latest version. Specify manually:" -ForegroundColor Red
    Write-Host "  `$Version = 'v0.1.0'; irm .../install.ps1 | iex" -ForegroundColor Yellow
    exit 1
}

Write-Info "Installing lunk $Version for Windows x86_64"

$BinaryName = "lunk-windows-x86_64.exe"
$DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$BinaryName"

# Create install directory
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$OutFile = Join-Path $InstallDir "lunk.exe"

# Download
Write-Info "Downloading from $DownloadUrl..."
try {
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $OutFile -UseBasicParsing
} catch {
    Write-Host "Download failed. Check that version $Version exists." -ForegroundColor Red
    exit 1
}

Write-Ok "Installed lunk to $OutFile"

# Add to user PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$InstallDir;$UserPath", "User")
    Write-Ok "Added $InstallDir to user PATH"
    Write-Host ""
    Write-Host "  Restart your terminal for PATH changes to take effect." -ForegroundColor Yellow
}

Write-Host ""
Write-Ok "Run 'lunk --help' to get started"
