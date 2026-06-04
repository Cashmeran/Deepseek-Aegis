param(
    [string]$InstallDir = "$env:LOCALAPPDATA\aegis"
)

$ErrorActionPreference = "Stop"
$Repo = "Cashmeran/deepseek-aegis"

Write-Host "Installing aegis..." -ForegroundColor Cyan

# Get latest version
$Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
$Version = $Release.tag_name
Write-Host "Version: $Version"

$Url = "https://github.com/$Repo/releases/download/$Version/aegis-windows-x86_64.zip"

# Download
$TempDir = Join-Path $env:TEMP "aegis-install"
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
$ZipFile = Join-Path $TempDir "aegis.zip"
Invoke-WebRequest -Uri $Url -OutFile $ZipFile

# Extract
Expand-Archive -Path $ZipFile -DestinationPath $TempDir -Force

# Install
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item "$TempDir\aegis.exe" "$InstallDir\aegis.exe" -Force
Copy-Item "$TempDir\aegis-diag.exe" "$InstallDir\aegis-diag.exe" -Force -ErrorAction SilentlyContinue

# Cleanup
Remove-Item -Recurse -Force $TempDir

# PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    $env:Path += ";$InstallDir"
    Write-Host "Added to PATH. Restart your terminal for changes to take effect." -ForegroundColor Yellow
}

Write-Host "Installed to $InstallDir" -ForegroundColor Green
Write-Host "Run: aegis --version"
