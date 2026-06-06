@echo off
setlocal enabledelayedexpansion
cd /d "%~dp0"

echo.
echo  === Aegis Desktop — Production Build ===
echo.

:: Check Rust
where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo [ERROR] Rust not found. Install from https://rustup.rs
    pause & exit /b 1
)

:: Install frontend deps if needed
if not exist "node_modules" (
    echo [1/3] Installing frontend dependencies...
    call npm install
    if %errorlevel% neq 0 ( pause & exit /b 1 )
)

:: Build frontend
echo [2/3] Building frontend...
call npx vite build
if %errorlevel% neq 0 (
    echo [ERROR] Frontend build failed
    pause & exit /b 1
)

:: Build Tauri
echo [3/3] Building Tauri desktop app...
call cargo tauri build
if %errorlevel% neq 0 (
    echo [ERROR] Tauri build failed
    pause & exit /b 1
)

echo.
echo  Build complete! Installer located at:
echo    src-tauri\target\release\bundle\
echo.
pause
