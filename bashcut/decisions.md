# Decisions

Newest first. Each entry: what was decided, why, and what it rules out. Add an entry when the owner
decides something; don't change an old one, add a new one that supersedes it.

## 2026-10-23: The CLI is `bashcutpro-cli` (+ `bcpro`); naming follows upstream (NM1)

- **Decided (owner):** names follow upstream FilmCraft's `filmcraft` (GUI) / `filmcraft-cli` (CLI) pair. The GUI is
  `BashCutPro.app/Contents/MacOS/BashCutPro`, `bashcutpro.exe`, `/usr/bin/bashcutpro`; the CLI is `bashcutpro-cli`
  with the alias `bcpro` on every OS (`bashcutpro-cli.exe` + `bcpro.exe`; on Linux `bcpro` is a symlink).
  `apps/bashcutpro-cli` builds the bins `bashcutpro-cli` and `bcpro` (no bin named `bashcutpro` any more); its
  `build.rs` says `bashcutpro-cli` in usage, help, version and errors. Upstream's source still builds the GUI as
  `filmcraft`; packaging renames it (NM2).
- **Lookup** (`bashcut_dirs::engine`, every consumer): `BASHCUTPRO_CLI`, `BASHCUT_FILMCRAFT_CLI`, `FILMCRAFT_CLI`
  (names unchanged), then `bashcutpro-cli` → `bcpro` → `filmcraft-cli` next to the program, then on `PATH`, else
  `bashcutpro-cli` (`FALLBACK`). Exact-file-name matching stays (T6a).
