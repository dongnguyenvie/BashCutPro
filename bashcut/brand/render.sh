#!/usr/bin/env bash
# Render every BashCutPro brand file from the SVGs that bashcut/brand/generate.py writes (the
# BashCutPro counterpart of packaging/icons.sh, which stays upstream's and renders FilmCraft's icon).
#
# Needs: resvg or rsvg-convert (brew install resvg / librsvg). On macOS, iconutil also writes the
# .icns; the .ico is packed by `cargo xtask ico`. The outputs are committed, so builds and packaging
# never need these tools. Every output needs its .attribution sidecar and an ATTRIBUTION.md row.
#
#   bashcut/brand/render.sh
set -euo pipefail
case "${1:-}" in
  -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
  "") ;;
  *) echo "render: unknown argument $1" >&2; exit 2 ;;
esac
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DIR="$HERE/app-icon"
SVG="$DIR/bashcutpro.svg"
# The Linux icon name follows the desktop file id, which is still upstream's (bashcut/rebrand.md).
APP_ID="ai.storyteller.filmcraft"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if command -v resvg >/dev/null; then
  render() { resvg -w "$2" -h "$2" "$1" "$3" </dev/null; }
  render_h() { resvg -h "$2" "$1" "$3" </dev/null; }
elif command -v rsvg-convert >/dev/null; then
  render() { rsvg-convert -w "$2" -h "$2" "$1" -o "$3"; }
  render_h() { rsvg-convert -h "$2" "$1" -o "$3"; }
else
  echo "error: resvg or rsvg-convert not found (brew install resvg)" >&2
  exit 1
fi

# macOS icons follow Apple's grid: the tile body is 824 of 1024 px with a transparent margin, so
# the Dock shows it at the same size as other apps (same numbers as packaging/icons.sh).
MAC="$TMP/mac.svg"
sed 's/viewBox="0 0 512 512"/viewBox="-62.14 -62.14 636.27 636.27"/' "$SVG" >"$MAC"
grep -q 'viewBox="-62.14' "$MAC" || { echo "error: unexpected viewBox in $SVG" >&2; exit 1; }

render "$SVG" 1024 "$DIR/bashcutpro-1024.png"
render "$SVG" 512 "$DIR/bashcutpro-512.png"
# Runtime window/Dock icon on macOS (apps/filmcraft/src/main.rs embeds it).
render "$MAC" 512 "$DIR/bashcutpro-macos-512.png"

# Linux hicolor theme (full-bleed tile).
for s in 16 24 32 48 64 128 256 512; do
  mkdir -p "$DIR/hicolor/${s}x${s}/apps"
  render "$SVG" "$s" "$DIR/hicolor/${s}x${s}/apps/$APP_ID.png"
done
mkdir -p "$DIR/hicolor/scalable/apps"
cp "$SVG" "$DIR/hicolor/scalable/apps/$APP_ID.svg"

# Windows .ico.
ICO_PNGS=()
for s in 16 20 24 32 40 48 64 128 256; do
  render "$SVG" "$s" "$TMP/ico-$s.png"
  ICO_PNGS+=("$TMP/ico-$s.png")
done
(cd "$ROOT" && cargo run -q -p xtask -- ico "$DIR/bashcutpro.ico" "${ICO_PNGS[@]}")

# macOS .icns.
if command -v iconutil >/dev/null; then
  SET="$TMP/bashcutpro.iconset"
  mkdir -p "$SET"
  for s in 16 32 128 256 512; do
    render "$MAC" "$s" "$SET/icon_${s}x${s}.png"
    render "$MAC" $((s * 2)) "$SET/icon_${s}x${s}@2x.png"
  done
  iconutil -c icns -o "$DIR/bashcutpro.icns" "$SET"
else
  echo "warning: iconutil not found (macOS only); bashcutpro.icns not regenerated" >&2
fi

# Wordmarks (crates/ui-egui/src/brand.rs embeds the PNGs): dark ink for light themes, white ink
# for dark themes.
for w in bashcutpro-wordmark bashcutpro-wordmark-white; do
  render_h "$HERE/wordmark/$w.svg" 512 "$HERE/wordmark/$w.png"
done
echo "brand files written to $HERE"
