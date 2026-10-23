#!/usr/bin/env bash
# Lint and test our build / generator scripts (ST1, the standard: bashcut/scripts/README.md). Run it whenever you
# change anything under bashcut/scripts, bashcut/packaging or bashcut/brand. Exit 1 if any check fails.
#
#   bashcut/scripts/lint-scripts.sh
#
# bash: `bash -n` + shellcheck. Python: py_compile + ruff (syntax errors and pyflakes, py39). PowerShell: parse check
# with pwsh, or powershell.exe on Windows. Then the unit tests (test_upstream.py, test_generate.py, which includes the determinism test).
# Optional tools (shellcheck, ruff, pwsh): a missing one is skipped with a SKIP line (brew install shellcheck ruff).
set -euo pipefail
case "${1:-}" in
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    "") ;;
    *) echo "lint-scripts: unknown argument $1" >&2; exit 2 ;;
esac
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1
# shellcheck source=bashcut/scripts/python.sh
. bashcut/scripts/python.sh
py="$(bashcut_python)"

failed=0
run() {
    local label="$1"
    shift
    if "$@"; then echo "ok    $label"; else echo "FAIL  $label"; failed=1; fi
}
skip() { echo "SKIP  $*"; }

dirs=(bashcut/scripts bashcut/packaging bashcut/brand)
sh_files=() py_files=() ps_files=()
while IFS= read -r f; do sh_files+=("$f"); done < <(git ls-files -- "${dirs[@]/%//*.sh}" bashcut/brand/packaging.env)
while IFS= read -r f; do py_files+=("$f"); done < <(git ls-files -- "${dirs[@]/%//*.py}")
while IFS= read -r f; do ps_files+=("$f"); done < <(git ls-files -- "${dirs[@]/%//*.ps1}")

for f in "${sh_files[@]}"; do run "bash -n $f" bash -n "$f"; done
if command -v shellcheck >/dev/null 2>&1; then
    run "shellcheck (${#sh_files[@]} files)" shellcheck -x -s bash "${sh_files[@]}"
else
    skip "shellcheck not installed"
fi

# shellcheck disable=SC2086 # $py may be "py -3"
run "py_compile (${#py_files[@]} files)" $py -c 'import ast, sys
for p in sys.argv[1:]:
    with open(p, "rb") as fh:
        compile(fh.read(), p, "exec", dont_inherit=True)' "${py_files[@]}"
if command -v ruff >/dev/null 2>&1; then
    run "ruff (${#py_files[@]} files)" ruff check --quiet --no-cache --isolated --target-version py39 --select E9,F "${py_files[@]}"
else
    skip "ruff not installed"
fi

# PowerShell 7 (pwsh) where it exists, else Windows PowerShell 5.1 (Git Bash on Windows)
ps=""
for c in pwsh powershell.exe; do
    if command -v "$c" >/dev/null 2>&1; then ps="$c"; break; fi
done
if [ -n "$ps" ]; then
    for f in "${ps_files[@]}"; do
        # shellcheck disable=SC2016 # PowerShell variables, expanded by PowerShell
        run "$ps parse $f" "$ps" -NoProfile -NonInteractive -Command '& { param($p) $e = $null; $null = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $p).Path, [ref]$null, [ref]$e); if ($e) { $e | ForEach-Object { Write-Error $_.Message }; exit 1 } }' "$f"
    done
else
    skip "pwsh / powershell.exe not installed: ${#ps_files[@]} PowerShell files not parsed"
fi

# shellcheck disable=SC2086
run "test_upstream.py" $py bashcut/scripts/test_upstream.py
# shellcheck disable=SC2086
run "test_generate.py" $py bashcut/packaging/test_generate.py

if [ "$failed" -ne 0 ]; then
    echo "lint-scripts: failed" >&2
    exit 1
fi
echo "lint-scripts: all green" >&2
