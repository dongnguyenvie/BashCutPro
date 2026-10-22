#!/usr/bin/env bash
# The fmt gate, Windows-safe (TL1): `cargo fmt --all -- --check`, and when that dies with
# "os error 206" (the rustfmt command line for the whole workspace is too long for Windows),
# `cargo fmt -p <crate> -- --check` once per workspace member. Exit 1 if any check fails.
#
#   bashcut/scripts/fmt-check.sh
set -euo pipefail
case "${1:-}" in
    -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
    "") ;;
    *) echo "fmt-check: unknown argument $1" >&2; exit 2 ;;
esac
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

err="$(mktemp)"
trap 'rm -f "$err"' EXIT

rc=0
cargo fmt --all -- --check 2>"$err" || rc=$?
if [ "$rc" -eq 0 ] || ! grep -q "os error 206" "$err"; then
    cat "$err" >&2
    exit "$rc"
fi

echo "fmt-check: whole-workspace command line too long (os error 206), checking per crate" >&2
# Package objects in `cargo metadata` start with "name" then "version"; dependency and target
# objects never have that pair, so this lists exactly the workspace members.
crates="$(cargo metadata --no-deps --format-version 1 | grep -o '"name":"[^"]*","version":"' | cut -d'"' -f4 || true)"
if [ -z "$crates" ]; then
    echo "fmt-check: cargo metadata listed no workspace members" >&2
    exit 1
fi

failed=()
for crate in $crates; do
    cargo fmt -p "$crate" -- --check || failed+=("$crate")
done
if [ "${#failed[@]}" -gt 0 ]; then
    echo "fmt-check: not formatted (or rustfmt failed): ${failed[*]}" >&2
    exit 1
fi
echo "fmt-check: $(echo "$crates" | wc -l | tr -d ' ') crates clean" >&2
