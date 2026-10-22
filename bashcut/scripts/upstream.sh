#!/usr/bin/env bash
# Upstream-sync tooling (issue #38): footprint check, rerere merge, setup. See upstream.py.
#
#   bashcut/scripts/upstream.sh footprint --pr   # before every PR
#   bashcut/scripts/upstream.sh footprint        # everything we carry since the last upstream merge
#   bashcut/scripts/upstream.sh merge [tag]      # sync (UPSTREAM.md "How to sync")
#   bashcut/scripts/upstream.sh --help           # every subcommand and option (upstream.py's argparse)
#
# Python: bashcut/scripts/python.sh ($BASHCUT_PYTHON, else python3, py -3, python).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=bashcut/scripts/python.sh
. "$here/python.sh"
py="$(bashcut_python)" || { echo "upstream.sh: set BASHCUT_PYTHON" >&2; exit 1; }
# shellcheck disable=SC2086 # $py may be "py -3"
exec $py "$here/upstream.py" "$@"
