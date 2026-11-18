#!/usr/bin/env bash
# Regenerates every brand PNG from its SVG source:
#   apps/desktop/src-tauri/icons/icon.svg -> icon-1024.png -> Tauri icon set
#   packaging/steam/*.svg                 -> packaging/steam/*.png
# Needs inkscape, ImageMagick (magick) and the desktop devDependencies (Tauri CLI).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
icons="$root/apps/desktop/src-tauri/icons"
steam="$root/packaging/steam"

for tool in inkscape magick npx; do
  command -v "$tool" >/dev/null || { echo "render-brand-art: $tool not found" >&2; exit 1; }
done

# render <svg> <png> <width> <height>; fails unless the PNG has exactly that size.
render() {
  rm -f "$2"
  inkscape "$1" --export-type=png --export-filename="$2" -w "$3" -h "$4" 2>/dev/null
  local got
  got="$(magick identify -format '%wx%h' "$2" 2>/dev/null)" || { echo "render-brand-art: $2 not written" >&2; exit 1; }
  [[ "$got" == "$3x$4" ]] || { echo "render-brand-art: $2 is $got, want $3x$4" >&2; exit 1; }
  echo "  $2 ($got)"
}

echo "App icon"
render "$icons/icon.svg" "$icons/icon-1024.png" 1024 1024

# tauri icon also writes Windows Store, iOS and Android sets; generate into a
# temp dir and keep only the files tauri.conf.json and the packaging use.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
out="$(cd "$root/apps/desktop" && npx --no-install tauri icon "$icons/icon-1024.png" -o "$tmp" 2>&1)" \
  || { echo "$out" >&2; echo "render-brand-art: tauri icon failed" >&2; exit 1; }
for f in 32x32.png 64x64.png 128x128.png 128x128@2x.png icon.png icon.ico icon.icns; do
  [[ -s "$tmp/$f" ]] || { echo "render-brand-art: tauri icon did not write $f" >&2; exit 1; }
  cp "$tmp/$f" "$icons/$f"
  echo "  $icons/$f"
done

echo "Steam artwork"
render "$steam/capsule.svg" "$steam/capsule.png" 600 900
render "$steam/wide.svg"    "$steam/wide.png"    920 430
render "$steam/hero.svg"    "$steam/hero.png"    3840 1240
render "$steam/logo.svg"    "$steam/logo.png"    1280 720
render "$icons/icon.svg"    "$steam/icon.png"    256 256
