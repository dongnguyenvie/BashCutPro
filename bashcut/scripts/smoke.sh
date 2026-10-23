#!/usr/bin/env bash
# BashCutPro smoke check (#54) for macOS and Linux builds. Windows: smoke.ps1.
#
# Launches the built app with --control, waits for its window, takes ui.screenshot, checks the window title says
# BashCutPro, runs `bashcutpro-cli commands razor` (or `filmcraft-cli`) and an MCP command_list, quits the app; then scans x86-64 release
# binaries for AVX / AVX2 / AVX-512 instructions outside the allowed (runtime-dispatched) functions.
# Exits non-zero on any failure and never reports success without having launched the app.
#
#   bashcut/scripts/smoke.sh --app target/release/filmcraft [--cli target/release/bashcutpro-cli]
#   bashcut/scripts/smoke.sh --app dist/BashCutPro.app            # the executable inside the bundle
#   bashcut/scripts/smoke.sh --isa-only target/release/filmcraft target/release/filmcraft-cli
#
# Options: --port N (default: a free port from 19000), --timeout S (60), --out DIR (target/smoke),
#          --expect-title TEXT (BashCutPro), --no-isa, --isa-allow FILE (bashcut/scripts/isa-allow.txt)
# Needs: bash (for /dev/tcp), objdump or llvm-objdump for the scan (x86-64 binaries only).
# No `set -e` on purpose (bashcut/scripts/README.md): every check runs and reports, and the exit code counts the FAILs.
set -u

here="$(cd "$(dirname "$0")" && pwd)"
app="" cli="" port="" timeout=60 out="target/smoke" expect="BashCutPro" isa=1 isa_only=0
allow="$here/isa-allow.txt"
isa_bins=()

while [ $# -gt 0 ]; do
  case "$1" in
    --app) app="$2"; shift 2 ;;
    --cli) cli="$2"; shift 2 ;;
    --port) port="$2"; shift 2 ;;
    --timeout) timeout="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --expect-title) expect="$2"; shift 2 ;;
    --no-isa) isa=0; shift ;;
    --isa-allow) allow="$2"; shift 2 ;;
    --isa-only) isa_only=1; shift; while [ $# -gt 0 ] && [ "${1#--}" = "$1" ]; do isa_bins+=("$1"); shift; done ;;
    -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
    *) echo "smoke: unknown argument $1" >&2; exit 2 ;;
  esac
done

failures=0 passes=0
pass() { passes=$((passes + 1)); echo "PASS  $*"; }
fail() { failures=$((failures + 1)); echo "FAIL  $*"; }
note() { echo "      $*"; }

