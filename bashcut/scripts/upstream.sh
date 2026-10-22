#!/usr/bin/env bash
# Upstream-sync tooling (issue #38): footprint check, rerere merge, setup. See upstream.py.
#
#   bashcut/scripts/upstream.sh footprint --pr   # before every PR
#   bashcut/scripts/upstream.sh footprint        # everything we carry since the last upstream merge
#   bashcut/scripts/upstream.sh merge [tag]      # sync (UPSTREAM.md "How to sync")
#
# Python: $BASHCUT_PYTHON if set (as in the agent kit, #223), else the first of `python3`, `py -3`,
# `python` whose --version says Python 3 (on Windows `python3` is often only the Store alias).
set -euo pipefail
script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/upstream.py"

if [ -n "${BASHCUT_PYTHON:-}" ]; then
    exec $BASHCUT_PYTHON "$script" "$@"
fi
for py in python3 "py -3" python; do
    if $py --version 2>&1 | grep -q '^Python 3\.'; then
        exec $py "$script" "$@"
    fi
done
echo "upstream.sh: no Python 3 found (tried python3, py -3, python); set BASHCUT_PYTHON" >&2
exit 1
