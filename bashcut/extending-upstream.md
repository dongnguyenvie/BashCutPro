# Extending FilmCraft: add first, modify when it is worth it

Read this before touching any file that is not ours. BashCutPro pulls upstream FilmCraft one way from time to time
([decisions.md](decisions.md), 2026-10-20). Upstream is stable and moves fast (about 1,700 commits in the two weeks
before 2026-10-20), so the cheapest fork is one that mostly adds to upstream's code. Editing upstream is allowed
when it is the simpler route and stays under our control: recorded, small, and cheap to re-apply at the next sync
(owner, 2026-10-20: "no need to be that strict, as long as it stays within our control").

## 1. The rule

1. **Add first; modify when it is worth it.** Prefer an addition (our files, a hook, a wrapper). Edit, delete or
   reorder upstream lines when the addition would be clearly worse (more code, a fragile wrapper, a worse UX), and
   the edit stays under control: small, in a file upstream rarely touches (§3: cold, ≲ 20 commits in 30 days), one
   row in `UPSTREAM.md` "Carried edits" (kind `edits`, or `deletes` for a removed file; why, until), and re-applied by merge + `rerere` at each sync.
   Hot files (§3) and high-churn structs (`UiState`, `FilmcraftApp`, `Session`, `Project`) still go through hooks
   and our own files: an edit there conflicts on nearly every sync.
2. **Our code goes in our files**: `bashcut-*` crates, `apps/bashcut-*`, `crates/<crate>/src/bashcut/` or
   `bashcut_*.rs` modules, `bashcut/` docs, new test files.
3. **A hook is one inserted line** in an upstream file that calls into our code. It never replaces a line. It sits
   next to a neighbour upstream rarely touches, and carries the marker
   `// BashCutPro: <why> (bashcut/extending-upstream.md)`. Every hook is listed in `UPSTREAM.md`.
4. **When upstream lacks something, add a new thing beside it.** Need different behaviour from an upstream
   command? Add a `bashcut.*` command that calls the original and does the rest. Need a method on an upstream type?
   Add it in our own file (§2). Never change the original.
5. **Depend on the surfaces that do not move** (§4). The process boundary (`filmcraft-cli`, MCP, command ids) is the
   most stable, Rust types the least.

### Carried edits in place today

These modify upstream lines (all listed in `UPSTREAM.md` "Carried edits", with the reason):

| What | Files | Status |
|---|---|---|
| Workflows manual only | `.github/workflows/*.yml` (`on:` blocks) | owner decision (2026-10-20): keep, re-apply on merge |
| Branding in code | `crates/ui-egui/src/{brand,links}.rs`, `apps/filmcraft/build.rs` | kept: cold files (≤ 4 upstream commits in 30 days) a hook can't cover; the hot ones (`main.rs`, `header.rs`, `dialogs.rs`) carry one hook each since #39 |
| Branding in packaging | `packaging/**`, `omastore.toml` | kept: cold, run as-is by upstream's shared release workflow; a packaging-time generator is sketched in #39 |
| Additions only (fine) | `NOTICE`, `LICENSE-MIT`, `ATTRIBUTION.md`, `.gitignore`, one pointer line in `AGENTS.md` / `CLAUDE.md`, `i18n/mod.rs` hook, `ui-egui/src/lib.rs` mod line | keep |

## 2. Extension points (how to add without modifying)

