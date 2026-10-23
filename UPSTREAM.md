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
| 2026-10-22 | `dd525c93d43b96843aedc50d7fa94a052474f476` | 2026-10-22 | Merge upstream/main (v0.10.0 + 20 commits, 41 commits in all): no conflicts; `sonarcloud.yml` made manual-only (see Sync records) |
| 2026-10-20 | `105c09bd80e2c86ec7464a2c59a2501eb89ffc5c` | 2026-10-19 | Check with `upstream.sh merge` (#63): already up to date, nothing merged (see Sync records) |
| 2026-10-20 | `105c09bd80e2c86ec7464a2c59a2501eb89ffc5c` | 2026-10-19 | Initial import (latest tag v0.8.0), PR #2452 merge |

### Sync records

One entry per sync: what came in, what conflicted, what it cost. Newest first.

**2026-10-22, upstream/main `dd525c93` (v0.10.0 + 20 commits).** `upstream.py merge dd525c93 --no-check --no-fetch
--branch upstream-sync/2026-10-22` (upstream/main, not the tag: it was 20 commits past `v0.10.0`). **41 commits, 56
files (+2496 / −166), no conflicts**: rerere had nothing to replay and git merged every hook and carried edit cleanly.
What came in: hostile-input hardening and mutation-fuzz tests for the video decoders (AV1, MPEG-2, H.264, HEVC, VP8,
VP9, ProRes, DNx, APV); MP4 damaged-audio allocation fix; AAF/OMF embedded audio never overwrites an existing file
(new `Services::write_file_new`, default fails closed); auto-save rotation ordered by real time; playback reopens a
lost audio output every 2 s (`AUDIO_RETRY_S`); macOS Cmd-Q asks to save, and `app.quit` works in control-only mode;
`filmcraft-cli exec` / `run` wait for `file.exportMedia` unless `"wait": false` (#2342); MCP screenshot uses async
I/O; release 0.10.0 (workspace version, so our crates are 0.10.0 too); docs drop competitor names
(`filmcraft-lumetri.png` → `filmcraft-color-panel.png`); CI: shared nix workflow, new `sonarcloud.yml`,
`.coderabbit.yaml`, `sonar-project.properties`.
Fixes on top of the merge: `sonarcloud.yml` `on:` set to `workflow_dispatch` only (carried edit; it ran on push and
PR); `Cargo.lock` re-adds our crates at 0.10.0. API breaks in our adapters: none (`cargo check --workspace` green with
no change to our code); none of our code implements `Services`, and the batch runner already passes `"wait": true`.
No upstream change to panels, dock, theme, `UiState` or the `ui()` frame, so the rail and side panel layout is
unaffected. CLI flags our tools use: unchanged (only the `run` help text grew). `.coderabbit.yaml` and
`sonar-project.properties` are inert here (no CodeRabbit / SonarCloud on our repo). Churn numbers in
`bashcut/extending-upstream.md` §3 refreshed (`--no-merges`, the method the table used). Time: about 40 min, most of
it `cargo check` / tests on a warm `target/`.
Shortcuts note (SC1, 2026-10-23): agent-kit's `commands.json` refresh (AK1) showed `markers.addRange` (⌃⇧M),
`trim.toggleType` (⌃T) and `multicam.cutToCamera9` (⌃9) without a default key. Not this sync and not ours: upstream's
`drop_control_collisions` (`crates/engine/src/shortcuts.rs`, since v0.5.0, `0b34c076`) drops a macOS-only ⌃ chord
off macOS when a ⌘ chord lands on the same Ctrl key there (⌘⇧M `markers.goPrev`, ⌘T `graphics.newText`, ⌘9
`timeline.toggleAllAudioTargets`; upstream test `default_keymaps_have_no_conflicts_on_any_platform`). The earlier
catalogue came from a macOS build, AK1's from Windows. No bashcut binding takes those keys (ours: ⌘⇧P palette,
⌘⌥⇧A Focus Agent). No change; the catalogue's provenance now names the build OS.

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
| `apps/filmcraft/src/main.rs` | 1 | `.with_title(filmcraft_ui_egui::bashcut_brand::NAME) // BashCutPro: window title (bashcut/extending-upstream.md)` | native window title, set after upstream's (#90) | [rebrand.md](bashcut/rebrand.md) |
| `apps/filmcraft/src/main.rs` | 1 | `options.viewport.app_id = Some(filmcraft_ui_egui::bashcut_brand::APP_ID.to_owned()); // BashCutPro: our Wayland app id / X11 class (bashcut/extending-upstream.md)` | window app id `com.birchapps.bashcutpro`, set after upstream's `with_app_id` so it matches our Linux desktop file (SB5) | [packaging/README.md](bashcut/packaging/README.md) |
| `apps/filmcraft/src/main.rs` | 1 | `let control_port = filmcraft_ui_egui::bashcut_ui::control::launch_port(control_port, data_dir.clone().or_else(default_data_dir).as_deref()); // BashCutPro: control channel on by default (bashcut/extending-upstream.md)` | control port when no `--control` is given: a free loopback port unless turned off (CC1) | [decisions.md](bashcut/decisions.md) |
| `apps/filmcraft/src/main.rs` | 1 | `mod bashcut_cli; // BashCutPro: --version, --help and errors name bashcutpro (bashcut/extending-upstream.md)` | the app's command-line output module (BR1) | [rebrand.md](bashcut/rebrand.md) |
| `apps/filmcraft/src/main.rs` | 1 | `bashcut_cli::tell_first!(text, error); // BashCutPro: bashcutpro, not filmcraft (bashcut/extending-upstream.md)` | first line of `tell`: `bashcutpro 0.10.0`, `Usage: bashcutpro …`, `bashcutpro: <error>`, Windows message box titled BashCutPro (BR1; macro so it stays one line) | [rebrand.md](bashcut/rebrand.md) |
| `crates/ui-egui/src/i18n/mod.rs` | 4 | `#[cfg(not(test))] // BashCutPro (bashcut/rebrand.md); upstream's catalog tests see upstream text` | `Language::tr` asks `bashcut_brand::text` first | [rebrand.md](bashcut/rebrand.md) |
| `crates/ui-egui/src/i18n/mod.rs` | 1 | `crate::bashcut_ui::catalog_has_ours!(text); // BashCutPro: in tests our labels count as covered, they live in our own catalog (bashcut/extending-upstream.md)` | first line of `Language::has`; tests only: upstream's catalog tests skip names only our `bashcut.*` commands use (FX15; macro so it stays one line) | [decisions.md](bashcut/decisions.md) |
| `crates/ui-egui/src/lib.rs` | 1 | `pub mod bashcut_ui; // BashCutPro: rail and side panel (bashcut/extending-upstream.md)` | in-app UI module (H5) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/src/lib.rs` | 1 | `let body = bashcut_ui::layout(self, ui, body); // BashCutPro: rail and side panel take the right edge (bashcut/extending-upstream.md)` | rail + side panel reserve the right edge of the body (H6) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/src/lib.rs` | 1 | `bashcut_ui::logic(self, ctx); // BashCutPro: automation.json follows the open project, also while the window is hidden (bashcut/extending-upstream.md)` | `automation.json` is published from `logic()`, which runs while an occluded window skips `ui` (H13, #191) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/src/lib.rs` | 1 | `bashcut_ui::batch::take_drops(self, &ctx); // BashCutPro: files dropped on the Batch section become batch inputs (bashcut/extending-upstream.md)` | a drop on the Batch section is not also imported into the open project (H12) | [in-app-ui.md](bashcut/in-app-ui.md) §6a |
| `crates/ui-egui/src/control.rs` | 1 | `crate::bashcut_ui::modal::guard!(app, ctx, req, pos); // BashCutPro: a click an open modal would swallow fails and names the modal (bashcut/extending-upstream.md)` | `ui.click` behind an open egui `Modal` (Recover Unsaved Changes, Revert, Preferences) answers an error naming it and its ids instead of `ok: true` (FX14; macro so it stays one line) | [in-app-ui.md](bashcut/in-app-ui.md) §4 |
| `crates/ui-egui/src/control.rs` | 1 | `crate::bashcut_brand::engine_commands!(app, req); // BashCutPro: engine.commands labels name BashCutPro, ids unchanged (bashcut/extending-upstream.md)` | first line of `handle` after `s`: `engine.commands` of the control channel says "Selection as BashCutPro Project…" and `bashcutpro-cli` in params, as MCP and the CLI already do; upstream's arm is no longer reached (BR6; macro so it stays one line) | [rebrand.md](bashcut/rebrand.md) |
| `crates/ui-egui/src/control.rs` | 1 | `crate::bashcut_ui::control::restore(ctx); // BashCutPro: ui.focus {} also restores a minimised window (bashcut/extending-upstream.md)` | before upstream's `ViewportCommand::Focus` in `ui.focus {}`: `Minimized(false)`, since Focus alone leaves a minimised Windows window minimised and the FX17 "call ui.focus {}" error would not help | [decisions.md](bashcut/decisions.md) |
| `crates/ui-egui/src/menus.rs` | 1 | `crate::bashcut_ui::route!(app, ctx, id, &params); // BashCutPro: our bashcut.* UI commands (bashcut/extending-upstream.md)` | first line of `invoke`: our command ids (H7; a macro so it stays one line) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/src/menus.rs` | 1 | `out.extend(crate::bashcut_ui::menu_items(session)); // BashCutPro: Window ▸ BashCutPro (bashcut/extending-upstream.md)` | our menu items (H9) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/src/menus.rs` | 1 | `v.extend(crate::bashcut_ui::commands()); // BashCutPro: our UI commands (bashcut/extending-upstream.md)` | our commands in `external_commands` (H8) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/Cargo.toml` | 1 | `bashcut-plugin-host = { path = "../bashcut-plugin-host" } # BashCutPro: plugin host for the Plugins and Runs sections (bashcut/extending-upstream.md)` | ui-egui depends on our plugin host crate (H4) | [plugin-host.md](bashcut/plugin-host.md) |
| `crates/ui-egui/Cargo.toml` | 1 | `bashcut-agent = { path = "../bashcut-agent" } # BashCutPro: agent sessions for the Agent section (bashcut/extending-upstream.md)` | ui-egui depends on our agent crate (H4) | [in-app-ui.md](bashcut/in-app-ui.md) §6 |
| `crates/ui-egui/Cargo.toml` | 1 | `bashcut-batch = { path = "../bashcut-batch" } # BashCutPro: batch runner for the Batch section (bashcut/extending-upstream.md)` | ui-egui depends on the batch runner (H4) | [in-app-ui.md](bashcut/in-app-ui.md) §6a |
| `crates/ui-egui/Cargo.toml` | 1 | `bashcut-dirs = { path = "../bashcut-dirs" } # BashCutPro: atomic prefs files and clip for the side panel (bashcut/extending-upstream.md)` | the side panel saves its prefs through our shared atomic write (RU4) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `crates/ui-egui/Cargo.toml` | 1 | `bashcut-brand = { path = "../bashcut-brand" } # BashCutPro: one command-catalog branding rule with bashcutpro-cli (bashcut/extending-upstream.md)` | `bashcut_brand::commands` (control channel `engine.commands`, BR6) calls the rule `bashcutpro-cli`'s MCP / CLI use, so the two can't drift (RC14) | [rebrand.md](bashcut/rebrand.md) |
| `apps/filmcraft/src/main.rs` | 1 | `bashcut_cli::window_failed_first!(e, data_root.as_deref(), started); // BashCutPro: BashCutPro could not start its window (bashcut/extending-upstream.md)` | first line of the window-failure block in `run`: log, crash log and a message box titled BashCutPro ("BashCutPro could not start its window", en + vi), then `run` returns before upstream's FilmCraft dialog (BR3; macro) | [rebrand.md](bashcut/rebrand.md) |
| `apps/filmcraft/src/native_menu.rs` | 1 | `app_menu.set_text(filmcraft_ui_egui::bashcut_brand::NAME); // BashCutPro: app menu title (bashcut/extending-upstream.md)` | macOS app submenu titled BashCutPro, right after upstream's `Submenu::new("FilmCraft", …)` (BR3; macOS-only, compile-checked on macOS only) | [rebrand.md](bashcut/rebrand.md) |
| `apps/filmcraft/src/control_server.rs` | 1 | `crate::bashcut_cli::ui_first!(req, tx, ctx, &rrx); // BashCutPro: ui.* requests fail after 11 s, not 60 s, when the window draws no frames (bashcut/extending-upstream.md)` | first line after `ControlRequest::new` in `serve`: the server thread answers `ui.*` requests (not `ui.focus`, `ui.menu.invoke`) within 11 s with "window not in front: … call ui.focus {}" when the UI thread draws no frame (window minimised or covered; FX17, FX19; macro) | [decisions.md](bashcut/decisions.md) |
| `xtask/src/main.rs` | 1 | `mod bashcut; // BashCutPro: layers of our crates (bashcut/extending-upstream.md)` | layer table of our crates (H11) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `xtask/src/main.rs` | 1 | `bashcut::our_layer_first!(name); // BashCutPro: our crates first (bashcut/extending-upstream.md)` | first line of `layer_of` (H11; macro) | [in-app-ui.md](bashcut/in-app-ui.md) |
| `xtask/src/main.rs` | 1 | `bashcut::dev_first!(task); // BashCutPro: cargo xtask dev builds the app with its engine and runs it (bashcut/extending-upstream.md)` | second line of `main`: `cargo xtask dev` (DV1; macro, returns the app's exit code) | [README.md](bashcut/README.md) |
| `crates/engine/src/lib.rs` | 1 | `pub mod bashcut; // BashCutPro: our engine commands (bashcut/extending-upstream.md)` | `bashcut.*` engine module | [in-app-ui.md](bashcut/in-app-ui.md) §3 H2 |
| `crates/engine/src/commands.rs` | 1 | `v.extend(crate::bashcut::commands()); // BashCutPro: bashcut.* commands (bashcut/extending-upstream.md)` | registers `bashcut.*` commands after `captions` | [in-app-ui.md](bashcut/in-app-ui.md) §3 H3 |
| `crates/engine/Cargo.toml` | 1 | `bashcut-dirs = { path = "../bashcut-dirs" } # BashCutPro: our own data folder (bashcut/extending-upstream.md)` | the engine asks our data-folder crate | [rebrand.md](bashcut/rebrand.md) |
| `crates/engine/Cargo.toml` | 1 | `bashcut-manifest = { path = "../bashcut-manifest" } # BashCutPro: plugin manifests for hooks (bashcut/extending-upstream.md)` | the engine's plugin hooks read manifests with the plugin host's reader (RC5) | [plugin-host.md](bashcut/plugin-host.md) |
| `crates/engine/src/autosave.rs` | 1 | `bashcut_dirs::data_dir_first!(); // BashCutPro: our own data folder, next to FilmCraft's (bashcut/rebrand.md)` | first line of `default_data_dir`: BashCutPro's folder, not FilmCraft's (SB1; macro so it stays one line) | [rebrand.md](bashcut/rebrand.md) |
| `crates/engine/src/transcript.rs` | 1 | `bashcut_dirs::models_dir_first!(); // BashCutPro: ours, else FilmCraft's models in place (bashcut/rebrand.md)` | first line of `models_dir`: models are not copied to our folder (SB1) | [rebrand.md](bashcut/rebrand.md) |
| `crates/engine/src/previews.rs` | 1 | `crate::bashcut::brand::previews_temp_first!(untitled_dir); // BashCutPro: "BashCutPro Previews" (bashcut/extending-upstream.md)` | after the wasm check in `default_temp_dir`: unsaved previews under `<temp>/BashCutPro Previews` (BR1; macro so it stays one line; the engine's unit tests keep upstream's name) | [rebrand.md](bashcut/rebrand.md) |
| `crates/engine/src/previews.rs` | 1 | `crate::bashcut::brand::previews_dir_first!(project_path); // BashCutPro: "BashCutPro Previews", an old project's kept (bashcut/extending-upstream.md)` | first line of `dir_for_project`: `BashCutPro Previews/<name>` next to a saved project, an existing `FilmCraft Previews/<name>` kept (BR1) | [rebrand.md](bashcut/rebrand.md) |
| `crates/engine/Cargo.toml` | 1 | `bashcut-analysis = { path = "../bashcut-analysis" } # BashCutPro: bashcut.* measurements (bashcut/extending-upstream.md)` | the engine uses our measurement crate | [in-app-ui.md](bashcut/in-app-ui.md) §3 H1 |
| `packaging/windows/package.ps1` | 1 | `if (-not $env:BASHCUT_BRANDED -and $env:BASHCUT_PACKAGING_PLAIN -ne '1') { & (Join-Path $PSScriptRoot '..\..\bashcut\packaging\package.ps1') @PSBoundParameters; exit $LASTEXITCODE } # BashCutPro: Pro pieces in the installer (bashcut/extending-upstream.md)` | runs `bashcut/packaging/package.ps1` (Pro pieces, #64) unless already inside the generated copy | [bashcut/packaging/README.md](bashcut/packaging/README.md) |

## Carried edits

Upstream files we change beyond hooks. `additions`: only inserted lines (no hook row needed), `edits`: any change,
re-applied by hand (or `rerere`) when a merge conflicts, `deletes`: upstream files we removed (on a modify/delete
conflict keep the deletion; the footprint check fails if one comes back). Keep `edits` rows small and in cold files (bashcut/extending-upstream.md §1).

| Files | Kind | Why | Until |
|---|---|---|---|
| `.github/workflows/*.yml` | edits | workflows run manually only (`on:` blocks), `app-title` in `release.yml` | owner decision 2026-10-20: keep, re-apply on merge |
| `crates/ui-egui/src/brand.rs` | edits | wordmark bytes: a hook would leave ArtCraft's PNGs compiled into our binary (2 lines; 1 upstream commit in 30 days) | keep while cold |
| `crates/ui-egui/src/links.rs` | edits | our URLs: the constants also feed menu hints, the status line and command results, so one mapping hook can't cover them (5 URLs, the 4 Help-menu labels of `ALL` that tests compare with the branded menu (#90), the scheme test; the `help.filmcraftHelp` hint asks `bashcut_brand::help_url`, BR3; 4 commits in 30 days) | keep while cold |
| `crates/ui-egui/src/panels/keyboard.rs` | edits | Help ▸ BashCutPro Help… (F1) opens our guide (BR3): `HELP_URL` is a const (now `bashcut_brand::GUIDE_URL`), and the `help.filmcraftHelp` arm opens `bashcut_brand::help_url()` (the Vietnamese guide when our strings are); a hook can't replace a const or a match arm (3 lines; 8 commits in 30 days) | keep while cold |
| `crates/ui-egui/src/crash.rs`, `crates/ui-egui/src/panels/menu_dialogs.rs` | edits | crash-log header `=== <crate> BashCutPro <version>` and the no-data-dir log folder `<temp>/BashCutPro Logs` (`bashcut_brand::logs_temp_dir`) (BR3; 1 line each, inside one expression; 2 and 13 commits in 30 days) | keep while cold |
| `apps/filmcraft/src/app_nap.rs`, `apps/filmcraft/src/dev_icon.rs` | edits | macOS App Nap activity reason says BashCutPro; the Linux dev desktop entry is `Name=BashCutPro` (BR3) and `com.birchapps.bashcutpro.desktop` with that `Icon=` / `StartupWMClass=` and hicolor icon names, matching the window's app id (`bashcut_brand::APP_ID`, CK1; the icons are read from upstream's `assets/app-icon` names); its test passes `bashcut_brand::NAME`, and the `MimeType=` line is upstream 79f5036c taken early (CK3, drops out on the next sync) (1 and 9 lines; 1 and 5 commits in 30 days) | keep while cold |
| `crates/ui-egui/build.rs` | edits | the contributors folder comes from the run-time `CARGO_MANIFEST_DIR`, not `env!()`: with a shared `CARGO_TARGET_DIR` cargo reuses the compiled build script across checkouts, and `env!` kept the first checkout's path, so About could lose its contributors (FX20; 1 line + marker comment; 1 commit in 30 days) | until upstream reads it at run time |
| `apps/filmcraft/build.rs` | edits | Windows exe icon and version info (5 lines; 1 commit in 30 days); `winresource` appends icons, so a second `set_icon` hook would not replace upstream's | keep while cold |
| `apps/filmcraft/src/logging.rs` | edits | log file `Logs/bashcutpro.log` (+ `bashcutpro.<n>.log`) instead of `filmcraft.log` (BR1): `LOG_FILE` is a const, so no hook can replace it (the const, the rotation name, 3 doc lines and 4 test asserts; 1 upstream commit in 30 days); each message passes `bashcut_cli::log_message`, so the first line says `BashCutPro <version>` (BR2; the `log::info!` sits in hot `main.rs`, where a hook can't replace it) | keep while cold |
| `packaging/**`, `omastore.toml` | edits | branding in packaging ([rebrand.md](bashcut/rebrand.md)); upstream's shared release workflow runs these scripts directly (no pre-step), all cold (≤ 5 commits in 30 days) | generator: #39 comment |
| `NOTICE`, `LICENSE-MIT`, `ATTRIBUTION.md`, `.gitignore`, `AGENTS.md`, `CLAUDE.md` | additions | our copyright lines, asset rows, `.local/`, the pointer to `bashcut/README.md` (+ the scripts-gate and dev-run lines in `AGENTS.md`) | keep |
| `.github/dependabot.yml` | deletes | no GitHub-hosted runners: Dependabot version updates ran on them daily (owner decision 2026-10-23, [decisions.md](bashcut/decisions.md)); security alerts are a repo setting, not this file | never; on a modify/delete conflict keep the deletion |
| `.coderabbit.yaml`, `sonar-project.properties`, `.github/workflows/sonarcloud.yml` | deletes | unused services: we run neither CodeRabbit nor SonarCloud (owner decision 2026-10-23, [decisions.md](bashcut/decisions.md)) | never; on a modify/delete conflict keep the deletion |
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
