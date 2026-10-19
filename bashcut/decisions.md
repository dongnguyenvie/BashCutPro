# Decisions

Newest first. Each entry: what was decided, why, and what it rules out. Add an entry when the owner
decides something; don't change an old one, add a new one that supersedes it.

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
