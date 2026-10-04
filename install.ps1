# ==============================================================================
# PLU: One-Command Installer for Windows (PowerShell)
# Usage:
#   irm https://raw.githubusercontent.com/khokharsnehil45/plu/master/install.ps1 | iex
# ==============================================================================

$ErrorActionPreference = "Stop"

$Repo = "khokharsnehil45/plu"
$BinaryName = "plu.exe"
$InstallDir = "$HOME\.cargo\bin"

Write-Host "========================================================" -ForegroundColor Cyan
Write-Host "  PLU: High-Performance PDF Loader & Unloader Installer " -ForegroundColor Cyan
Write-Host "========================================================" -ForegroundColor Cyan

# Check if install directory exists
if (-not (Test-Path -Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$CargoCmd = Get-Command cargo -ErrorAction SilentlyContinue

if ($CargoCmd) {
    Write-Host "[+] Cargo detected. Building and installing PLU from GitHub..." -ForegroundColor Green
    & cargo install --git "https://github.com/$Repo.git" --force
    if ($LASTEXITCODE -eq 0) {
        Write-Host "========================================================" -ForegroundColor Green
        Write-Host "  [+] PLU successfully installed via Cargo!" -ForegroundColor Green
        Write-Host "========================================================" -ForegroundColor Green
    } else {
        Write-Host "[-] Cargo install encountered an issue." -ForegroundColor Red
    }
} else {
    Write-Host "[!] Cargo not found in current session." -ForegroundColor Yellow
    
    # Try fetching latest release from GitHub Releases API
    try {
        $ReleaseUrl = "https://api.github.com/repos/$Repo/releases/latest"
        $Release = Invoke-RestMethod -Uri $ReleaseUrl -Headers @{ "User-Agent" = "PowerShell" }
        $Asset = $Release.assets | Where-Object { $_.name -like "*windows-x86_64*.zip" } | Select-Object -First 1

        if ($Asset) {
            Write-Host "[+] Downloading prebuilt binary: $($Asset.name)..." -ForegroundColor Cyan
            $ZipPath = "$env:TEMP\plu.zip"
            Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $ZipPath
            Expand-Archive -Path $ZipPath -DestinationPath $InstallDir -Force
            Remove-Item -Path $ZipPath -Force
            Write-Host "[+] Extracted binary to $InstallDir\$BinaryName" -ForegroundColor Green
        } else {
            throw "No prebuilt Windows release assets found."
        }
    } catch {
        Write-Host "[+] Installing Rust toolchain using winget..." -ForegroundColor Yellow
        if (Get-Command winget -ErrorAction SilentlyContinue) {
            winget install --id Rustlang.Rustup -e --silent --accept-package-agreements --accept-source-agreements
            $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")
            cargo install --git "https://github.com/$Repo.git" --force
        } else {
            Write-Host "[-] Please install Rust from https://rustup.rs/ to compile PLU." -ForegroundColor Red
            Exit 1
        }
    }
}

# Ensure ~/.cargo/bin is in User PATH
$UserPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [System.Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    $env:Path += ";$InstallDir"
    Write-Host "[+] Added $InstallDir to user PATH." -ForegroundColor Green
}

Write-Host "`nVerification:" -ForegroundColor Cyan
if (Get-Command plu -ErrorAction SilentlyContinue) {
    & plu --version
} else {
    & "$InstallDir\plu.exe" --version
}

Write-Host "`nUsage:" -ForegroundColor Green
Write-Host "  plu --load input.pdf --unload output.md"
Write-Host "  plu --load input.pdf -f md"
Write-Host "  plu --load .\documents\ -f md"
Write-Host "  plu --ui"
