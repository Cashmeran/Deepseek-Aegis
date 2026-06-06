#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"

echo ""
echo " === Aegis Desktop ==="
echo ""

# Check Node.js
if ! command -v node &>/dev/null; then
    echo "[ERROR] Node.js not found. Install from https://nodejs.org"
    exit 1
fi

# Check Rust
if ! command -v cargo &>/dev/null; then
    echo "[ERROR] Rust not found. Install from https://rustup.rs"
    exit 1
fi

# Install frontend deps if needed
if [ ! -d "node_modules" ]; then
    echo "[1/2] Installing frontend dependencies..."
    npm install
fi

# Start dev mode
echo "[2/2] Starting Tauri dev server..."
echo ""
cargo tauri dev
