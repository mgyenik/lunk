#!/usr/bin/env bash
set -euo pipefail

# Quick local dev build script. Assumes rust toolchain and bun are installed.
# For Tauri system dependencies on Ubuntu/Debian:
#   sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev

cd "$(dirname "$0")/.."

echo "==> Installing frontend dependencies..."
cd frontend && bun install && cd ..

echo "==> Building CLI..."
cargo build -p grymoire-cli

echo "==> CLI binary at: target/debug/grymoire"
echo ""
echo "==> To run the desktop app:"
echo "     cargo install tauri-cli --version '^2'"
echo "     cd crates/grymoire-app && cargo tauri dev"
echo ""
echo "==> To build a release:"
echo "     cargo build --release -p grymoire-cli"
echo "     cd crates/grymoire-app && cargo tauri build"
