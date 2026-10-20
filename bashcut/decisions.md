# Decisions

Newest first. Each entry: what was decided, why, and what it rules out. Add an entry when the owner
decides something; don't change an old one, add a new one that supersedes it.

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
