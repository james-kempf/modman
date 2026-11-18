#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Install the Vessel icon and a .desktop entry under ~/.local so GNOME/KDE on
# Wayland can show the app icon during `npm run tauri dev`. Wayland compositors
# resolve window icons by matching the app_id (vessel-desktop) to an installed
# .desktop file's StartupWMClass; the embedded window icon is ignored.
# Remove with: scripts/install-dev-desktop.sh --uninstall
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
icons="$repo/apps/desktop/src-tauri/icons"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
apps="$data/applications"
hicolor="$data/icons/hicolor"
desktop="$apps/org.vessel.app.desktop"

refresh() {
  # Best effort: a user hicolor dir often has no index.theme, so the cache can
  # fail to build. GTK ignores a cache older than its dirs and scans instead.
  command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -f -t "$hicolor" 2>/dev/null || true
  command -v update-desktop-database >/dev/null && update-desktop-database "$apps" || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$desktop" \
    "$hicolor/512x512/apps/org.vessel.app.png" \
    "$hicolor/256x256/apps/org.vessel.app.png" \
    "$hicolor/128x128/apps/org.vessel.app.png" \
    "$hicolor/64x64/apps/org.vessel.app.png" \
    "$hicolor/32x32/apps/org.vessel.app.png"
  refresh
  echo "Removed dev desktop entry and icons."
  exit 0
fi

install -Dm644 "$icons/icon.png" "$hicolor/512x512/apps/org.vessel.app.png"
install -Dm644 "$icons/128x128@2x.png" "$hicolor/256x256/apps/org.vessel.app.png"
install -Dm644 "$icons/128x128.png" "$hicolor/128x128/apps/org.vessel.app.png"
install -Dm644 "$icons/64x64.png" "$hicolor/64x64/apps/org.vessel.app.png"
install -Dm644 "$icons/32x32.png" "$hicolor/32x32/apps/org.vessel.app.png"

# The dev binary isn't on PATH; GLib hides entries whose Exec can't be found.
mkdir -p "$apps"
sed "s|^Exec=.*|Exec=$repo/target/debug/vessel-desktop|" \
  "$repo/packaging/common/org.vessel.app.desktop" >"$desktop"
chmod 644 "$desktop"

refresh
echo "Installed $desktop and org.vessel.app icons. Restart the app to pick them up."
