#!/usr/bin/env bash
set -euo pipefail

REPO="Cashmeran/Deepseek-Aegis"
BIN="aegis"

# ── Detect OS/Arch ──────────────────────────────────────────────────
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Linux)  PLATFORM="linux-x86_64" ;;
  Darwin) PLATFORM="macos-$(uname -m)" ;;
  MINGW*|MSYS*) PLATFORM="windows-x86_64" ;;
  *)
    echo "Unsupported OS: $OS"
    echo "Download manually from https://github.com/$REPO/releases"
    exit 1
    ;;
esac

# ── Get latest version ──────────────────────────────────────────────
VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$VERSION" ]; then
  echo "Error: could not determine latest version"
  exit 1
fi

echo "Installing aegis $VERSION for $PLATFORM..."

# ── Download ────────────────────────────────────────────────────────
URL="https://github.com/$REPO/releases/download/$VERSION/aegis-$PLATFORM.tar.gz"
TMPDIR=$(mktemp -d)
curl -fsSL "$URL" | tar xz -C "$TMPDIR"

# ── Install to ~/.local/bin ─────────────────────────────────────────
INSTALL_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
mkdir -p "$INSTALL_DIR"
cp "$TMPDIR/aegis" "$INSTALL_DIR/aegis"
cp "$TMPDIR/aegis-diag" "$INSTALL_DIR/aegis-diag" 2>/dev/null || true
chmod +x "$INSTALL_DIR/aegis"

rm -rf "$TMPDIR"

# ── PATH check ───────────────────────────────────────────────────────
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
  echo ""
  echo "Add this to your shell config (~/.bashrc, ~/.zshrc):"
  echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
  echo ""
fi

echo "Installed to $INSTALL_DIR/aegis"
echo "Run: aegis --version"
