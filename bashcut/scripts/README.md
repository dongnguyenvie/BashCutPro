# Our scripts: the standard (ST1)

Every build, sync and generator script BashCutPro adds follows this page. Check it with
`bashcut/scripts/lint-scripts.sh` whenever you change anything under `bashcut/scripts/`, `bashcut/packaging/` or
`bashcut/brand/` (part of the gate for script changes; see `AGENTS.md`).

## Where scripts live

| Kind | Folder |
|---|---|
| Upstream sync, dev and check tools | `bashcut/scripts/` |
| Packaging (the generated copy of upstream's `packaging/`, first-run setup shipped in packages) | `bashcut/packaging/` |
| Brand assets (SVG generator, icon renderer, brand variables for packaging) | `bashcut/brand/` |
| Build-time code generation | Rust: `build.rs` of our crate (`apps/bashcutpro-cli/build.rs`), `xtask/src/bashcut.rs` |

Never edit an upstream file in place from a script: generators write a copy (`target/bashcut-packaging`, `OUT_DIR`)
and upstream calls into ours through a recorded hook line (`bashcut/extending-upstream.md`).

## Every script

| Script | Purpose | Run | OS |
|---|---|---|---|
| `scripts/upstream.sh` (+ `upstream.py`, `test_upstream.py`) | footprint check, upstream merge, setup | `bashcut/scripts/upstream.sh footprint --pr` | all (Git Bash on Windows) |
| `scripts/fmt-check.sh` | fmt gate, per-crate fallback on Windows os error 206 | `bashcut/scripts/fmt-check.sh` | all |
| `scripts/lint-scripts.sh` | lint + unit tests of every script on this page | `bashcut/scripts/lint-scripts.sh` | macOS, Linux, Git Bash |
| `scripts/python.sh` | the one Python 3 lookup (sourced by `upstream.sh`, `lint-scripts.sh`, `brand/packaging.env`) | `. bashcut/scripts/python.sh; bashcut_python` | all |
| `scripts/smoke.sh` / `smoke.ps1` (+ `isa-allow.txt`) | launch a build, screenshot, CLI + MCP check, AVX scan | `bashcut/scripts/smoke.sh --app target/release/filmcraft` | macOS, Linux / Windows |
| `scripts/bashcutpro-mcp.sh` | MCP command for agents (reference for the kit's copy) | `claude mcp add bashcutpro -- bashcut/scripts/bashcutpro-mcp.sh` | macOS, Linux |
| `packaging/generate.py` (+ `test_generate.py`) | writes the packaging copy with the Pro pieces | run by `packaging/*/package.sh` and `package.ps1`; by hand: `python3 bashcut/packaging/generate.py --out DIR` | all |
| `packaging/package.ps1` | Windows hook target: generate, then run the copy's `package.ps1` | via `packaging/windows/package.ps1` | Windows |
| `packaging/share/bashcut-setup.sh` / `.ps1` | optional first-run setup, shipped in the packages | `bashcut-setup.sh --help` | macOS, Linux, FreeBSD / Windows |
| `brand/packaging.env` | brand variables + re-exec into the packaging copy (sourced by upstream `packaging/env.sh`) | sourced | macOS, Linux, FreeBSD |
| `brand/generate.py` | writes the brand SVGs (needs `fontTools`, an authoring tool: outputs are committed) | `python3 bashcut/brand/generate.py` | all |
| `brand/render.sh` | renders PNG / .icns from the SVGs (needs resvg or rsvg-convert) | `bashcut/brand/render.sh` | macOS, Linux |
| `apps/bashcutpro-cli/build.rs` | `bashcutpro` source from upstream's CLI with our name (SB2) | `cargo build -p bashcutpro-cli` | all |
| `xtask/src/bashcut.rs` | layer table of our crates for `cargo xtask layers` | `cargo xtask layers` | all |

## Every environment variable

Configuration is environment variables with defaults, nothing else: no script reads a `.env` file, and no script
holds a secret. Keep it that way.

| Variable | Read by | Default | Set by |
|---|---|---|---|
| `BASHCUT_PYTHON` | `scripts/python.sh` (so `upstream.sh`, `lint-scripts.sh`, `packaging.env`) | first of `python3`, `py -3`, `python` that is Python 3 | you, when the lookup picks the wrong one |
| `BRAND_NAME`, `BRAND_SLUG`, `BRAND_ICON_DIR` | upstream `packaging/*` (via `packaging.env`); `BRAND_ICON_DIR` also `packaging/generate.py` | `BashCutPro`, `bashcutpro`, `bashcut/brand/app-icon` | `brand/packaging.env` |
| `BASHCUT_PACKAGING_PLAIN` | `packaging.env`, `packaging/windows/package.ps1` hook | unset: package with the Pro pieces | you: `1` packages upstream's files as they are |
| `BASHCUT_BRANDED`, `BASHCUT_ROOT`, `BASHCUT_PACKAGING` | the generated packaging copy, `packaging.env`, the Windows hook | unset | `packaging.env` / `packaging/package.ps1` before re-exec (internal) |
| `CARGO_TARGET_DIR` | `packaging.env`, `package.ps1`, `bashcutpro-mcp.sh` | `<repo>/target` | you / cargo |
| `FILMCRAFT_DATA_DIR`, `XDG_DATA_HOME`, `APPDATA` | `bashcut-setup.sh` / `.ps1` (data folder) | the OS data folder | you / the OS |

## Rules

- **bash:** `#!/usr/bin/env bash`, `set -euo pipefail`, a header comment with usage, and `-h` / `--help` (prints
  the header). Quote variables; shellcheck-clean (`# shellcheck disable=SCxxxx # reason` when a rule is wrong). Two
  deliberate exceptions use `set -u` only, so every step runs and reports: `smoke.sh` (counts FAILs, exits non-zero
  on any) and `bashcut-setup.sh` (optional steps; a failed one does not stop the next). Sourced files
  (`packaging.env`, `python.sh`) start with `# shellcheck shell=bash` and set no shell options.
- **Python:** 3.9+, standard library only (exception: `brand/generate.py` needs `fontTools`), `argparse`,
  `main(argv=None)` + `if __name__ == "__main__"`, exits non-zero on failure, works with `py -3` on Windows. Tests
  are `unittest` files next to the script.
- **PowerShell:** comment-based help at the top, `param(...)`, `Set-StrictMode -Version Latest` and
  `$ErrorActionPreference = 'Stop'`. Exceptions until checked with `pwsh` on Windows: `smoke.ps1` (keeps
  `Continue`, counts FAILs like `smoke.sh`) and `bashcut-setup.ps1`.
- **Generators** are deterministic (same input, byte-identical output; `test_generate.py` runs it twice), fail loudly
  when an anchor they substitute is missing or matches an unexpected number of times, and never edit upstream files
  in place.

## Lint

`bashcut/scripts/lint-scripts.sh`: `bash -n` + shellcheck on every `.sh` and `packaging.env`; `py_compile` + ruff
(`--select E9,F`, py39) on every `.py`; a PowerShell parse check when `pwsh` exists; then `test_upstream.py` and
`test_generate.py`. Missing shellcheck / ruff / pwsh print `SKIP` (`brew install shellcheck ruff`).
