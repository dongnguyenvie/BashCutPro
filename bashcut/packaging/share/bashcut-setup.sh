#!/usr/bin/env bash
# BashCutPro first-run setup (#64): optional, every step can be skipped. macOS / Linux / FreeBSD.
#
#   bashcut-setup.sh [--yes] [--skip-looks] [--skip-profiles] [--skip-kit] [--skip-doctor]
#
# 1. Looks: the curated look pack as .cube LUTs + looks.json in <data dir>/bashcut/looks (bashcut.looks.export).
# 2. Output profiles: compiled into export presets "BashCut <name>" (bashcut.profiles.compile).
# 3. Agent kit: installs it as a Claude Code plugin when claude is found, and for Codex (clone + install-codex.sh)
#    when codex is found (needs access to the kit repository).
# 4. Plugins and models: nothing installed here; plugins come from the registry in the app (Plugins on the rail) and
#    each plugin downloads its models, checked against models.json, on first use.
# 5. bashcut-doctor: checks the machine and prints fixes.
# No `set -e` on purpose (bashcut/scripts/README.md): a failed optional step is reported and the next one still runs.
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
# the tools sit next to this script (CLI zip), in ../../MacOS (macOS app: Resources/bashcut) or in ../../bin (Linux / FreeBSD share/);
# printed without ../.. (PV2: doctor's fix lines showed share/bashcutpro/../../bin/...);
# find -name matches the exact case (exact file names, T6a; on a Mac volume a case-insensitive match is the app itself, #187)
find_tool() {
  local t="$1" d
  for d in "$here" "$here/../../MacOS" "$here/../../bin" "$here/../../../bin"; do
    if [ -x "$d/$t" ] && [ -n "$(find "$d" -maxdepth 1 -name "$t" -print 2>/dev/null)" ]; then printf '%s/%s\n' "$(cd "$d" && pwd)" "$t"; return 0; fi
  done
  command -v "$t" 2>/dev/null
}
ask() {
  [ "$yes" = 1 ] && return 0
  printf '%s [y/N] ' "$1"
  read -r reply || return 1
  case "$reply" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

# BashCutPro's own data folder, the app's rules (bashcut_dirs::data_dir, SB1); not FilmCraft's
if [ -n "${BASHCUTPRO_DATA_DIR:-}" ]; then data="$BASHCUTPRO_DATA_DIR"
elif [ -n "${FILMCRAFT_DATA_DIR:-}" ]; then data="$FILMCRAFT_DATA_DIR"
elif [ "$(uname -s)" = Darwin ]; then data="$HOME/Library/Application Support/BashCutPro"
else data="${XDG_DATA_HOME:-$HOME/.local/share}/bashcutpro"
fi
# the engine: bashcutpro-cli, its alias bcpro, upstream's filmcraft-cli (NM2; plain bashcutpro is the app on Linux)
cli="$(find_tool bashcutpro-cli || find_tool bcpro || find_tool filmcraft-cli || true)"
echo "BashCutPro setup: data folder $data, command line (bashcutpro-cli) ${cli:-not found}"
mkdir -p "$data/bashcut/profiles" "$data/bashcut/templates" "$data/bashcut/looks"

# looks and profiles are written by the headless engine, not the open app (BASHCUT_BRIDGE=off, CC3)
if [ "$skip_looks" = 0 ] && [ -n "$cli" ] && ask "Write the look pack (.cube LUTs) to $data/bashcut/looks?"; then
  BASHCUT_BRIDGE=off "$cli" exec bashcut.looks.export "dir=$data/bashcut/looks" >/dev/null && echo "  looks written" || echo "  looks: failed (skipped)"
fi
if [ "$skip_profiles" = 0 ] && [ -n "$cli" ] && ask "Add the output profiles (TikTok, Reels, Shorts, YouTube…) as export presets?"; then
  BASHCUT_BRIDGE=off "$cli" exec bashcut.profiles.compile >/dev/null && echo "  export presets \"BashCut …\" added" || echo "  profiles: failed (skipped)"
fi
if [ "$skip_kit" = 0 ]; then
  agents=""
  for a in claude codex; do command -v "$a" >/dev/null 2>&1 && agents="$agents $a"; done
  echo "Agent CLIs:${agents:- none found (install Claude Code or Codex to drive BashCutPro with an agent)}"
  # Claude Code: install the kit as its plugin (the kit README's way); Codex: clone it and run its installer
  if command -v claude >/dev/null 2>&1 && ask "Install the agent kit as a Claude Code plugin (bcp)?"; then
    { claude plugin marketplace add dongnguyenvie/bashcutpro-agent-kit && claude plugin install bcp@bashcutpro-agent-kit; } >/dev/null 2>&1 \
      && echo "  kit installed in Claude Code; in Claude Code run /bcp:setup once" \
      || echo "  kit: Claude Code plugin install failed (skipped); see the kit README"
  fi
  kit="$HOME/.bashcutpro/bashcutpro-agent-kit"
  if command -v codex >/dev/null 2>&1 && [ ! -d "$kit" ] && command -v git >/dev/null && ask "Install the agent kit for Codex (clone to $kit)?"; then
    git clone --depth 1 https://github.com/dongnguyenvie/bashcutpro-agent-kit "$kit" && sh "$kit/scripts/install-codex.sh" \
      && echo "  kit installed for Codex" \
      || echo "  kit: Codex install failed (skipped); see the kit README"
  fi
fi
echo "Plugins: install them in the app (the Plugins icon on the right-hand rail, or Window > BashCutPro > Plugins); models download on first use."
doctor="$(find_tool bashcut-doctor || true)"
if [ "$skip_doctor" = 0 ] && [ -n "$doctor" ]; then
  echo "Checking this machine (bashcut-doctor):"
  "$doctor" ${cli:+--cli "$cli"} || true
fi