- **Why:** on Linux `/usr/bin/bashcutpro` is the GUI, so a CLI of the same name cannot exist there; the `-cli`
  suffix matches upstream and needs no case trick inside the macOS bundle (#187).
- **Rules out:** a CLI binary named `bashcutpro`, and plain `bashcutpro` in the engine lookup (it would find the GUI).
  Supersedes the names in SB2 and #154 below.

## 2026-10-23: `bashcutpro --bridge` defaults to the open BashCutPro (CC2)

- **Decided:** `bashcutpro`/`bcpro` add `--bridge <address>` from `<data dir>/bashcut/automation.json` (CC1; read with
  `bashcut_dirs::data_dir` + `bashcut_connect::automation::live`: live pid, loopback, port answers within 300 ms) when the subcommand talks to
  the bridge (`commands`, `describe`, `exec`, `run`, `inspect`, `import`, `export`, `mcp`), no `--bridge` is given,
  none of `--project`/`--demo`/`--save`/`--save-as`/`--data-dir` is (they mean the headless engine) and
  `BASHCUT_BRIDGE` is not `off`. A note on stderr names the address. No window found: headless, as before.
- **Where:** `apps/bashcutpro-cli/src/bridge_default.rs`; one `build.rs` anchor routes upstream's argument list
  through it. The subcommand list lives in `build.rs` (`BRIDGED` + `NOT_BRIDGED` = `render`), which fails the build
  when upstream's `match cmd` arms that use `Backend::open(` / `"--bridge"` no longer match it.
- **Rules out:** a default bridge for `render` (headless only) and for callers that name a project; our own tools
  that want the headless engine pass `--project`/`--data-dir` or set `BASHCUT_BRIDGE=off`.
- **Callers (CC3):** headless, `BASHCUT_BRIDGE=off` set: `bashcut/scripts/smoke.sh` + `smoke.ps1` (the CLI checks test
  the engine, not the app they launched), `bashcut/packaging/share/bashcut-setup.sh` + `.ps1` (looks export, profiles
  compile), `bashcut-doctor` (every probe it runs: `commands`, `exec transcript.models`), `bashcut-mcp` on the engine
  it starts (it decides bridge or headless itself; in bridge mode it passes `--bridge`, so `off` only stops a
  headless `mcp` with no project from being bridged anyway); `bashcut-batch` is RC7. Left as they are: the Agent
  section (`mcp --bridge <app>`, explicit), plugins (the host fills their manifest `connect` block, which always
  passes `--project` or `--bridge`), `bashcut/scripts/bashcutpro-mcp.sh` (runs `bashcut-mcp`). xtask and the test
  fakes do not start the engine.

## 2026-10-23: Doctor uses the shared engine lookup; its model check stays on the plugins' cache (RCB)

- **Decided (RC2):** `bashcut-doctor` picks the engine like batch and the UI: `--cli`, else `bashcut_dirs::engine`
  (`BASHCUTPRO_CLI`, `BASHCUT_FILMCRAFT_CLI`, `FILMCRAFT_CLI`, then `bashcutpro` / `bcpro` / `filmcraft-cli` next to
  the doctor, then on `PATH`). The `--version` probe stays a separate step in the `cli` check.
- **Behaviour change:** `bcpro` (the name inside the macOS bundle, #187) and `BASHCUT_FILMCRAFT_CLI` are now honoured,
  and an engine next to the doctor wins over one on `PATH`; the `cli` finding shows the found path. Fakes keep
  no probe: `cli_program` calls `bashcut_dirs::engine::lookup` with the `Env`'s variables, `Env::exe_dir`
  (None for fakes) and `Env::path` (DR1).
- **Not changed:** the `model.<id>` check keeps `<data dir>/bashcut-models` (`BASHCUT_MODELS_DIR`). RV2 listed it as
  a bug against `bashcut_dirs::models_dir` (`<data dir>/models`), but they are two stores: `bashcut-models` is where
  the plugins' downloader (`bashcut_models.py`) caches what `models.json` lists, `models` holds the engine's
  speech/voice models. Pointing the check at `models` would report every plugin model missing. Named
  `bashcut_doctor::PLUGIN_MODELS_DIR` so the difference is visible.


## 2026-10-23: Control channel on by default (CC1)

- **Decided (owner):** make agents "just work". The app starts its control channel at launch with no flag: port 9876
  when free, else a free one, bound to **127.0.0.1 only**. Discovery is the existing
  `<data dir>/bashcut/automation.json` (`address`, `pid`, `project`; removed on close; readers skip a dead pid), so
  `bashcut-mcp`, plugins and the Agent section needed no new reader; no separate `control.json`. No prompt, no token.
- **Opt-out:** the Agent section's "Allow AI agents to control BashCutPro" (default on; off writes
  `<data dir>/bashcut/agents-off`) or `BASHCUT_CONTROL=0`. Applies at the next start: the server is created with the
  window. `--control <port>` / `FILMCRAFT_CONTROL_PORT` still win.
- **Security note:** loopback only, never `0.0.0.0`; any process of the same user can already drive the app (it can
  read and write the project files and the data folder), so the channel adds no new reach for remote attackers. The
  server refuses non-JSON lines (browser `fetch` junk) and caps lines and connections (upstream `control_server.rs`).
  On a shared multi-user machine other local users can reach a loopback port: they should turn the setting off.
- **Rules out:** binding any non-loopback address; a mandatory token or a confirmation prompt.

## 2026-10-23: Linux packages use the app id `com.birchapps.bashcutpro` (SB4)

- **Decided:** the generated Linux packaging (`bashcut/packaging/generate.py`, no upstream edit) ships
  `com.birchapps.bashcutpro.desktop`, `.metainfo.xml`, `/usr/share/mime/packages/com.birchapps.bashcutpro.xml`,
  hicolor icons named `com.birchapps.bashcutpro.png`/`.svg` and the Flatpak `com.birchapps.bashcutpro` (both
  manifests, generated as `target/bashcut-packaging/linux/flatpak/com.birchapps.bashcutpro{,.bundle}.yml`), so a
  BashCutPro package installs beside FilmCraft's without replacing its menu entry, icons or Flatpak. Both Flatpak
  manifests build/install `bashcutpro`, `bcpro` and the Pro tools; none ships `filmcraft-cli`.
- **Kept upstream's:** the app binary name (`filmcraft`, Exec line). Its window id is ours since SB5: a hook in
  `apps/filmcraft/src/main.rs` sets `bashcut_brand::APP_ID` (`com.birchapps.bashcutpro`) as Wayland app id / X11
  class after upstream's `with_app_id`, and the desktop file says `StartupWMClass=com.birchapps.bashcutpro`. The `.fcproj` MIME type keeps `application/x-filmcraft` (shared format): with both apps installed,
  whichever was installed last opens `.fcproj` by default.
- **Flathub from source:** run `python3 bashcut/packaging/generate.py --out target/bashcut-packaging` first, then
  `flatpak-builder … target/bashcut-packaging/linux/flatpak/com.birchapps.bashcutpro.yml` (its sources are the
  checkout and the generated Linux files). Upstream's `packaging/linux/flatpak/ai.storyteller.filmcraft.yml` stays
  as it is (carried speech edit only).

## 2026-10-22: BashCutPro has its own data folder (SB1)

- **Decided:** BashCutPro keeps its state in `BashCutPro` (macOS `~/Library/Application Support`, Windows
  `%APPDATA%`) or `bashcutpro` (XDG), not FilmCraft's folder, so both apps install side by side without sharing
  state. One crate, `bashcut-dirs`, answers for the engine (one-line hook in `autosave::default_data_dir`; the
  engine's own unit tests still see upstream's folder), `bashcut-batch`, `bashcut-connect` and `bashcut-doctor`.
  `BASHCUTPRO_DATA_DIR` overrides, `FILMCRAFT_DATA_DIR` still works (scripts and the kit set it), and upstream's
  portable rule stays (#139: `portable.txt` next to the exe means `<exe dir>/data`, never `%APPDATA%`).
- **One-time import:** the first time our folder is asked for and does not exist while FilmCraft's does, we copy
  `preferences.json` (settings and recent projects), `export-presets.json`, `bashcut/` (without the open window's
  `automation.json`) and `bashcut-plugins/`, and write `bashcut/imported-from-filmcraft.txt` (the log; never runs
  twice). Not copied: `Auto-Save`, `Recovery`, `Logs`, graphics state and `models`.
- **Models (SB3, 2026-10-23):** `transcript::models_dir` (hook) always answers our `models` folder (created), so a
  download lands in BashCutPro's folder. Reads fall back per file: once per process, each file of FilmCraft's
  `models` that ours lacks is hard-linked in (a symlink on Unix when hard links fail; no `.part` leftovers, never
  over a file of ours), so the same gigabytes are not downloaded or copied twice. FilmCraft's files stay read
  only: the downloader writes `<name>.part` and renames it over the link. Replaces SB1's folder-level fallback.
- **Why:** owner decision 2026-10-22. eframe's app name stays "FilmCraft": the build has no eframe persistence, so
  it stores nothing under that name.

## 2026-10-22: `bashcutpro` is generated from upstream `filmcraft-cli` at build time (SB2)

- **Decided:** the `apps/bashcut-cli` wrapper is gone. `apps/bashcutpro-cli` is a small committed crate (bins
  `bashcutpro` and `bcpro`); its `build.rs` copies `apps/filmcraft-cli/src/*.rs` into `OUT_DIR`, renames the
  user-visible text (`filmcraft-cli` → `bashcutpro`, "FilmCraft" → "BashCutPro" in help/usage/version/errors) and the
  bins `include!` the result. Command ids, JSON fields, MCP tool names and `FILMCRAFT_*` vars are unchanged.
- **Why this layout:** the crate always exists, so `Cargo.lock` is stable and `cargo build` works on a fresh clone
  without a script; the generated source lives in `target/` only (nothing to gitignore); upstream's crate stays
  byte-identical and keeps its own tests (the copy carries them too: the `bashcutpro` bin runs its tests since T5, for `src/bridge_default.rs`).
- **Loud failure:** `build.rs` fails the build when an anchor string, the file list, the `mod` lines, the crate
  attribute or upstream's `[dependencies]` change, naming what to update.
- **Lookups:** our tools try `bashcutpro`, then `bcpro` (the only name inside the macOS app bundle, #187), then
  `filmcraft-cli` (dev builds). Batch runs the found binary directly on every OS (no wrapper to see through).
- **Packages:** `bashcut/packaging/generate.py` renames `filmcraft-cli` to `bashcutpro` in the copied scripts and
  manifests (`RENAMES`, loud when a listed file has nothing to rename), so no package ships `filmcraft-cli`.
  Not yet: upstream's Flatpak source manifest (`packaging/linux/flatpak/*.yml`, read by flatpak-builder directly)
  and the Linux ids `com.birchapps.bashcutpro`.

## 2026-10-23: Delete upstream's CodeRabbit and SonarCloud config

- **Decided:** remove `.coderabbit.yaml`, `sonar-project.properties` and `.github/workflows/sonarcloud.yml`, recorded
  as a `deletes` row in `UPSTREAM.md` Carried edits. On a modify/delete merge conflict keep the deletion;
  `upstream.sh footprint` fails if one of them comes back.
- **Why:** owner: "xoá đi, chúng ta không cần"; we use neither service, the files were inert.

## 2026-10-20: Port every BashCut capability, in Pro's own way

- **Decided:** Pro covers what the Swift BashCut can do, but not by copying it: each capability is ported as a
  similar or different feature that fits FilmCraft, and when it does not belong in the core it lives in a kit skill,
  a plugin or our own module. This reverses the earlier "skip" for `library` and `self-learn` in
  [migration.md](migration.md). Wave 3 tickets #66–#73 (self-learn, library, online browser, vision, variants,
  script alignment, outcomes, doctor / hooks).
- **Why:** owner: Pro must surpass the old app; "port qua tính năng tương tự hoặc khác đi, cho phù hợp… nếu k ở trong
  core thì skill support".

## 2026-10-20 (later): Upstream edits allowed when they stay under control

- **Decided:** additions stay the default, but upstream files may be edited when that is simpler, as long as the
  edit stays under our control: small, preferably in a cold file, recorded as a `UPSTREAM.md` Carried edits row, and
  re-applied by merge + `rerere` at each sync. Hot files and high-churn structs still go through hooks and our own
  files. Sync stays merge-based (no rebase of the fork onto upstream). Rules: [extending-upstream.md](extending-upstream.md) §1.
- **Why:** owner: "no need to be that strict, as long as it stays within our control". Strict add-only made some changes
  (branding, CLI naming) costlier than the conflicts they avoid.
- **Supersedes:** the strict wording of "Upstream code is only added to, never modified" below.

## 2026-10-20: Upstream code is only added to, never modified (superseded: see above)

- **Decided:** we never edit, delete or reorder upstream lines, signatures, structs, defaults, commands, tests or
  docs. Our behaviour comes from new files, new `bashcut.*` commands that wrap originals, extension `impl`s / traits,
  and one-line **inserted** hooks. Rules, extension points and upstream hot spots: [extending-upstream.md](extending-upstream.md).
- **Why:** owner: upstream's original is solid; extend it instead of changing it, so syncs stay cheap.
- **Exceptions:** the manual-only workflows (owner decision) and today's branding edits, which #39 turns into additions.

## 2026-10-20: In-app UI design accepted; side panel by default

- **Decided:** build the in-app UI as in [in-app-ui.md](in-app-ui.md): a right rail and side panel reserved by one
  layout hook (no `PanelKind`), sections Agent / Plugins / Runs, code in `bashcut-*` crates and
  `crates/ui-egui/src/bashcut_ui/`, about 15 hook lines in upstream files. Supersedes the "floating window" default of
  the plugin-host approval: floating windows stay as the pop-out mode. Tickets #34–#40 (tracking #40).
- **Why:** owner asked for a UI designed for BashCutPro (not a copy of BashCut's) that extends FilmCraft without
  merge conflicts.

## 2026-10-20: One-way upstream sync, no contributions upstream

- **Decided:** BashCutPro is a separate fork. We pull `upstream/main` (FilmCraft) into it from time to time and
  never open PRs or issues against `storytold/filmcraft`.
- **Why:** owner's call: the two projects stay separate; upstream keeps developing fast and we only take its changes.
- **How:** engine or UI features we need (loudness meter, beat detection, picture checks, word-by-word captions,
  plugin host) live in new `bashcut`-prefixed crates or files. Upstream files get at most a one-line hook at a spot
  upstream rarely edits, recorded in `UPSTREAM.md`. Sync with `.local/scripts/upstream.sh` on upstream tags or every
  1–2 weeks, with `git rerere` on, then `cargo check` and the `bashcut-*` tests.
- **Rules out:** upstreaming the engine gaps in [skill-needs.md](skill-needs.md) as a way to avoid carrying them.

## 2026-10-20: bashcut-plugin-host design approved

- **Decided:** the owner approved [plugin-host.md](plugin-host.md). P1–P2 show plugins in a floating egui window
  (no new `PanelKind`), and plugins install only from `registry.json` at a git ref (no local-folder install).
- **Why:** a `PanelKind` variant touches upstream's dock and workspace files on every merge; a single install
  source keeps what runs reviewable.
- **Next:** P1 code (list / install / run / settings) in `crates/bashcut-plugin-host`.

## 2026-10-20: Plugin manifest and registry format

- **Decided:** a plugin is a folder in `bashcutpro-plugins/plugins/<name>/` with a `bashcut-plugin.json`
  (`schema: "bashcut-plugin/1"`): id, version, platforms, `runtime` (e.g. Python ≥ 3.9, packages), `entry` (an argument
  list, with per-OS overrides), `connect` (how to pass `--project <fcproj>` / `--bridge <host:port>` / `--cli <path>`),
  `app` (min app version, MCP tools, engine command ids and effect ids it calls), `settings` (each with a flag and
  environment variables; secrets only through those), `actions` (args, params as flags, `needs`, `edits`, an optional
  `handoff`) and `skills`. `registry.json` (`schema: "bashcut-registry/1"`) lists each plugin with the fields needed to
  choose it and where its files are (`source`: a folder of the repo at a git ref). `tools/check_registry.py` keeps the two
  consistent.
- **Result protocol** (`bashcut-result/1`): one JSON object on stdout (`ok`, `message`, `data` | `error`), progress on
  stderr, exit 0 / 1 / 2 (usage) / 3 (hand-off: the plugin needs the calling agent, e.g. no API key, and `data` holds what
  the agent needs).
- **The plugin connects to the app itself** (it starts `filmcraft-cli mcp`), rather than the host proxying calls. Why: it
  works today with no host at all (agents and shells run it), and the same command line is what `bashcut-plugin-host`
  will run later; the host only adds the connection arguments and secrets.
- **Secrets** come from environment variables or a flag, never from the plugin's settings file
  (`<data dir>/bashcut-plugins/<id>/settings.json`, non-secret settings only). A host keeps keys in the OS key store and
  sets the variables; agents never see a key.
- **`app.commands` lists engine command ids**, not BashCut tools, so a host (or a test) can check them against
  `command_list` of the running build before an action runs.
- **Rules out:** an in-process plugin ABI, plugin code inside the BashCutPro tree, and BashCut's plugin protocol
  (session transport, host calls, `pluginData`), which the fork does not have.
- **Gaps seen while porting translate-captions** (engine features worth an upstream-friendly issue, not blockers):
  `command_batch` makes each edit its own undo step (a 200-line translation is 200 undos; a grouped batch would be one);
  there is no OCR / text detection command, so burned-in subtitles are found by a pixel heuristic on `file.exportFrame`
  stills; `captions.list` has no revision to guard an agent's late answer, so `apply` compares texts instead.

## 2026-10-20: Names for our plugin layer

- **Decided:** the in-app host crate is `crates/bashcut-plugin-host` (package `bashcut-plugin-host`).
  Command ids `bashcut.plugins.*` (`bashcut.plugins.list`, `bashcut.plugins.run`…), menu Window ▸
  BashCut Plugins, install folder `<data dir>/bashcut-plugins/`, plugin manifest `bashcut-plugin.json`.
- **Why:** upstream gap G11 (VST3 / AU / OpenFX hosting) will likely take names like `plugin-host` or
  `plugins`; a `bashcut` prefix keeps our code visibly ours and conflict-free. `ext-host` was rejected
  as vague.

## 2026-10-20: Plugins run outside the app

- **Decided:** plugins are separate processes that talk to BashCutPro through MCP or the control
  channel and import their results with existing commands. The registry and plugins live in
  `bashcutpro-plugins`.
- **Why:** FilmCraft has no plugin system, and adding one inside the core would make every upstream
  merge painful.
- **Limits:** no in-app panels, menus or real-time render effects until `bashcut-plugin-host` exists.

## 2026-10-20: Rewrite skills for FilmCraft instead of mapping BashCut's

- **Decided:** a new `bashcutpro-agent-kit` with skills written against FilmCraft command ids. Port the
  editing knowledge from `bashcut-agent-kit`, not its commands. `bashcut-agent-kit` and `bashcut-plugins`
  stay with the Swift BashCut; the two sides are not kept in sync.
- **Why:** BashCut's ~300 dedicated tools and plugin API (2–8) do not exist in FilmCraft. Mapping them
  would mean either a translation layer that drifts, or reshaping the fork's core and losing upstream.

## 2026-10-20: Stay mergeable with upstream FilmCraft

- **Decided:** keep merging `upstream/main` (procedure and log in `UPSTREAM.md`). Our code goes into
  new files and `bashcut`-prefixed crates; edits to upstream files stay minimal.

## 2026-10-20: GitHub Actions are manual

- **Decided:** every workflow in `.github/workflows/` triggers on `workflow_dispatch` only (commit
  `fc3cd05c`).
- **When merging upstream:** if a workflow's `on:` block conflicts, keep `workflow_dispatch` only.

## 2026-10-23: One plugin manifest reader (RC5)

- **Decided:** `bashcut-plugin.json` is read only by `crates/bashcut-manifest` (layer 1, deps `bashcut-dirs` +
  serde), with the `hooks` list in its `Manifest`. The plugin host re-exports it as `manifest`; the engine's hooks
  and `bashcut-doctor` use it as well, so a manifest the host would refuse declares no hooks, and the doctor
  reports it as a warning (`plugin.<folder>`) instead of skipping it. OS names come from `bashcut_dirs::Os`:
  every Unix that is not macOS is `linux` (the `entry.linux` override, XDG folders).
- **Why:** three parsers had drifted (doctor used raw `consts::OS`, hooks skipped validation).

## 2026-10-23: One child-process supervisor (RC3)

- **Decided:** every child BashCutPro supervises (plugin action runs, the key-store tool, `git` for installs, agent
  turns, engine hooks) goes through `bashcut_dirs::child` (layer 0, so the engine may use it without a new upstream
  line). Cancel and timeout both stop softly first (TERM to the process group on Unix, KILL after 1.5 s); on Windows
  both are a kill (a tree kill since RC13). Plugin settings / secrets / run env live in `bashcut_manifest::settings`
  (re-exported as the host's `settings`), so hooks get the same env as action runs, secrets included (the OS key store,
  as the app's host reads it). The app's host reads secrets through a cache warmed on a side thread
  (`bashcut_ui::plugins::CachedKeyStore`).
- **Why:** three copies of the supervisor had drifted (hooks killed hard and dropped stderr, the plugin runner killed
  hard on timeout so a plugin could not stop its MCP child), the documented `check-key` hook could not see its key,
  and a locked keyring froze frames for up to 20 s.

## 2026-10-23: Secret writes off the UI thread; Windows tree kill without job objects (RC13)

- **Decided:** the Plugins section's secret Save / Clear write on a side thread (`plugins::set_secret_async`); the row
  shows a spinner with "Saving…" / "Clearing…" and its field and buttons wait until the write settles (a failure goes
  to the status line). The control channel's `bashcut.plugins.setSecret` stays synchronous (it answers with the
  result). `CachedKeyStore`: writes win over reads in flight (a read fills only an empty key; a successful delete
  caches `None`), so the warm thread cannot bring back a value deleted while it ran. It wraps any
  `SecretStore` (`CachedKeyStore::new`; the OS store by default, a fake via `plugins::use_key_store` before the host
  exists), which is how the tests drive it (T7). On Windows
  `bashcut_dirs::child::terminate` runs `taskkill /PID <pid> /T /F`, killing grandchildren too, then falls back to
  `Child::kill`; Windows still has no soft stop.
- **Why:** an unlock prompt froze the app on a click; job objects need `unsafe` FFI, which only `crates/platform`
  may use (`AGENTS.md` §0.3), and `taskkill /T` gets the tree without it.

## 2026-10-23: Two windows: the first started keeps automation.json (FX7)

- **Decided:** a window writes `<data dir>/bashcut/automation.json` only when the file is missing, malformed, already
  its own, or names a process that is gone (dead pid, or its loopback address no longer accepts a connection). It
  removes the file on close only when the file names its own pid. Every window checks every 3 s
  (`bashcut_ui::connect::CHECK`, with a repaint request so an idle window checks too), so a second window takes the
  file over within a few seconds after the first one quits or crashes. While both run, agents, plugins and `bcpro`
  bridge to the first window. A window started with an explicit `--control` gets no priority.
- **Why:** before, a second instance (such as `smoke.sh --control 19000`) overwrote the file, deleted it on quit, and
  the first window never wrote it again, so agents lost the running app. First-started is simpler than last-focused:
  there is no focus tracking, and the file does not move between windows while an agent is bridged. CC2 bridge
  discovery (`automation::live`) is unchanged.
