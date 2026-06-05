@echo off
cd /d "%~dp0"
if not exist "node_modules" (
    echo Installing frontend dependencies...
    call npm install
)
echo Building Aegis Desktop (production)...
call cargo tauri build
echo.
echo Build complete! Find the installer in: src-tauri\target\release\bundle
pause