# ---------------------------------------------------------------------------------------------- ISA scan
# Any ymm register is AVX/AVX2, any zmm or opmask (k1–k7) register AVX-512: none belongs in a baseline x86-64 build,
# except in functions that pick their code at run time (listed, one regex per line, in the allow file).
isa_scan() {
  local bin="$1" dump arch
  [ -f "$bin" ] || { fail "isa: $bin not found"; return; }
  arch="$(file -b "$bin" 2>/dev/null)"
  case "$arch" in
    *x86-64*|*x86_64*) ;;
    *) note "isa: $bin is not x86-64 ($arch); skipped"; return ;;
  esac
  dump="$(command -v llvm-objdump || command -v objdump || true)"
  [ -n "$dump" ] || { fail "isa: no objdump / llvm-objdump to disassemble $bin"; return; }
  local report
  report="$("$dump" -d --no-show-raw-insn "$bin" 2>/dev/null | awk -v allow="$allow" '
    BEGIN { while ((getline l < allow) > 0) { if (l !~ /^#/ && l != "") pats[++n] = l } }
    /^[0-9a-f]+ <.*>:$/ { fn = $0; sub(/^[0-9a-f]+ </, "", fn); sub(/>:$/, "", fn); next }
    /%ymm|%zmm|%k[1-7][^0-9]|[^a-z]ymm[0-9]|[^a-z]zmm[0-9]/ {
      kind = ($0 ~ /zmm|%k[1-7]/) ? "AVX-512" : "AVX/AVX2"
      ok = 0; for (i = 1; i <= n; i++) if (fn ~ pats[i]) { ok = 1; break }
      if (!ok && !((fn, kind) in seen)) { seen[fn, kind] = 1; print kind "\t" fn }
    }')"
  if [ -z "$report" ]; then
    pass "isa: $bin has no AVX/AVX2/AVX-512 outside allowed functions"
  else
    fail "isa: $bin uses wide vector instructions in $(printf '%s\n' "$report" | wc -l | tr -d ' ') function(s):"
    printf '%s\n' "$report" | head -20 | while IFS= read -r l; do note "$l"; done
  fi
}

if [ "$isa_only" = 1 ]; then
  [ ${#isa_bins[@]} -gt 0 ] || { echo "smoke: --isa-only needs binaries" >&2; exit 2; }
  for b in "${isa_bins[@]}"; do isa_scan "$b"; done
  echo "smoke (isa): $passes passed, $failures failed"
  [ "$failures" = 0 ]; exit $?
fi

# ---------------------------------------------------------------------------------------------- launch
[ -n "$app" ] || { echo "smoke: --app <path to the built app> is required" >&2; exit 2; }
exe="$app"
if [ -d "$app" ] && [ -d "$app/Contents/MacOS" ]; then
  exe="$(find "$app/Contents/MacOS" -maxdepth 1 -type f -perm -u+x | head -1)"
fi
if [ -z "$exe" ] || [ ! -x "$exe" ]; then fail "launch: $app is not an executable or an .app bundle"; echo "smoke: 0 passed, 1 failed"; exit 1; fi
if [ -z "$cli" ]; then
  # the engine lookup order (bashcut_dirs::engine, NM1): bashcutpro-cli, bcpro, filmcraft-cli; next to the app, else
  # on PATH (never plain bashcutpro: that is the Linux GUI)
  for name in bashcutpro-cli bcpro filmcraft-cli; do
    if [ -x "$(dirname "$exe")/$name" ] && [ -f "$(dirname "$exe")/$name" ]; then cli="$(dirname "$exe")/$name"; break; fi
  done
  for name in bashcutpro-cli bcpro filmcraft-cli; do
    [ -n "$cli" ] && break
    cli="$(command -v "$name" || true)"
  done
fi
if [ -z "$port" ]; then
  port=19000
  while (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; do port=$((port + 1)); done
fi
mkdir -p "$out"
out="$(cd "$out" && pwd)"

# The window's control channel needs a session token (FILMCRAFT_CONTROL_TOKEN, 64 hex). Use the caller's, else make
# one before the launch: the app reads it from the environment and every request below carries it. Never echoed.
tok="${FILMCRAFT_CONTROL_TOKEN:-$(python3 -c 'import secrets;print(secrets.token_hex(32))' 2>/dev/null || od -An -N32 -tx1 /dev/urandom | tr -d ' \n')}"
export FILMCRAFT_CONTROL_TOKEN="$tok"

echo "smoke: $exe on port $port (cli: ${cli:-none}, out: $out)"
"$exe" --control "$port" --control-input-only >"$out/app.log" 2>&1 &
pid=$!
cleanup() { kill "$pid" 2>/dev/null; }
trap cleanup EXIT

# One request on a fresh connection; prints the reply line (empty on failure). The session token goes into the JSON
# line of every request (ui.inspect, ui.elements, ui.screenshot, app.quit).
request() {
  local reply="" line="$1"
  line="{\"token\":\"$tok\",${line#\{}"
  exec 3<>"/dev/tcp/127.0.0.1/$port" 2>/dev/null || return 1
  printf '%s\n' "$line" >&3
  IFS= read -r -t "${2:-30}" reply <&3
  exec 3<&- 3>&-
  printf '%s' "$reply"
}

ready=0
for _ in $(seq 1 $((timeout * 2))); do
  kill -0 "$pid" 2>/dev/null || break
  r="$(request '{"id":1,"method":"ui.inspect"}' 5 2>/dev/null || true)"
  case "$r" in *'"ok":true'*|*'"ok": true'*) ready=1; break ;; esac
  sleep 0.5
done
if [ "$ready" != 1 ]; then
  fail "launch: no answer on the control port within ${timeout}s (log: $out/app.log)"
  tail -20 "$out/app.log" 2>/dev/null | while IFS= read -r l; do note "$l"; done
  echo "smoke: $passes passed, $failures failed"
  exit 1
fi
pass "launch: the app answers ui.inspect (pid $pid)"

# ---------------------------------------------------------------------------------------------- window
shot="$out/smoke.png"
rm -f "$shot"
r="$(request "{\"id\":2,\"method\":\"ui.screenshot\",\"params\":{\"path\":\"$shot\"}}" 30 || true)"
if [ -s "$shot" ] && [ "$(head -c 4 "$shot" | tail -c 3)" = "PNG" ]; then
  pass "screenshot: $shot"
else
  fail "screenshot: no PNG written ($r)"
fi

title=""
case "$(uname -s)" in
  Darwin)
    title="$(osascript -e "tell application \"System Events\" to get name of every window of (first process whose unix id is $pid)" 2>/dev/null || true)"
    [ -n "$title" ] || title="$(osascript -e "tell application \"System Events\" to get name of (first process whose unix id is $pid)" 2>/dev/null || true)" ;;
  Linux)
    if command -v xdotool >/dev/null; then
      title="$(xdotool search --pid "$pid" getwindowname 2>/dev/null | tr '\n' ' ')"
    elif command -v wmctrl >/dev/null; then
      title="$(wmctrl -lp | awk -v p="$pid" '$3 == p { $1 = $2 = $3 = $4 = ""; print }')"
    fi ;;
