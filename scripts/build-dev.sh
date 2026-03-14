#!/usr/bin/env bash
set -euo pipefail

# Quick local dev build script. Assumes rust toolchain and bun are installed.
# For Tauri system dependencies on Ubuntu/Debian:
#   sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev

cd "$(dirname "$0")/.."

echo "==> Installing frontend dependencies..."
cd frontend && bun install && cd ..

echo "==> Building CLI..."
cargo build -p lunk-cli

echo "==> CLI binary at: target/debug/lunk"
echo ""
echo "==> To run the desktop app:"
echo "     cargo install tauri-cli --version '^2'"
echo "     cd crates/lunk-app && cargo tauri dev"
echo ""
echo "==> To build a release:"
echo "     cargo build --release -p lunk-cli"
echo "     cd crates/lunk-app && cargo tauri build"
