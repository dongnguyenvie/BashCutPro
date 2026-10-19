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
| 2026-10-20 | `105c09bd80e2c86ec7464a2c59a2501eb89ffc5c` | 2026-10-19 | Check with `upstream.sh merge` (#63): already up to date, nothing merged (see Sync records) |
| 2026-10-20 | `105c09bd80e2c86ec7464a2c59a2501eb89ffc5c` | 2026-10-19 | Initial import (latest tag v0.8.0), PR #2452 merge |

### Sync records

One entry per sync: what came in, what conflicted, what it cost. Newest first.

**2026-10-20, first run of the tooling (#63).** `git fetch upstream --tags`; `upstream.sh merge --no-check` on
`upstream-sync/2026-10-20`: the newest tag `v0.9.0` (`4a69a038`, 2026-10-17) is already inside our base and
`upstream/main` is still `105c09bd`, so: **0 commits pulled, no conflicts, ~2 s**. rerere is on for this clone. No
`cargo check` was needed (nothing changed; builds are off on worker machines this week). API breaks in our adapters:
none. The footprint after the run: 38 upstream files, Rust lines 46 / 150, 16 hook rows all matched. The tool printed
a sync-log row for a merge that did not happen; fixed in the same PR (it now says "already up to date"). Next: repeat
on the next upstream tag or in 1–2 weeks, whichever comes first.

Same day, Windows (geekomT) re-run: still 0 commits; `upstream.py` crashed on Windows decoding git output with the
ANSI code page (`UnicodeDecodeError` in the footprint diff). It now reads git output as UTF-8; after the fix
`merge --no-check --no-fetch` and the footprint pass (38 files, Rust 46 / 150, ledger OK) and `test_upstream.py` passes (18 tests).

## Hooks

One row per hook: an inserted block in an upstream file that calls into our code. `First line` is the hook's first
inserted line exactly as in the file (whitespace trimmed; escape `|` as `\|`) and must carry the `BashCutPro` marker;
`Lines` is how many inserted lines the hook spans (1 unless it cannot be one line). Add the row in the PR that adds
the hook; the footprint check reports any inserted upstream line that no row covers, and any row it cannot find.

| File | Lines | First line | Why | Design |
|---|---|---|---|---|
| `crates/ui-egui/src/lib.rs` | 1 | `pub mod bashcut_brand; // BashCutPro: branded strings and links (bashcut/extending-upstream.md)` | brand lookup module | [rebrand.md](bashcut/rebrand.md) |
| `crates/ui-egui/src/panels/dialogs.rs` | 1 | `let false = crate::bashcut_brand::about_tab(app, ui) else { return }; // BashCutPro: our About tab, no ArtCraft marks (bashcut/extending-upstream.md)` | About ▸ About drawn by `bashcut_brand::about_tab` | [rebrand.md](bashcut/rebrand.md) |
| `crates/ui-egui/src/header.rs` | 1 | `let label = crate::bashcut_brand::community_button(label); // BashCutPro: our community button (bashcut/extending-upstream.md)` | header button label | [rebrand.md](bashcut/rebrand.md) |
| `apps/filmcraft/src/main.rs` | 1 | `.with_icon(filmcraft_ui_egui::bashcut_brand::app_icon()) // BashCutPro: our app icon (bashcut/extending-upstream.md)` | window / Dock icon, set after upstream's | [rebrand.md](bashcut/rebrand.md) |
| `crates/ui-egui/src/i18n/mod.rs` | 4 | `#[cfg(not(test))] // BashCutPro (bashcut/rebrand.md); upstream's catalog tests see upstream text` | `Language::tr` asks `bashcut_brand::text` first | [rebrand.md](bashcut/rebrand.md) |
| `crates/ui-egui/src/lib.rs` | 1 | `pub mod bashcut_ui; // BashCutPro: rail and side panel (bashcut/extending-upstream.md)` | in-app UI module (H5) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/src/lib.rs` | 1 | `let body = bashcut_ui::layout(self, ui, body); // BashCutPro: rail and side panel take the right edge (bashcut/extending-upstream.md)` | rail + side panel reserve the right edge of the body (H6) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/src/menus.rs` | 1 | `crate::bashcut_ui::route!(app, ctx, id, &params); // BashCutPro: our bashcut.* UI commands (bashcut/extending-upstream.md)` | first line of `invoke`: our command ids (H7; a macro so it stays one line) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/src/menus.rs` | 1 | `out.extend(crate::bashcut_ui::menu_items(session)); // BashCutPro: Window ▸ BashCutPro (bashcut/extending-upstream.md)` | our menu items (H9) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/src/menus.rs` | 1 | `v.extend(crate::bashcut_ui::commands()); // BashCutPro: our UI commands (bashcut/extending-upstream.md)` | our commands in `external_commands` (H8) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/Cargo.toml` | 1 | `bashcut-plugin-host = { path = "../bashcut-plugin-host" } # BashCutPro: plugin host for the Plugins and Runs sections (bashcut/extending-upstream.md)` | ui-egui depends on our plugin host crate (H4) | [plugin-host.md](bashcut/plugin-host.md) |
| `crates/ui-egui/Cargo.toml` | 1 | `bashcut-agent = { path = "../bashcut-agent" } # BashCutPro: agent sessions for the Agent section (bashcut/extending-upstream.md)` | ui-egui depends on our agent crate (H4) | [in-app-ui.md](bashcut/in-app-ui.md) §6 |
| `xtask/src/main.rs` | 1 | `mod bashcut; // BashCutPro: layers of our crates (bashcut/extending-upstream.md)` | layer table of our crates (H11) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `xtask/src/main.rs` | 1 | `bashcut::our_layer_first!(name); // BashCutPro: our crates first (bashcut/extending-upstream.md)` | first line of `layer_of` (H11; macro) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/engine/src/lib.rs` | 1 | `pub mod bashcut; // BashCutPro: our engine commands (bashcut/extending-upstream.md)` | `bashcut.*` engine module | [in-app-ui.md](bashcut/in-app-ui.md) §3 H2 |
| `crates/engine/src/commands.rs` | 1 | `v.extend(crate::bashcut::commands()); // BashCutPro: bashcut.* commands (bashcut/extending-upstream.md)` | registers `bashcut.*` commands after `captions` | [in-app-ui.md](bashcut/in-app-ui.md) §3 H3 |
| `crates/engine/Cargo.toml` | 1 | `bashcut-analysis = { path = "../bashcut-analysis" } # BashCutPro: bashcut.* measurements (bashcut/extending-upstream.md)` | the engine uses our measurement crate | [in-app-ui.md](bashcut/in-app-ui.md) §3 H1 |

## Carried edits

Upstream files we change beyond hooks. `additions`: only inserted lines (no hook row needed), `edits`: any change,
re-applied by hand when a merge conflicts. Each `edits` row is an exception to the rules and should shrink.

| Files | Kind | Why | Until |
|---|---|---|---|
| `.github/workflows/*.yml` | edits | workflows run manually only (`on:` blocks), `app-title` in `release.yml` | owner decision 2026-10-20: keep, re-apply on merge |
| `crates/ui-egui/src/brand.rs` | edits | wordmark bytes: a hook would leave ArtCraft's PNGs compiled into our binary (2 lines; 1 upstream commit in 30 days) | keep while cold |
| `crates/ui-egui/src/links.rs` | edits | our URLs: the constants also feed menu hints, the status line and command results, so one mapping hook can't cover them (5 lines + the scheme test; 4 commits in 30 days) | keep while cold |
| `apps/filmcraft/build.rs` | edits | Windows exe icon and version info (5 lines; 1 commit in 30 days); `winresource` appends icons, so a second `set_icon` hook would not replace upstream's | keep while cold |
| `packaging/**`, `omastore.toml` | edits | branding in packaging ([rebrand.md](bashcut/rebrand.md)); upstream's shared release workflow runs these scripts directly (no pre-step), all cold (≤ 5 commits in 30 days) | generator: #39 comment |
| `NOTICE`, `LICENSE-MIT`, `ATTRIBUTION.md`, `.gitignore`, `AGENTS.md`, `CLAUDE.md` | additions | our copyright lines, asset rows, `.local/`, the pointer to `bashcut/README.md` | keep |
| `Cargo.lock` | additions | lock entries of our `bashcut-*` crates | keep: on merge take upstream's file, `cargo check` re-adds ours |

## Footprint check

```bash
bashcut/scripts/upstream.sh footprint --pr   # before every PR: the upstream files this branch touches
bashcut/scripts/upstream.sh footprint        # all we carry since the last upstream merge (-v: every hunk)
```

It diffs upstream files (our files and new files left out: any path part named `bashcut`, `bashcut_*`, `bashcut-*`
or `bashcut.*`, and this file; `Cargo.lock` is listed as generated, not checked), prints lines per file and each file's upstream commits in the last 30 days
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