| Need | Do this | Never |
|---|---|---|
| A new engine command | `crates/engine/src/bashcut/mod.rs` → `commands() -> Vec<CommandSpec>`, ids `bashcut.*`; one inserted line `v.extend(crate::bashcut::commands());` right after `v.extend(crate::captions::commands());` in `build()` (the old head of the list; upstream appends at the tail) | register an upstream id again, change an upstream command's params or result |
| Different behaviour of an upstream command | a `bashcut.*` wrapper: look it up with `commands::find(id)`, call its `run`, then add what we need | patch the original `run` |
| A method on `Session`, `Project`, … | inside the defining crate: an `impl Session { … }` block in our own file (`crates/engine/src/bashcut/`); from another crate: an extension trait (`trait SessionExt { … } impl SessionExt for Session`) | add fields or change existing methods |
| State of our own | our own struct: engine side in our module, UI side in egui context data (`ctx.data_mut`), saved to `<data dir>/bashcut/` | new fields in `Session`, `FilmcraftApp`, `UiState` (they change all the time, §3) |
| Grouped undo, snapshots | `History` is public and stores `Arc<Project>` snapshots: our module can record, fold or restore steps itself | a hook into history code |
| UI | `bashcut_ui::layout` (one inserted line in `ui()`) for the rail and side panel; `egui::Area` / `egui::Window` for overlays; upstream `Tokens` and widgets read-only | a new `PanelKind`, drawing inside upstream panels, edits to `dock.rs` / `theme.rs` / `icons.rs` |
| UI commands and menus | `bashcut_ui::commands()` / `menu_items()` appended by inserted lines in `menus.rs` (`external_commands`, `menu_items_for`); ids `bashcut.ui.*`, `bashcut.plugins.*` | new entries in `UI_COMMANDS` |
| Strings | `bashcut_ui/strings/<lang>.tsv` through the existing `bashcut_brand::text` lookup in `i18n/mod.rs` | edits to upstream `.tsv` catalogs (very hot, §3) |
| Data shown in upstream panels | markers, labels, clip names, subclips, project notes, written with existing commands | new fields in the project schema |
| Per-project data of our own | sidecar folder `<name>.fcproj.bashcut/` | changing `.fcproj` |
| Looks, presets, templates | files in the data dir: export presets, effect presets, LUTs, Lumetri presets, `.fcgt` | built-ins added to upstream's preset tables |
| A new crate or app | `crates/bashcut-*` or `apps/bashcut-*` (joined by the `crates/*` / `apps/*` globs); path dependencies (`{ path = "../engine" }`) | entries in the root `Cargo.toml` |
| Layer check for a new crate | `xtask/src/bashcut.rs` + one inserted first line in `layer_of`: `if let Some(l) = bashcut::layer_of(name) { return Some(l); }` | editing the `LAYERS` table |
| Tests | new files: `crates/<x>/tests/bashcut_*.rs`, `crates/engine/src/bashcut/tests.rs` | edits to upstream tests |
| Docs | `bashcut/` only | edits to `ROADMAP.md`, `docs/**`, `README.md` (hot, §3) |

## 3. What upstream changes most

Upstream commits touching each file in the 30 days to 2026-10-22 (`git log --since=2026-09-22 --no-merges upstream/main`).
Re-counted at the 2026-10-22 sync (upstream `dd525c93`, v0.10.0 + 20 commits): within ±5 of the 2026-10-20 count.
Treat anything above ~20 as "will conflict with any edit".

