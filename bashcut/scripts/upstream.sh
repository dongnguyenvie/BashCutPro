#!/usr/bin/env bash
# Upstream-sync tooling (issue #38): footprint check, rerere merge, setup. See upstream.py.
#
#   bashcut/scripts/upstream.sh footprint --pr   # before every PR
#   bashcut/scripts/upstream.sh footprint        # everything we carry since the last upstream merge
#   bashcut/scripts/upstream.sh merge [tag]      # sync (UPSTREAM.md "How to sync")
set -euo pipefail
exec python3 "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/upstream.py" "$@"
