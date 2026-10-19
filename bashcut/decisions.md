# Decisions

Newest first. Each entry: what was decided, why, and what it rules out. Add an entry when the owner
decides something; don't change an old one, add a new one that supersedes it.

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
