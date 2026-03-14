#!/usr/bin/env bash
set -euo pipefail

# Lunk installer for Linux/macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/mgyenik/lunk/main/scripts/install.sh | bash
#    or: ./install.sh [version]

REPO="mgyenik/lunk"
INSTALL_DIR="${LUNK_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${1:-latest}"

info() { printf "\033[1;34m==>\033[0m %s\n" "$1"; }
ok()   { printf "\033[1;32m==>\033[0m %s\n" "$1"; }
err()  { printf "\033[1;31merror:\033[0m %s\n" "$1" >&2; exit 1; }

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  PLATFORM="linux" ;;
  Darwin) PLATFORM="darwin" ;;
  *)      err "Unsupported OS: $OS" ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *)             err "Unsupported architecture: $ARCH" ;;
esac

BINARY_NAME="lunk-${PLATFORM}-${ARCH}"

# Resolve version
if [ "$VERSION" = "latest" ]; then
  info "Fetching latest release..."
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)
  if [ -z "$VERSION" ]; then
    err "Could not determine latest version. Specify a version: ./install.sh v0.1.0"
  fi
fi

info "Installing lunk ${VERSION} for ${PLATFORM}-${ARCH}"

# Download
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${BINARY_NAME}"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

info "Downloading from ${DOWNLOAD_URL}..."
curl -fsSL -o "${TMP_DIR}/lunk" "$DOWNLOAD_URL" || err "Download failed. Check that version ${VERSION} exists."

# Install
mkdir -p "$INSTALL_DIR"
chmod +x "${TMP_DIR}/lunk"
mv "${TMP_DIR}/lunk" "${INSTALL_DIR}/lunk"

ok "Installed lunk to ${INSTALL_DIR}/lunk"

# Check PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
  echo ""
  echo "  Add to your PATH:"
  echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
  echo ""
  echo "  Or add to your shell profile (~/.bashrc, ~/.zshrc):"
  echo "    echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc"
fi

echo ""
ok "Run 'lunk --help' to get started"
