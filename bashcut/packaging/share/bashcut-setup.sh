#!/usr/bin/env bash
# BashCutPro first-run setup (#64): optional, every step can be skipped. macOS / Linux / FreeBSD.
#
#   bashcut-setup.sh [--yes] [--skip-looks] [--skip-profiles] [--skip-kit] [--skip-doctor]
#
# 1. Looks: the curated look pack as .cube LUTs + looks.json in <data dir>/bashcut/looks (bashcut.looks.export).
# 2. Output profiles: compiled into export presets "BashCut <name>" (bashcut.profiles.compile).
# 3. Agent kit: detects claude / codex and offers to clone bashcutpro-agent-kit (needs git and access).
# 4. Plugins and models: nothing installed here; plugins come from the registry in the app (Window > BashCut
#    Plugins) and each plugin downloads its models, checked against models.json, on first use.
# 5. bashcut-doctor: checks the machine and prints fixes.
set -u
yes=0 skip_looks=0 skip_profiles=0 skip_kit=0 skip_doctor=0
for a in "$@"; do
  case "$a" in
    --yes|-y) yes=1 ;;
    --skip-looks) skip_looks=1 ;;
    --skip-profiles) skip_profiles=1 ;;
    --skip-kit) skip_kit=1 ;;
    --skip-doctor) skip_doctor=1 ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "bashcut-setup: unknown argument $a" >&2; exit 2 ;;
  esac
done

here="$(cd "$(dirname "$0")" && pwd)"
# the tools sit next to this script (CLI zip), in ../../MacOS (macOS app: Resources/bashcut) or in ../../bin (Linux / FreeBSD share/)
find_tool() {
  local t="$1" d
  for d in "$here" "$here/../../MacOS" "$here/../../bin" "$here/../../../bin"; do
    if [ -x "$d/$t" ]; then printf '%s\n' "$d/$t"; return 0; fi
  done
  command -v "$t" 2>/dev/null
}
ask() {
  [ "$yes" = 1 ] && return 0
  printf '%s [y/N] ' "$1"
  read -r reply || return 1
  case "$reply" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

if [ -n "${FILMCRAFT_DATA_DIR:-}" ]; then data="$FILMCRAFT_DATA_DIR"
elif [ "$(uname -s)" = Darwin ]; then data="$HOME/Library/Application Support/FilmCraft"
else data="${XDG_DATA_HOME:-$HOME/.local/share}/filmcraft"
fi
cli="$(find_tool filmcraft-cli || true)"
echo "BashCutPro setup: data folder $data, filmcraft-cli ${cli:-not found}"
mkdir -p "$data/bashcut/profiles" "$data/bashcut/templates" "$data/bashcut/looks"

if [ "$skip_looks" = 0 ] && [ -n "$cli" ] && ask "Write the look pack (.cube LUTs) to $data/bashcut/looks?"; then
  "$cli" exec bashcut.looks.export "dir=$data/bashcut/looks" >/dev/null && echo "  looks written" || echo "  looks: failed (skipped)"
fi
if [ "$skip_profiles" = 0 ] && [ -n "$cli" ] && ask "Add the output profiles (TikTok, Reels, Shorts, YouTube…) as export presets?"; then
  "$cli" exec bashcut.profiles.compile >/dev/null && echo "  export presets \"BashCut …\" added" || echo "  profiles: failed (skipped)"
fi
if [ "$skip_kit" = 0 ]; then
  agents=""
  for a in claude codex; do command -v "$a" >/dev/null 2>&1 && agents="$agents $a"; done
  echo "Agent CLIs:${agents:- none found (install Claude Code or Codex to drive BashCutPro with an agent)}"
  kit="$HOME/.bashcutpro/bashcutpro-agent-kit"
  if [ -n "$agents" ] && [ ! -d "$kit" ] && command -v git >/dev/null && ask "Clone the agent kit to $kit?"; then
    git clone --depth 1 https://github.com/dongnguyenvie/bashcutpro-agent-kit "$kit" \
      && echo "  kit cloned; follow its README (Claude Code plugin, or scripts/install-codex.sh)" \
      || echo "  kit: clone failed (skipped)"
  fi
fi
echo "Plugins: install them in the app (Window > BashCut Plugins); models download on first use."
doctor="$(find_tool bashcut-doctor || true)"
if [ "$skip_doctor" = 0 ] && [ -n "$doctor" ]; then
  echo "Checking this machine (bashcut-doctor):"
  "$doctor" ${cli:+--cli "$cli"} || true
fi
