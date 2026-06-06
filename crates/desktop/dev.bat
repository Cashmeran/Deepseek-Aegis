@echo off
setlocal enabledelayedexpansion
cd /d "%~dp0"

echo.
echo  === Aegis Desktop ===
echo.

:: Check Node.js
where node >nul 2>&1
if %errorlevel% neq 0 (
    echo [ERROR] Node.js not found. Install from https://nodejs.org
    pause & exit /b 1
)

:: Check Rust
where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo [ERROR] Rust not found. Install from https://rustup.rs
    pause & exit /b 1
)

:: Install frontend deps if needed
if not exist "node_modules" (
    echo [1/2] Installing frontend dependencies...
    call npm install
    if %errorlevel% neq 0 ( pause & exit /b 1 )
)

:: Start dev mode
echo [2/2] Starting Tauri dev server...
echo.
call cargo tauri dev

pause
