#!/usr/bin/env bash
# The MCP command for agents (reference for the kit's scripts/bashcutpro-mcp.sh, #51): runs
# bashcut-mcp, which bridges to the open BashCutPro window when it publishes
# <data dir>/bashcut/automation.json, else starts a headless engine (with the project's engine lock),
# and checks writes against the project folder and BASHCUTPRO_ROOTS. See bashcut/agent-connect.md.
#
#   claude mcp add bashcutpro -- bashcut/scripts/bashcutpro-mcp.sh [--project my.fcproj]
set -euo pipefail
if command -v bashcut-mcp >/dev/null 2>&1; then
  exec bashcut-mcp "$@"
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
for dir in "${CARGO_TARGET_DIR:-$ROOT/target}/release" "${CARGO_TARGET_DIR:-$ROOT/target}/debug"; do
  if [ -x "$dir/bashcut-mcp" ]; then
    exec "$dir/bashcut-mcp" "$@"
  fi
done
echo "bashcutpro-mcp: bashcut-mcp not found (cargo build -p bashcut-mcp)" >&2
exit 2
