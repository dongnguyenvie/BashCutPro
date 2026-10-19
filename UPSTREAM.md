# Upstream

BashCutPro is based on **FilmCraft**.

- Source: https://github.com/storytold/filmcraft
- License: dual MIT / Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` and `ATTRIBUTION.md` kept as-is)
- Remote: `upstream` (fetch only; push disabled): `bashcut/scripts/upstream.sh setup`

We only add to upstream's code (rules: [`bashcut/extending-upstream.md`](bashcut/extending-upstream.md)). Everything we
carry in upstream files is listed below, and `bashcut/scripts/upstream.sh footprint` fails on anything that is not.

## Sync log

Newest first. Add a row after every merge from upstream (`upstream.sh merge` prints it).

| Date (sync) | Upstream commit | Upstream commit date | Note |
|---|---|---|---|
| 2026-10-20 | `105c09bd80e2c86ec7464a2c59a2501eb89ffc5c` | 2026-10-19 | Initial import (latest tag v0.8.0), PR #2452 merge |

## Hooks

One row per hook: an inserted block in an upstream file that calls into our code. `First line` is the hook's first
inserted line exactly as in the file (whitespace trimmed; escape `|` as `\|`) and must carry the `BashCutPro` marker;
`Lines` is how many inserted lines the hook spans (1 unless it cannot be one line). Add the row in the PR that adds
the hook; the footprint check reports any inserted upstream line that no row covers, and any row it cannot find.

| File | Lines | First line | Why | Design |
|---|---|---|---|---|
| `crates/ui-egui/src/lib.rs` | 1 | `pub mod bashcut_brand; // BashCutPro: branded strings and links (bashcut/extending-upstream.md)` | brand lookup module | [rebrand.md](bashcut/rebrand.md) |
| `crates/ui-egui/src/i18n/mod.rs` | 4 | `#[cfg(not(test))] // BashCutPro (bashcut/rebrand.md); upstream's catalog tests see upstream text` | `Language::tr` asks `bashcut_brand::text` first | [rebrand.md](bashcut/rebrand.md) |

## Carried edits

Upstream files we change beyond hooks. `additions`: only inserted lines (no hook row needed), `edits`: any change,
re-applied by hand when a merge conflicts. Each `edits` row is an exception to the rules and should shrink.

| Files | Kind | Why | Until |
|---|---|---|---|
| `.github/workflows/*.yml` | edits | workflows run manually only (`on:` blocks), `app-title` in `release.yml` | owner decision 2026-10-20: keep, re-apply on merge |
| `apps/filmcraft/build.rs`, `apps/filmcraft/src/main.rs`, `crates/ui-egui/src/brand.rs`, `crates/ui-egui/src/header.rs`, `crates/ui-egui/src/links.rs`, `crates/ui-egui/src/panels/dialogs.rs` | edits | branding in code ([rebrand.md](bashcut/rebrand.md)) | #39 |
| `packaging/**`, `omastore.toml` | edits | branding in packaging ([rebrand.md](bashcut/rebrand.md)) | #39 |
| `NOTICE`, `LICENSE-MIT`, `ATTRIBUTION.md`, `.gitignore`, `AGENTS.md`, `CLAUDE.md` | additions | our copyright lines, asset rows, `.local/`, the pointer to `bashcut/README.md` | keep |

## Footprint check

```bash
bashcut/scripts/upstream.sh footprint --pr   # before every PR: this branch's changes only
bashcut/scripts/upstream.sh footprint        # all we carry since the last upstream merge (-v: every hunk)
```

It diffs upstream files (our folders and new files left out: `bashcut/`, `crates/bashcut-*`, `apps/bashcut-*`,
`*/bashcut/`, `bashcut_*`, this file), prints lines per file and each file's upstream commits in the last 30 days
(`HOT` at 20 or more: any edit there will conflict), and fails when a hunk is not allowed by the two tables above, a
hook row is stale or has no marker, or the Rust lines changed in upstream files exceed 150. Its tests:
`python3 bashcut/scripts/test_upstream.py`.

## How to sync

On upstream tags (preferred) or every 1–2 weeks:

1. `bashcut/scripts/upstream.sh merge [tag]`: turns on `git rerere`, merges the newest upstream tag (or the one given)
   on `upstream-sync/<date>`, takes upstream's `Cargo.lock`, then runs `cargo check --workspace`, `cargo test` for
   every `bashcut-*` package and `cargo xtask layers` (no release build). rerere replays resolutions recorded at
   earlier syncs.
2. Conflicts should only be in hook lines or carried edits; re-place a hook if upstream moved the function (search for
   `// BashCutPro`), resolve, `git add`, `git commit` (rerere records it), and re-run with `--no-fetch` for the checks.
   For a workflow `on:` block keep `workflow_dispatch` only.
3. `cargo test -p filmcraft-ui-egui --test bashcut_ui` once that test exists, and re-check `filmcraft-cli` flags our
   tools use (its help changes often).
4. Add the sync-log row above, update the hook rows if a hook moved, refresh the churn numbers in
   `bashcut/extending-upstream.md` §3, and open the PR.
