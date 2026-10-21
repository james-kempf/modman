#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the modman AppImage with the Tauri bundler, then replaces the
# generated AppRun with packaging/appimage/AppRun so that CLI invocations
# (`<AppImage> nxm <url>`, `<AppImage> autoupdate run`, any argument) run
# the bundled `modman` CLI and a bare launch starts the GUI.
#
# Usage (from anywhere): packaging/appimage/build-appimage.sh [extra tauri build args]
# Env: SKIP_FRONTEND=1 uses the existing apps/desktop/dist (no `npm run build`).
# Output: target/release/bundle/appimage/modman_<version>_amd64.AppImage
# Idempotent: re-running rebuilds and re-patches from a fresh AppDir.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BUNDLE="$ROOT/target/release/bundle/appimage"
APPDIR="$BUNDLE/modman.AppDir"
TAURI_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/tauri"
PLUGIN="$TAURI_CACHE/linuxdeploy-plugin-appimage.AppImage"

# 1. The CLI first: tauri.conf.json bundle.linux.appimage.files maps
#    /usr/bin/modman to target/release/modman.
cargo build --manifest-path "$ROOT/Cargo.toml" --release --locked -p modman-cli

# 2. Tauri bundle (builds modman-desktop, runs linuxdeploy, packs once).
tauri_args=(build --bundles appimage)
if [[ "${SKIP_FRONTEND:-0}" == 1 ]]; then
    tauri_args+=(--config '{"build":{"beforeBuildCommand":""}}')
fi
(cd "$ROOT/apps/desktop" && npx tauri "${tauri_args[@]}" "$@")

# 3. Swap in the dispatching AppRun (keep the linuxdeploy one as AppRun.gui).
[[ -d "$APPDIR" ]] || { echo "AppDir not found: $APPDIR" >&2; exit 1; }
if ! grep -q 'modman AppImage entry point' "$APPDIR/AppRun" 2>/dev/null; then
    mv -f "$APPDIR/AppRun" "$APPDIR/AppRun.gui"
fi
[[ -x "$APPDIR/AppRun.gui" ]] || { echo "AppRun.gui missing in $APPDIR" >&2; exit 1; }
install -m755 "$ROOT/packaging/appimage/AppRun" "$APPDIR/AppRun"
[[ -x "$APPDIR/usr/bin/modman" ]] || { echo "CLI missing in $APPDIR/usr/bin" >&2; exit 1; }

# 4. Repack with the appimage plugin Tauri already downloaded.
[[ -x "$PLUGIN" ]] || { echo "missing $PLUGIN (run a tauri appimage build once)" >&2; exit 1; }
version=$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$ROOT/apps/desktop/src-tauri/tauri.conf.json" | head -1)
out="$BUNDLE/modman_${version}_amd64.AppImage"
rm -f "$out"
(cd "$BUNDLE" && APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 OUTPUT="$out" "$PLUGIN" --appdir "$APPDIR")
[[ -x "$out" ]] || { echo "AppImage not produced: $out" >&2; exit 1; }
echo "$out"