esac
if [ -n "$title" ]; then
  case "$title" in
    *"$expect"*) pass "title: \"$title\"" ;;
    *) fail "title: \"$title\" does not say $expect" ;;
  esac
else
  # no window-title tool (or no permission): the app's own UI must show the name
  els="$(request '{"id":3,"method":"ui.elements"}' 30 || true)"
  case "$els" in
    *"$expect"*) pass "title: window title unreadable here; the UI shows $expect" ;;
    *) fail "title: window title unreadable here and the UI does not show $expect (macOS: allow Terminal in Privacy ▸ Accessibility)" ;;
  esac
fi

# ---------------------------------------------------------------------------------------------- CLI and MCP
# The CLI checks test the engine itself: without this, `commands` would bridge to the app launched above (CC3).
export BASHCUT_BRIDGE=off
# Run a command with a time limit (macOS has no `timeout`); its output goes to $2.
run_limited() {
  local secs="$1" file="$2"; shift 2
  "$@" >"$file" 2>&1 &
  local p=$! i=0
  while kill -0 "$p" 2>/dev/null; do
    i=$((i + 1))
    [ "$i" -gt $((secs * 10)) ] && { kill "$p" 2>/dev/null; wait "$p" 2>/dev/null; return 124; }
    sleep 0.1
  done
  wait "$p"
}

if [ -z "$cli" ] || [ ! -x "$cli" ]; then
  fail "cli: bashcutpro-cli / bcpro / filmcraft-cli not found (pass --cli)"
else
  if run_limited 60 "$out/cli-razor.txt" "$cli" commands razor && grep -qi razor "$out/cli-razor.txt"; then
    pass "cli: commands razor ($(wc -l <"$out/cli-razor.txt" | tr -d ' ') line(s))"
  else
    fail "cli: commands razor failed ($out/cli-razor.txt)"
  fi
  mcp_in="$out/mcp-in.jsonl"
  {
    echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"1"}}}'
    echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"command_list","arguments":{}}}'
  } >"$mcp_in"
  run_limited 60 "$out/mcp-out.jsonl" sh -c "\"$cli\" --demo mcp < \"$mcp_in\""
  if grep -q '"id":2' "$out/mcp-out.jsonl" && grep '"id":2' "$out/mcp-out.jsonl" | grep -q 'file.newProject' && ! grep '"id":2' "$out/mcp-out.jsonl" | grep -q '"isError":true'; then
    pass "mcp: command_list answers"
  else
    fail "mcp: command_list gave no command list ($out/mcp-out.jsonl)"
  fi
fi

# ---------------------------------------------------------------------------------------------- quit
request '{"id":9,"method":"app.quit"}' 10 >/dev/null || true
for _ in $(seq 1 30); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
if kill -0 "$pid" 2>/dev/null; then
  fail "quit: the app is still running 15 s after app.quit"
else
  pass "quit: the app exited"
fi
trap - EXIT
cleanup

# ---------------------------------------------------------------------------------------------- ISA
if [ "$isa" = 1 ]; then
  isa_scan "$exe"
  [ -n "$cli" ] && [ -x "$cli" ] && isa_scan "$cli"
fi

echo "smoke: $passes passed, $failures failed ($(uname -s) $(uname -m), $(date -u +%Y-%m-%dT%H:%MZ))"
[ "$failures" = 0 ]
