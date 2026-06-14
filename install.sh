#!/usr/bin/env bash
set -euo pipefail

REPO="Cashmeran/Deepseek-Aegis"
BIN="aegis"

# ── Color helpers ───────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# ── Detect OS/Arch ──────────────────────────────────────────────────
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Linux)  PLATFORM="linux-x86_64" ;;
  Darwin)
    case "$ARCH" in
      arm64)  PLATFORM="macos-aarch64-apple-darwin" ;;
      x86_64) PLATFORM="macos-x86_64-apple-darwin" ;;
      *)      echo -e "${RED}Unsupported Mac architecture: $ARCH${NC}"; exit 1 ;;
    esac ;;
  MINGW*|MSYS*) PLATFORM="windows-x86_64" ;;
  *)
    echo -e "${RED}Unsupported OS: $OS${NC}"
    echo "Download manually from https://github.com/$REPO/releases"
    exit 1
    ;;
esac

# ── Dependency check ────────────────────────────────────────────────
for cmd in curl tar; do
  if ! command -v "$cmd" &>/dev/null; then
    echo -e "${RED}Error: '$cmd' is required but not installed.${NC}"
    exit 1
  fi
done

# ── Get latest version ──────────────────────────────────────────────
echo -e "Fetching latest version..."
VERSION=$(curl -fsSL --retry 3 --retry-delay 2 "https://api.github.com/repos/$REPO/releases/latest" \
  | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$VERSION" ]; then
  echo -e "${RED}Error: could not determine latest version. Check your network or GitHub API rate limit.${NC}"
  echo "You can download manually from https://github.com/$REPO/releases"
  exit 1
fi

echo -e "${GREEN}Installing aegis $VERSION for $PLATFORM...${NC}"

# ── Download ────────────────────────────────────────────────────────
URL="https://github.com/$REPO/releases/download/$VERSION/aegis-$PLATFORM.tar.gz"
TMPDIR=$(mktemp -d)
ARCHIVE="$TMPDIR/aegis.tar.gz"

if ! curl -fsSL --retry 3 --retry-delay 2 -o "$ARCHIVE" "$URL"; then
  echo -e "${RED}Error: download failed.${NC}"
  echo "URL: $URL"
  echo "Check your network or download manually from https://github.com/$REPO/releases"
  rm -rf "$TMPDIR"
  exit 1
fi

if ! tar xzf "$ARCHIVE" -C "$TMPDIR"; then
  echo -e "${RED}Error: failed to extract archive. The download may be corrupted.${NC}"
  rm -rf "$TMPDIR"
  exit 1
fi

# ── Verify binary exists ────────────────────────────────────────────
if [ ! -f "$TMPDIR/aegis" ]; then
  echo -e "${RED}Error: archive does not contain 'aegis' binary.${NC}"
  ls -la "$TMPDIR"
  rm -rf "$TMPDIR"
  exit 1
fi

# ── Install to ~/.local/bin ─────────────────────────────────────────
INSTALL_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
mkdir -p "$INSTALL_DIR"
cp "$TMPDIR/aegis" "$INSTALL_DIR/aegis"
cp "$TMPDIR/aegis-diag" "$INSTALL_DIR/aegis-diag" 2>/dev/null || true
chmod +x "$INSTALL_DIR/aegis"

rm -rf "$TMPDIR"

# ── Basic smoke test ────────────────────────────────────────────────
if "$INSTALL_DIR/aegis" --version >/dev/null 2>&1; then
  echo -e "${GREEN}Binary verified OK.${NC}"
else
  echo -e "${YELLOW}Warning: binary may not be executable. Try running '$INSTALL_DIR/aegis' directly.${NC}"
fi

# ── PATH check ───────────────────────────────────────────────────────
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
  echo ""
  echo -e "${YELLOW}Add this to your shell config (~/.bashrc, ~/.zshrc):${NC}"
  echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
  echo ""
fi

echo -e "${GREEN}Installed to $INSTALL_DIR/aegis${NC}"
echo "Run: aegis --help"
