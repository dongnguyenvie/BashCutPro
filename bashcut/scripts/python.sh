# shellcheck shell=bash
# The one Python 3 lookup for our bash scripts (sourced, not run): upstream.sh and bashcut/brand/packaging.env.
#
#   . bashcut/scripts/python.sh && py="$(bashcut_python)" && $py script.py   # $py unquoted: it may be "py -3"
#
# $BASHCUT_PYTHON if set (as in the agent kit, #223), else the first of `python3`, `py -3`, `python` whose --version
# says Python 3 (on Windows `python3` is often only the Store alias). Prints the command; returns 1 when none is found.
# Python on Windows writes the console code page (cp1252) and crashes on non-ASCII; force UTF-8 for every caller.
export PYTHONUTF8=1
export PYTHONIOENCODING=utf-8
bashcut_python() {
    if [ -n "${BASHCUT_PYTHON:-}" ]; then
        printf '%s\n' "$BASHCUT_PYTHON"
        return 0
    fi
    local py
    for py in python3 "py -3" python; do
        # shellcheck disable=SC2086 # "py -3" is a command plus an argument
        if $py --version 2>&1 | grep -q '^Python 3\.'; then
            printf '%s\n' "$py"
            return 0
        fi
    done
    echo "no Python 3 found (tried python3, py -3, python); set BASHCUT_PYTHON" >&2
    return 1
}
