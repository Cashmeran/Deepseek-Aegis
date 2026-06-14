param(
    [string]$InstallDir = "$env:LOCALAPPDATA\aegis"
)

$ErrorActionPreference = "Stop"
$Repo = "Cashmeran/Deepseek-Aegis"

Write-Host "Installing aegis..." -ForegroundColor Cyan

try {
    # Get latest version
    Write-Host "Fetching latest version..."
    $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -MaximumRetryCount 3 -RetryIntervalSec 2
    $Version = $Release.tag_name
    if (-not $Version) {
        throw "Could not determine latest version"
    }
    Write-Host "Version: $Version" -ForegroundColor Green

    $Url = "https://github.com/$Repo/releases/download/$Version/aegis-windows-x86_64.zip"

    # Download
    $TempDir = Join-Path $env:TEMP "aegis-install"
    New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
    $ZipFile = Join-Path $TempDir "aegis.zip"

    Write-Host "Downloading..."
    Invoke-WebRequest -Uri $Url -OutFile $ZipFile -MaximumRetryCount 3 -RetryIntervalSec 2

    # Verify download
    if (-not (Test-Path $ZipFile)) {
        throw "Download failed: file not found at $ZipFile"
    }
    $zipSize = (Get-Item $ZipFile).Length
    if ($zipSize -lt 1024) {
        throw "Download appears corrupted (size: $zipSize bytes)"
    }
    Write-Host "Downloaded ($([math]::Round($zipSize/1MB, 1)) MB)" -ForegroundColor Green

    # Extract
    Write-Host "Extracting..."
    Expand-Archive -Path $ZipFile -DestinationPath $TempDir -Force

    # Verify binary exists in extracted files
    $exePath = Get-ChildItem -Path $TempDir -Recurse -Name "aegis.exe" | Select-Object -First 1
    if (-not $exePath) {
        throw "Archive does not contain aegis.exe"
    }
    $extractDir = Split-Path (Join-Path $TempDir $exePath) -Parent
    if (-not $extractDir) { $extractDir = $TempDir }

    # Install
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item (Join-Path $extractDir "aegis.exe") "$InstallDir\aegis.exe" -Force
    Copy-Item (Join-Path $extractDir "aegis-diag.exe") "$InstallDir\aegis-diag.exe" -Force -ErrorAction SilentlyContinue

    # Cleanup
    Remove-Item -Recurse -Force $TempDir

    # Smoke test
    try {
        & "$InstallDir\aegis.exe" --version 2>&1 | Out-Null
        Write-Host "Binary verified OK." -ForegroundColor Green
    } catch {
        Write-Host "Warning: binary may not be executable. Check your antivirus or try running directly." -ForegroundColor Yellow
    }

    # PATH
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
        $env:Path += ";$InstallDir"
        Write-Host "Added to PATH. Restart your terminal for changes to take effect." -ForegroundColor Yellow
    }

    Write-Host "Installed to $InstallDir" -ForegroundColor Green
    Write-Host "Run: aegis --help" -ForegroundColor Cyan

} catch {
    Write-Host "Error: $_" -ForegroundColor Red
    Write-Host ""
    Write-Host "You can download manually from: https://github.com/$Repo/releases" -ForegroundColor Yellow
    exit 1
}
