#!/usr/bin/env bash
# Run the desktop app in Tauri dev mode (Vite + hot-reloading Rust backend).
# Usage: ./dev.sh
set -euo pipefail

cd "$(dirname "$0")/apps/desktop"

if [ ! -d node_modules ]; then
  npm install
fi

exec npm run tauri dev