| Changes | File | What it means for us |
|---|---|---|
| 161 | `crates/engine/src/commands.rs` | the command table; only the one inserted `extend` line (§2) |
| 133 | `crates/ui-egui/src/lib.rs` | app shell and frame loop; only the mod line and the layout line |
| 130 | `Cargo.lock` | on merge take upstream's, run `cargo check` |
| 103 | `crates/ui-egui/src/panels/timeline.rs` | never touch; show our data as markers / labels |
| 97 | `crates/engine/src/lib.rs` | `Session`, `History`; only the `pub mod bashcut;` line |
| 75 | `crates/ui-egui/src/menus.rs` | three inserted lines (invoke routing, commands, menu items) |
| 60 | `Cargo.toml` (root) | never touch (globs and path deps) |
| 56 | `crates/ui-egui/src/panels/mod.rs` | never touch |
| 55 | `crates/ui-egui/src/state.rs` | `UiState` (63 struct changes since 09-01); never add fields |
| 53 / 39 / 36 | `crates/ui-egui/src/i18n/{es,ja,zh-cn}.tsv` (all catalogs) | never touch; our strings in our own catalog |
| 51 | `crates/export/src/lib.rs` | use export presets and `filmcraft-cli export`, not its types |
| 47 | `apps/filmcraft/src/main.rs` | one hook (app icon); nothing new |
| 43 | `crates/project/src/lib.rs` | project schema; never add fields; build projects with commands |
| 41 | `crates/ui-egui/src/panels/effect_controls.rs` | never touch |
| 40 | `xtask/src/main.rs` | one inserted line in `layer_of` |
| 40 | `crates/render/src/lib.rs` | never touch; word-caption styles (#49) try the no-hook route first |
| 37 / 37 | `panels/monitor.rs`, `panels/dialogs.rs` | never touch (dialogs: one hook, our About tab) |
| 36 | `crates/edit/src/lib.rs` | call through commands, not edit functions, where possible |
| 31 | `crates/engine/src/settings.rs` | our settings in `<data dir>/bashcut/` |
| 29 / 32 | `crates/ui-egui/src/control.rs`, `apps/filmcraft-cli/src/main.rs` | use the control protocol and CLI as documented; the CLI help changed 12× since 09-01, so re-check flags after a sync |
| 74 / 62 / 75 | `docs/architecture.md`, `README.md`, `ROADMAP.md` | never touch |

Struct and API churn since 2026-09-01 (changes to the definition):

| Surface | Changes | Use it? |
|---|---|---|
| `UiState` | 63 | read only, never extend |
| `FilmcraftApp` | 46 | read only, never extend |
| `Session` | 29 | through methods and commands; extension `impl` in our files |
| `filmcraft-cli` help (flags) | 12 | yes, re-check after sync |
| `Project` | 8 | read through commands / inspect JSON |
| `PanelKind`, `menus::invoke` head | 4 | do not extend `PanelKind` |
| `Tokens`, `History`, automation `Element` | 2–3 | yes |
| `CommandSpec`, automation `Registry`, MCP `command_run` | 1–2 | yes: the most stable Rust surfaces |

Refresh these numbers at each upstream sync (`bashcut/scripts/upstream.sh footprint` prints each changed file's
30-day count; `git log --since=<30 days> --format= --name-only upstream/main | sort | uniq -c | sort -rn` for the rest)
and update this section.

## 4. Stability ladder

Prefer the highest rung that does the job:

1. **Process boundary**: `filmcraft-cli` (`exec`, `run`, `export`, `render`, `mcp`), MCP tools, the control channel,
   command ids and their JSON params. Batch runner, plugins and agents live here.
2. **Command registry**: `CommandSpec`, `commands::find`, `command_specs()` (1 change since 09-01).
3. **Public value types**: `History`, `Tokens`, automation registry.
4. **Engine internals**: `Session` methods, `Project` fields, edit functions. Wrap each use in one place in our
   module so a rename is fixed once.
5. **UI internals**: `FilmcraftApp`, `UiState`, panels. Avoid.

## 5. Checklist for a change that touches upstream

- [ ] Is there an extension point in §2? Prefer it. If an edit is simpler and the file is cold, edit it and add a
      `UPSTREAM.md` Carried edits row.
- [ ] If a hook is needed: one inserted line, no edited line, stable neighbour, marker comment, `UPSTREAM.md` row.
- [ ] No new field or variant in the high-churn structs (`UiState`, `FilmcraftApp`, `Session`, `Project`); upstream
      tests are not edited (add ours beside them).
- [ ] Our tests are in our own files and pass: `cargo test -p <our crates>`, `cargo xtask layers`.
- [ ] `bashcut/scripts/upstream.sh footprint --pr` passes: every inserted upstream line is covered by a `UPSTREAM.md`
      Hooks row, no upstream line is changed (except the Carried edits rows, §1).
