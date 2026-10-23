# Design: BashCutPro's in-app UI and how it extends FilmCraft

Status: **accepted** (2026-10-20); tickets #34 (U0), #31 (U1), #35 (U2), #36 (U3), #37 (U4); batch and quality
additions #42–#54 (§1a); tracking #40. Extends [plugin-host.md](plugin-host.md) (#23, #31) and follows the one-way
upstream sync decision in [decisions.md](decisions.md): everything below is ours, lives in our own files, and
reaches upstream code only through a short list of one-line hooks.

It does not copy BashCut's Swift UI. It is built for FilmCraft's egui shell and for how BashCutPro is driven
(agents over MCP, plugins as processes).

## 1. What the user gets

```
┌ header (upstream) ─────────────────────────────────────────────────────────────┐
│ File Edit Clip …                                              ⌕   BashCutPro    │
├────────────────────────────────────────────────────────────┬──────────────┬──┤
│                                                            │ Agent ▾      │🤖│
│        FilmCraft dock (upstream, untouched)                │ ──────────── │🧩│
│   Project │ Source │ Program │ Effect Controls …           │ you: cut the │⏳│
│                                                            │  ums, keep … │  │
│                                                            │ agent: plan  │  │
│                                                            │ ┌ Gate ────┐ │  │
│        Timeline                                            │ │ Rough cut│ │  │
│                                                            │ │ [Approve]│ │  │
│                                                            │ └──────────┘ │  │
│                                                            │ [message…] ⏎ │  │
├────────────────────────────────────────────────────────────┴──────────────┴──┤
│ status bar (upstream)                                                         │
└───────────────────────────────────────────────────────────────────────────────┘
                                                      side panel ↑        rail ↑
```

- **Rail**: a 36 px strip on the right edge, always there. One icon per section, top to bottom: **Agent**,
  **Batch**, **Looks**, **Plugins**, **Runs** (§1a). Plugins that declare a panel add their own icon below. Click
  toggles the side panel; Cmd/Ctrl+Alt+Shift+A focuses Agent (Cmd/Ctrl+Shift+A is upstream's Deselect All);
  **the command palette** (§1a) has its own shortcut (#52; Cmd/Ctrl+K is upstream's Add Edit).
  An icon's tooltip gives the title, the blurb and the shortcut from the live shortcut map. A dot marks news in a
  section that is not on screen (UX9): on **Runs**, accent while a plugin job runs and `danger` once a run failed
  that Runs has not shown yet; on **Batch**, accent while a batch runs. Showing the section (docked or popped out)
  clears it. The rail button's automation label says so (`Runs: failed`, `Runs: running`, `Batch: running`).
- **Side panel** (left of the rail): resizable (240–720 px), collapsible, width and open section remembered.
  It takes width from the dock area, so it behaves like a docked panel without being in FilmCraft's dock tree.
  Any section can pop out into a floating window (for a second monitor) and dock back.
- **Agent**: chat with the editing agent (Claude Code, Codex, or another agent plugin), streamed replies, tool calls
  collapsed to one line each ("timeline.razor ×12"), **gate cards** for the edit-workflow gates (brief, plan,
  rough cut, draft) with Approve / Change / Stop, and a Stop button that ends the turn.
- **Plugins**: installed plugins from `registry.json`, install / update / remove, settings form generated from the
  manifest (secrets go to the OS key store), actions with a params form, and the plugin's own view when it has one.
- **Runs**: plugin runs, installs and registry checks with progress (from the plugin's stderr), elapsed time, the
  full error of a failed run, Retry, Copy log, Reveal for result paths, Cancel, and "Send to agent" for a hand-off
  result (`bashcut-result/1`, exit 3). A running batch adds one row, "Batch: n of m · Open Batch"; its items live in
  the Batch section. Agent turns live in the Agent section, not here (UX6; the last run per plugin also shows in its
  card, UX5).

### 1a. Built for batch and for better results (2026-10-20)

Owner's goals for Pro: batch runs must be smooth and first-class, and finished videos must be better than the old
BashCut's. The UI follows from that:

- **Batch** (#46, runner #45): a batch is a list of inputs, steps and output profiles; each input becomes its own
  project; items run **in parallel and headless** (one `filmcraft-cli` per item), so the window stays free and a
  batch survives closing the app (resumable job book). The section shows one row per item with a staged progress
  bar that learns step durations, output thumbnails, and **Open to fix / Retry / Reveal**. Any plugin action or tool
  can start one ("Run on many files…"). Templates with slots (#42) are the usual starting point.
- **Quality by default, visible**: output profiles per platform (#43) set size, bitrate, loudness and captions so
  every export is platform-correct; **Looks** (#47) is a curated pack with a one-line purpose per look, engine-rendered
  thumbnails and golden checks; one-knob voice polish (#48); word-by-word caption styles (#49). Review findings
  appear as markers (rule 1 below) and in the Agent section's gate cards with the measurement behind them.
- **Command palette** (#52): one search over commands, plugin actions, looks, profiles, recipes and batch presets,
  with current shortcuts; the fastest path for users who do not want the agent.
- **Long jobs never block**: plugin runs, installs and registry checks show in Runs with honest staged progress (a
  running batch as one row that opens the Batch section; agent turns in the Agent section);
  sections are non-modal and follow the current selection.
- **Agents see less, act surer**: compact timeline summary and typed errors (#50), atomic grouped edits as one undo
  step (#44), auto-connect to the open window with a project lock (#51).

Rules that keep it out of upstream's way:

1. **Never draw inside an upstream panel.** Anything that belongs on the timeline or a monitor (beats, review
   findings, selects, chapters) is written as project data upstream already renders: markers with colours and
   comments, clip labels, subclips. The agent and plugins write it with ordinary commands.
2. **No new `PanelKind`, no edit to dock serialization or workspaces.**
3. **No new field in `FilmcraftApp` or `UiState`.** Our state lives in our own module (§4).

## 2. Where the code lives

| Path | Layer | What |
|---|---|---|
| `crates/bashcut-plugin-host/` | L4 (no workspace deps) | manifests (`bashcut-plugin/1`), registry (`bashcut-registry/1`), install into `<data dir>/bashcut-plugins/`, runner (spawn with connect args, stderr progress, `bashcut-result/1` parse, cancel, timeout, capped output), view model (`bashcut-view/1`). No egui, no OS crates; testable headless. |
| `crates/bashcut-agent/` | L4 (no workspace deps) | agent sessions: start an agent CLI as a child process with an MCP config pointing at the app (`<engine> mcp --bridge 127.0.0.1:<control port>`, engine `bashcutpro-cli`, else `bcpro` or `filmcraft-cli`), parse its event stream into a transcript model (message, tool call, gate, error), stop, resume; conversations saved in the project sidecar (§5). Adapters per agent (Claude Code `stream-json`, Codex) behind one trait. |
| `crates/bashcut-batch/`, `apps/bashcut-batch/` | L4 (no workspace deps) / app | batch pipelines, parallel headless items through `filmcraft-cli`, job book, presets (#45). `apps/*` glob: no upstream edit. |
| `crates/bashcut-analysis/` | L3 | read-only measurements for the engine gaps in [skill-needs.md](skill-needs.md): loudness (wraps `filmcraft-audio-dsp` BS.1770), beats / tempo (wraps `audio-dsp`'s remix onset analysis), black / frozen frames (its own frame-difference math over frames the engine adapter decodes). Depends only on `audio-dsp` (L1), so the engine (L4) can depend on it without a same-layer edge; pure functions, wasm-safe. |
| `crates/engine/src/bashcut/` | (engine) | `mod.rs` with `commands() -> Vec<CommandSpec>`: `bashcut.audio.loudness`, `bashcut.audio.beats`, `bashcut.review.pictureChecks`, … They read the `Session`, call `bashcut-analysis`, return JSON. Headless MCP gets them for free because they are engine commands. |
| `crates/ui-egui/src/bashcut_ui/` | L5 | `mod.rs` (the hooks' entry points), `rail.rs`, `side_panel.rs`, `agent_panel.rs`, `plugins_panel.rs`, `runs_panel.rs`, `batch_panel.rs`, `looks_panel.rs`, `palette.rs`, `view.rs` (renders `bashcut-view/1` with egui widgets), `state.rs`, `strings/` (our i18n), `widgets.rs` (the automation-id `button` / `small_button` every section uses, and `note` with the `NOTE` / `META` text sizes for dim notes and inline errors), `shared.rs` (`get_or_init` / `existing` / `lock`: the plugin host, agent and batch held once in context data). Uses upstream's `Tokens`, `widgets` and automation registry read-only. |
| `crates/ui-egui/tests/bashcut_ui.rs` | test | headless scripted UI tests for the rail and panels (new file). |
| `xtask/src/bashcut.rs` | tool | the layers of our ten crates (`bashcut-dirs` 0, `bashcut-analysis` 3, `bashcut-plugin-host`/`-agent`/`-batch`/`-connect` 4, the apps `bashcutpro-cli`, `bashcut-batch-cli`, `bashcut-doctor`, `bashcut-mcp` and `bashcut-winres`, the build helper that gives their Windows exes the BashCutPro icon and VERSIONINFO (PK4), 6), so `cargo xtask layers` knows them; the L4 crates depend on no workspace crate but `bashcut-dirs`, and only `ui-egui` (L5) and our apps use them. |

New crates join the workspace by the existing `members = ["crates/*"]` glob, so the root `Cargo.toml` is not
edited. Our crates reference upstream crates by path (`filmcraft-engine = { path = "../engine" }`) instead of adding
entries to `[workspace.dependencies]`.

**Why the plugin host and agent are not engine commands:** running a plugin or an agent from the app is a UI
concern; headless agents already run plugins themselves. Their commands (`bashcut.plugins.*`, `bashcut.agent.*`,
`bashcut.ui.*`) are frontend commands like upstream's `window.*`: listed with `external_commands`, rebindable,
reachable over the control channel and MCP bridge mode.

## 3. The hooks: every upstream line we add

Every hook is an **inserted** line; no upstream line is edited (rules: [extending-upstream.md](extending-upstream.md)).

| # | File (upstream) | Line(s) | Churn at that spot since 2026-09-01 |
|---|---|---|---|
| H1 | `crates/engine/Cargo.toml` | `bashcut-analysis = { path = "../bashcut-analysis" }` | dependency list, low |
| H2 | `crates/engine/src/lib.rs` | `pub mod bashcut; // BashCutPro` | mod list, low |
| H3 | `crates/engine/src/commands.rs` (`build()`) | inserted right after `v.extend(crate::captions::commands());` (the old head of the list): `v.extend(crate::bashcut::commands());` | 1 change at that neighbour (the tail changed 3×: upstream appends there) |
| H4 | `crates/ui-egui/Cargo.toml` | `bashcut-plugin-host`, `bashcut-agent`, `bashcut-batch` path deps | low |
| H5 | `crates/ui-egui/src/lib.rs` | `pub mod bashcut_ui;` next to `pub mod bashcut_brand;` | already ours |
| H6 | `crates/ui-egui/src/lib.rs` (`ui()`) | right after `let body = …`: `let body = bashcut_ui::layout(self, ui, body);` | 2 changes |
| H7 | `crates/ui-egui/src/menus.rs` (`invoke`) | first line: `crate::bashcut_ui::route!(app, ctx, id, &params);` (a macro in our file that returns early, so `cargo fmt` keeps it one line) | 4 changes (highest; rerere replays the resolution) |
| H8 | `crates/ui-egui/src/menus.rs` (`external_commands`) | `v.extend(crate::bashcut_ui::commands());` before `v` | 3 changes |
| H9 | `crates/ui-egui/src/menus.rs` (`menu_items_for`) | `out.extend(crate::bashcut_ui::menu_items(session));` before `out` | 2 changes |
| H10 | `crates/ui-egui/src/i18n/mod.rs` | **not needed** (#34): our strings are translated by `bashcut_ui::strings::s`, which reads the interface language or our own override (Vietnamese is not an upstream language) | — |
| H12 | `crates/ui-egui/src/lib.rs` (`frame()`) | before `self.handle_drops(&ctx);`: `bashcut_ui::batch::take_drops(self, &ctx);` (files dropped on the Batch section's drop zone become batch inputs instead of project imports, #46) | low |
| H13 | `crates/ui-egui/src/lib.rs` (`logic()`) | right after `self.drain_control(ctx);`: `bashcut_ui::logic(self, ctx);` (`automation.json` follows the open project also while an occluded macOS window skips `ui()`, #191) | low |
| H14 | `crates/ui-egui/src/control.rs` (`ui.click` arm) | after the click point is resolved: `crate::bashcut_ui::modal::guard!(app, ctx, req, pos);` (macro, as H7; FX14) | low |
| H11 | `xtask/src/main.rs` | `mod bashcut;` and, as the first line of `layer_of`: `bashcut::our_layer_first!(name);` (macro, as H7) | 2 changes |

About **16 lines in 8 upstream files**, all marked `// BashCutPro`, all listed in `UPSTREAM.md`.
`Cargo.lock` conflicts are resolved by taking upstream's file and running `cargo check` (it re-adds our crates).

What `bashcut_ui::layout` does in H6, in order: read our state; if the rail is hidden (setting or Import/Export
mode) return `body` unchanged; draw the rail at the right edge of `body`; if a section is open, draw the side panel
left of it; draw floating (popped-out) sections as `egui::Window`s and toasts as foreground `egui::Area`s; return
the shrunk rect for the dock. One call per frame covers docked, floating and overlay UI.

## 4. State, settings, automation, strings

Built in #34: `bashcut_ui/{mod,rail,side_panel,state,strings}.rs`. Sections are rows of `bashcut_ui::SECTIONS`
(id, title, icon, blurb, `draw` fn, optional `badge` and `shortcut`).

**Adding a section** (RU6): one `SECTIONS` row and a `draw` fn in its own file. The rail, `show.<id>`, the
`params()` hints and `ui.json` validation all read the table. A rail dot is the row's `badge` fn (called every
frame with "on screen"; the rail hides the dot while shown), a tooltip shortcut other than `show.<id>` its
`shortcut` command id. Commands of its own: one `MODULES` row (prefix, `invoke`, `commands`) in `mod.rs`.
New strings (title, blurb) still need their `strings/vi.tsv` rows.

Commands so far:
`bashcut.ui.state`, `toggleRail`, `focusAgent` (Cmd/Ctrl+Alt+Shift+A), `section {section, open?}`, `closePanel`,
`setWidth {width}`, `popOut {section?}`, `dockBack {section}`, `setLanguage {language}`, `show.<section>`
(Window ▸ BashCutPro), and `setLanguage.follow`, `setLanguage.en`, `setLanguage.vi` (no params; Window ▸
BashCutPro ▸ Language ▸ Follow App / English / Tiếng Việt, each running `setLanguage` with its code; `follow`
clears our override so the interface language is used). Our files go under `<data dir>` = the session's auto-save data dir (`bashcut_ui::data_dir`);
with no persistence (headless tests) nothing is read or written.

Built in #31 (U1): `bashcut_ui/plugins.rs` keeps one `bashcut_plugin_host::Host` in context data (key
`bashcut.plugins.host`), polled by `layout` each frame; `plugins_panel.rs` and `runs_panel.rs` draw the sections.
Commands `bashcut.plugins.{list, registry {location?}, install|update {plugin}, remove {plugin}, settings {plugin,
values?}, setSecret {plugin, setting, value?}, run {plugin, action, params?, timeoutSeconds?, bridge?}, runs, job
{job}, cancel {job}, clear, sendToAgent {job}}` all go through `Host::command`, so the buttons, the control channel
and MCP share one code path (`sendToAgent` answers `sent: false` until the Agent section, #35). Runs connect with
`--bridge 127.0.0.1:<port>` while the app serves its control channel (on by default, CC1; `--control <port>` or
`FILMCRAFT_CONTROL_PORT` pick the port), and get
`--cli` = `filmcraft-cli` next to the app. Automation ids: `bashcut.plugins.registry.refresh`,
`bashcut.plugins.<id>[.install|.update|.remove]`, `bashcut.plugins.<id>.settings.<setting>` / `.settings.save`,
`bashcut.plugins.<id>.secret.<setting>[.save|.clear]`, `bashcut.plugins.<id>.action.<action>[.param.<param>]`,
`bashcut.plugins.<id>.<action>.run`, `bashcut.runs.<job>[.progress|.cancel|.log|.agent]`, `bashcut.runs.clear`.

Action forms (UX2) follow the manifest: the label is the param's `title`, else its id in words (`maxCps` → "Max
cps"). A param with neither `optional` nor a `default` is required (a `bool` never is: a checkbox always has a value):
its label ends in `*`, and **Run** / **Run on many files…** stay disabled with "Fill in: …" as the tooltip until it
is filled. `integer` / `number` are drag values clamped to `minimum..=maximum` and go out as JSON numbers; one with
no value shows "–" and is left to the plugin's default. `path` adds **Browse…** (`…param.<param>.browse`), which
picks one file. `plugins_panel::field` / `Spec` are `pub(super)` so other forms (the Batch step form) draw the same
fields.

Control channel on by default (CC1, `bashcut_ui/control.rs`): with no `--control` / `FILMCRAFT_CONTROL_PORT`, the
launch hook in `apps/filmcraft/src/main.rs` takes port 9876 on 127.0.0.1 when free, else one the OS picks, and
`automation.json` names it as before. Off when the user unticks **Allow AI agents to control BashCutPro** (Agent
section, `bashcut.agent.allowControl`, writes `<data dir>/bashcut/agents-off`) or with `BASHCUT_CONTROL=0`; a change
applies at the next start ("Restart BashCutPro to apply."). When the setting turned it off, the "off" lines below
get a **Turn on** button (`bashcut.plugins.turnOn`). An explicit `--control` always wins.

Setup hints (UX4, `bashcut_ui/hints.rs`): started without the control channel, Plugins shows one dim line at the top
("Control channel is off…", `bashcut.plugins.setup`, tooltip: how to turn it on). A `needs: "project"` action then
says "needs the control channel" (`…action.<action>.blocked`, same tooltip) instead of "uses the open project": its ▶
and its form's **Run** are disabled, with the channel reason and "Fill in: …" as one list in **Run**'s tooltip. The
form still opens and **Run on many files…** stays enabled, because a batch opens the project file itself and needs no
channel. Error status lines go through `hints::friendly`, which turns the known setup errors (`python3` or
`claude` cannot start, `no_cli`, `needs the open project`) into one sentence plus "See Troubleshooting".
`hints::python_found` / `cli_found` are the `bashcut-doctor` lookups, asked once per session, for other sections (UX7).

Run outcome in the card (UX5): each plugin card shows its last run under its title (also while the card is closed),
`bashcut.plugins.<id>.last`: "Running 40 %" / "Running…", "Done", "Needs the agent", "Cancelled", or "Failed: …"
through `hints::friendly` (a run that did not start at all shows its error there too, not in the panel's status line),
then **Open in Runs** (`….last.runs`, switches the rail section) and ✕ (`….last.dismiss`). The last run per plugin
lives in the panel's `Drafts`; a newer run of that plugin started anywhere (palette, agent, control channel) takes its
place. The panel-top "Started run N" is gone. **Decision (audit §8, P3): no auto-switch to Runs.** Running an
action, from the panel or by command, leaves the open section alone; the card says how it went and links to Runs.
(The audit's "lost switch" was the translate form being a `view` action, which opens Plugins by design.) A `view`
action still opens Plugins, where its form is. **Install** reads **Installing…** (disabled) and **Update to …**
reads **Updating…** while the host installs that id, so a double click starts one install; a failed install says
"Failed: …" under its row (`bashcut.plugins.<id>.install.status`). The first time Plugins is drawn with a data folder
and no registry (and no check made yet), it checks the registry once; the line under **Available**
(`bashcut.plugins.registry.status`) says "Checking the registry…", "N plugins available" / "All plugins are installed",
or "Could not check …", with the registry location as its tooltip. Headless tests never reach the network: their
`Driver::with_data` points the host at an empty local `registry.json` (`bashcut.plugins.registry {location}`, or
`BASHCUT_PLUGIN_REGISTRY=<file>` for a child test process) before anything draws, and wait on async state with the
bounded `Driver::wait*` helpers (TS1).

Finished-run toasts and dismissable status lines (RN1, `bashcut_ui/toast.rs`, audit X3): a plugin run
(`JobKind::Run`) that ends while Runs is not on screen (not docked or popped out, rail hidden, or not Edit mode)
gets one toast, "✓ <run title> finished" or "✕ <run title> failed" (`bashcut.toast.<job>`), with **Show**
(`….show`: opens Runs, which outlines that run in the accent colour and scrolls to it) and ✕ (`….dismiss`). Toasts
are a foreground `egui::Area` at the bottom right, left of the rail, at most 3, each for 8 s; opening Runs clears
them. No upstream hook: upstream's `FilmcraftApp::toast` is one string without buttons. **Decision:** cancelled runs,
installs and registry checks do not toast (the canceller knows; installs show under their row). The panel-top status
lines of Plugins, Runs, Agent and Batch (`bashcut.plugins.status`, `bashcut.runs.status`, `bashcut.agent.status`,
`bashcut.batch.status`, `bashcut.batch.error`; RN2) go through `toast::status_line`: a ✕
(`….status.dismiss`) hides the line until its message changes or the status clears and comes back.

Built in #52 (P): `bashcut_ui/palette.rs`, a foreground `egui::Area` drawn at the end of `layout` in every mode.
Cmd/Ctrl+Shift+P (`bashcut.palette.open`, Window ▸ BashCutPro ▸ Command Palette…) opens it; Cmd/Ctrl+K stays
upstream's Add Edit. Items: engine and UI commands (live shortcut and enabled state), `bashcut.*` commands, plugin
actions (`bashcut.plugins.run`), and catalogs read from list commands when they exist: looks (`bashcut.looks.list`),
output profiles (`bashcut.profiles.list`), templates (`bashcut.template.list`); batch presets join `CATALOGS` with one
row once the batch runner lists them. Kit recipes (`vlog-*`) open the Agent section and copy a starting prompt
(`bashcut.palette.recipe`) until the Agent section takes a draft. Enter runs through `menus::invoke` (undoable); Tab,
or Enter on an item with required params, opens a JSON params editor (Cmd/Ctrl+Enter runs). Ranking: substring /
fuzzy score plus recent use (`<data dir>/bashcut/palette.json`). Commands `bashcut.palette.{open {query?}, close,
search {query, limit?}, run {item, params?}, recipe {recipe}}`; automation ids `bashcut.palette`,
`bashcut.palette.input`, `bashcut.palette.item.<key>`, `bashcut.palette.params[.run|.back]`.

Built in #47 (Q): `crates/engine/src/bashcut/looks.rs`, a curated pack of 26 looks, each a set of Lumetri Color
parameters with a one-line purpose, genre tags and one intensity knob (1.0 = as designed, 0 = no change; each look
has a safe range). Commands `bashcut.looks.{list {tags?, query?}, get {look, intensity?}, apply {look, intensity?,
clips?}, remove {clips?}, thumbnail {look, intensity?, width?, frame?, path?}, export {look + path | dir, size?},
check {look?}}`. Applying adds one Lumetri Color marked by a `bashcut_look` text param (Lumetri ignores it), so a
second look replaces the first; one undo step. Thumbnails are upstream's Lumetri on its procedural preview picture
(or the program frame), cached by look fingerprint; `.cube` LUTs are sampled from the same grade. Golden values at
min / default / max intensity live in `looks_golden.json` (bless with `BASHCUT_BLESS=1`), damage checks (clipped
highlights, crushed shadows, skin hue shift, ramp order) in `looks_tests.rs` and `bashcut.looks.check`. The Looks
section (`bashcut_ui/looks_panel.rs`, rail id `looks`) lists them with thumbnails, filter and genre tags, the knob,
"Preview on current frame", Apply / Remove on the selection and favourites (`<data dir>/bashcut/looks-ui.json`;
commands `bashcut.ui.looks.{favourite {look, on?}, previewFrame {on?}, state}`); automation ids
`bashcut.looks.<id>[.apply|.favourite]`, `bashcut.looks.{filter,intensity,frame,remove,tag.<tag>}`. The palette
lists looks through `bashcut.looks.list`.

Built in #68: the Library section (`bashcut_ui/library_panel.rs`, rail id `library`). **Online** runs the resources
plugin's `search` (music, sound effects, photos, video, icons, fonts; nothing downloaded, licence and source shown on
each result) and `download` (the file plus its `.licence.json` record), then imports with `file.import` and places with
`timeline.place {track: "new", audioTrack: "new"}` itself, so it works without the control channel; credit lines go
into the project notes only with the Credits switch on (`<data dir>/bashcut/library-ui.json`, off by default).
**Mine** runs the library plugin's `search`; media items (music, sound, sticker, overlay) are placed by the app, the
rest go through the plugin's `apply` (needs `--control`). Searches and downloads are plugin runs (they show in Runs),
results cached per query (32). Preview opens the preview URL in the browser for now; in-app thumbnails and audio
preview need an HTTP image loader / player (later, or through plugin views #37). Commands
`bashcut.ui.library.{search {tab?, kind?, query}, results, place|add {index}, credits {on?}, state}`; automation ids
`bashcut.library.{tab.online,tab.mine,query,search,credits,kind.<kind>,install}`,
`bashcut.library.online.<n>[.licence|.preview|.source|.add|.place]`, `bashcut.library.mine.<id>[.place|.apply|.favourite]`.

- **UI state** (open section, width, popped-out sections, panel scroll, draft message) is a `bashcut_ui::State`
  kept in egui's context data (`ctx.data_mut(|d| d.get_temp_mut_or_default::<State>(id))`), so `FilmcraftApp`
  gains no field. It is saved to `<data dir>/bashcut/ui.json` when it changes, not in upstream's workspace files.
  The failed runs Runs has shown (for the rail's dot) are kept there too but never saved: job ids restart with the app.
- **Background work** (plugin runs, agent processes) runs on threads owned by `bashcut-plugin-host` /
  `bashcut-agent`; the UI polls their channels each frame and asks for a repaint while anything runs. Nothing
  blocks the UI thread. All plugin and agent output is hostile input: capped sizes, no panics (AGENTS.md §0).
- **Automation ids**: every widget registers in `app.auto` with ids under `bashcut.` (`bashcut.rail.agent`,
  `bashcut.agent.input`, `bashcut.gate.approve`, `bashcut.plugins.<id>.run`), so `ui.inspect` / `ui.click` drive
  them like upstream widgets.
- **Scroll areas** (FX16, FX18): egui's scroll bar floats over the content's right edge, so every scroll area of
  ours goes through `widgets::scroll_clear_of_bar`: while its content is taller than the area, the content ends
  `widgets::scroll_bar_overlap` (the hovered bar's width) short of the right edge, and right-aligned states, ✕
  buttons and full-width cards stay visible and clickable. A section's area registers `bashcut.panel.<id>.scroll`.
  Painted rows (the palette) subtract `scroll_bar_overlap` themselves.
- **Open modal dialogs** (FX14, `bashcut_ui/modal.rs`, hook H14): an egui `Modal` (upstream's Recover Unsaved
  Changes, Revert, Preferences) takes every click outside its own widgets. `ui.click` on an element behind it (by
  `id`, or at `x,y` over such an element) answers `ok: false` with `“Recover Unsaved Changes” is open and takes
  every click: `<id>` is behind it; answer it first: recovery.discard, recovery.recover, recovery.later` instead of
  `ok: true` with nothing happening. The dialog's own buttons already have upstream ids (`recovery.recover`,
  `recovery.discard`, `recovery.later`, `recovery.item.<n>`; `revert.yes|no`; `settings.*`), so no extra hook
  exposes them. A widget registered with its egui response is judged by its layer; the others by the modal's id
  prefix; a modal we do not know only blocks the former. `ui.move`, `ui.drag` and `ui.scroll` are not checked.
- **Strings**: English in code, translations in `bashcut_ui/strings/<lang>.tsv` (Vietnamese first), looked up by
  `bashcut_ui::strings::s` (interface language, or our own override `bashcut.ui.setLanguage {language:"vi"}`).
  Upstream's `.tsv` files are never edited and no i18n hook is needed. `strings.rs` tests guard the catalog: every
  `s("…")` / `f("…")` literal, section, menu label and status word has a row, strings looked up through a variable
  (library kinds, palette kinds, kit recipe titles and summaries) are listed explicitly, and a key may appear only
  once (a later row would silently replace an earlier one). The palette shows recipe titles and summaries in the UI
  language; plugin manifest text uses the manifest's own `i18n` (`plugin-host.md`, incl. choice labels).
  The `bashcut-agent` crate stays English (it has no UI); the Agent panel shows its errors and notices through
  `strings::agent` (VI4): a plain row for fixed messages (`Stopped.`, `nothing to send`, …), else one of the
  `strings::AGENT_MESSAGES` templates for the `format!` ones (`cannot start `{program}`: {error}`, `the agent
  exited with status {code}: {output}`, …; a nested `{error}` is translated too). The agent's own words (its
  stderr, a sign-in message, an error event) and OS error text stay as written. A new message in the crate needs
  its row or template.
- **Theme and icons**: upstream `Tokens` (dark/light follow upstream); our icons are drawn in `bashcut_ui` (no
  edit to upstream's `icons.rs` enum).

## 5. Project data

Our per-project data goes in a sidecar folder next to the project, `<name>.fcproj.bashcut/`:
`conversations/`, `plan.json` (edit plan, gates and their state), `runs/` (plugin results and logs). The `.fcproj`
format is not changed. What must be visible in FilmCraft's own panels is written through commands as markers,
labels or subclips (§1 rule 1). A project opened in upstream FilmCraft still works; it ignores the sidecar.

## 6. Agent panel in detail

- **Start**: user picks an agent: Claude Code (`claude`, or `BASHCUT_CLAUDE`) or an agent plugin (`kind: "agent"`
  in `bashcut-plugin.json`, with an `agent` section: `args`, `mcpConfig` (`{config}`, or `{mcpCommand}`/`{mcpArgs}`),
  `resume` (`{session}`), `prompt`, `timeoutSecs` and `events`: `claude-stream-json` or `codex-json`, the rows of
  `bashcut_agent::format::FORMATS`, RC6; see [plugin-host.md](plugin-host.md)). The app writes a temporary MCP config for
  `<engine> mcp --bridge 127.0.0.1:<control port>` (`bashcutpro-cli`, else `bcpro` or `filmcraft-cli`) and starts the agent with the kit's skills available; with the
  control channel off (CC1: on by default) an agent that needs the app says so instead of starting.
- **The server calls itself BashCutPro (BR4)**: upstream's MCP server (`filmcraft_automation::FilmcraftMcp`) answers
  `initialize` with `serverInfo.name = "filmcraft"` and "FilmCraft video editor" in `instructions`, and two tool
  results name FilmCraft (the `ui_screenshot` note, the "drives the live app" hint); all of it goes into the agent's
  context. `bashcutpro-cli mcp` (and `bcpro mcp`) runs that server through `apps/bashcutpro-cli/src/mcp_brand.rs`,
  which rewrites the outgoing lines: `serverInfo.name` `bashcutpro`, "FilmCraft" → "BashCutPro" in `instructions` and
  in every tool `title`/`description` of `tools/list`, and the exact texts in `src/mcp_texts.rs`; everything else
  passes through unchanged. Tool names, `filmcraft://` resource URIs and command ids are a protocol and keep upstream's
  spelling. No upstream edit: `build.rs` swaps the `serve_stdio()` call by anchor and fails the build when
  `crates/automation/src/server.rs` grows a FilmCraft string outside that list or loses one in it (tests and comments
  aside). `filmcraft-cli mcp` (the last engine fallback) still says FilmCraft.
- **Command labels without FilmCraft (BR5)**: the engine's command catalog has user text naming upstream (the label
  "Selection as FilmCraft Project…"; our own `bashcut.profiles.apply` params help now says `bashcutpro-cli export
  --preset` at the source). `mcp_brand::brand_commands` rewrites any `command.list` result: "FilmCraft" → "BashCutPro"
  in each command's `label`, `menu` and `params`, `filmcraft-cli` → `bashcutpro-cli` in `params`; ids stay. The MCP
  server applies it to `command_list`, `command_run command.list` and the `filmcraft://commands` resource (a text item
  that parses as a list of commands); `bashcutpro-cli commands`, `describe` and `exec command.list` apply it through two
  `build.rs` anchors, and `build.rs` fails when upstream's server stops sending the catalog the way `MCP_CATALOG`
  expects. The command palette shows engine and UI command labels through `bashcut_brand::text` (the BR2/BR3
  overrides, with their Vietnamese rows), so it says "Selection as BashCutPro Project…" and "BashCutPro Help…" too. The control channel's
  `engine.commands` (scripts, tests, the web API) gives the same English list as MCP: `bashcut_brand::commands`, through
  the one-line `bashcut_brand::engine_commands!` hook at the top of `control::handle` (BR6); `bashcutpro-cli` can't share the function (it doesn't depend on `ui-egui`),
  so the two copies follow the same rule.
- **MCP server and permissions (AG7, checked with `claude` 2.1.309 on macOS)**: the config names the server
  `bashcutpro` (`bashcut_agent::process::SERVER`), the name the agent kit's `bcp` plugin and `/bcp:setup` use, so
  Claude Code's tools are `mcp__bashcutpro__*` (Codex: `-c mcp_servers.bashcutpro.*`). Claude Code also gets
  `--strict-mcp-config`: in a real `claude -p` run with `--plugin-dir <kit>` the `system/init` event listed the
  kit's own server as `plugin:bcp:bashcutpro` (tools `mcp__plugin_bcp_bashcutpro__*`) next to ours (`bashcutpro`,
  source `dynamic`), plus the user's claude.ai connectors; with the flag only `bashcutpro` is left, while the
  `bcp:*` skills and every plugin still load (`plugins`, `skills`). The user's other MCP servers are not offered to
  the in-app agent; that is the price of one copy of the tools. No `--bare` (it skips the subscription login and
  plugin discovery). Allowed without a prompt (`process::allowed_tools`): `mcp__bashcutpro` (every tool of
  the app's server) and only the read-only checks `/bcp:setup` runs (AK2): `Bash(command -v *)`,
  `Bash(printenv BASHCUTPRO_CLI *)`, `Bash(<engine> commands *)` for `bashcutpro-cli`, `bcpro` and `filmcraft-cli`
  (an absolute path to the engine does not match and is denied), `Read(~/.config/bashcutpro/mcp.env)` and
  `Read(//<data dir>/bashcut/automation.json)` (macOS / Linux; a Windows path has no rule form). Not installs,
  builds, `claude mcp add` or writing `mcp.env`; no bypass mode: anything else that needs approval is asked in
  the panel (AG8 below; without a host to ask, `touch` landed in `permission_denials`, the rules above run). The `system/init` event also
  carries `capabilities` (`interrupt_receipt_v1`, `interrupt_cancel_queued_v1`, …), `tools`, `mcp_servers` and
  `plugins`.
- **`BASHCUT_IN_APP_AGENT=1`** (next to `BASHCUT_AGENT_HOST=1`) is in every in-app agent child's environment. What
  the kit's launcher (`scripts/bashcutpro-mcp.sh`, AK3) should do with it: when set, exit 0 at once without
  starting an engine, printing one line to stderr ("the app passes its own bashcutpro server"). With
  `--strict-mcp-config` Claude Code does not start the plugin's server at all, so this only matters for agents
  started without it (agent plugins, a user's own `BASHCUT_CLAUDE` wrapper). Codex: `install-codex.sh` registers
  `bashcutpro` in Codex's own config and our `-c mcp_servers.bashcutpro.*` overrides that same key, so Codex also
  sees one copy.
- **Working directory**: the project's folder, or `<data dir>/agent` (`~/Library/Application Support/BashCutPro/agent`
  on macOS) for an unsaved project (and for a bare project file name without a folder), never the app's folder or a checkout (a BashCutPro checkout's `.mcp.json` adds
  `filmcraft` servers for development). Claude Code keeps its sessions per folder, so `--resume` finds them there.
- **Batch agent steps** (`crates/bashcut-batch/src/agent.rs`) use the same server name and add
  `--strict-mcp-config --permission-prompts none`: unattended, a call that would prompt is denied at once (checked:
  it shows up in the `result`'s `permission_denials`) instead of waiting.
- **Live session (AG2, checked with `claude` 2.1.309 on macOS)**: a format whose `FORMATS` row is `live`
  (`claude-stream-json`) keeps **one child process per conversation** (`process::Live`): stdin stays open and each
  message (plus a short context line: project, active sequence, selection, playhead) is one more stream-json line;
  the end of a turn is the `result` event, not the exit. Checked: two messages written 0.5 s apart give two
  `result`s from one process (a fresh `system/init` before each turn, same `session_id`), and the process stays
  alive until stdin closes (then exits 0 in about a second). Claude Code runs with `--replay-user-messages`: when
  it takes a message from stdin it echoes it as a `user` line with `"isReplay": true` (`Event::Picked`).
  **Send never blocks**: a message sent while Claude works is written at once and shown under the transcript as
  "You · queued" (`bashcut.agent.queued.<n>`, `bashcut.agent.state` → `queued`) until its echo arrives, then it
  joins the transcript (an agent without the echo: at its `result`). The `system/init` `capabilities` are kept on
  the session (`Agent::capabilities`, in `bashcut.agent.state`) for AG3/AG4. **Restart**: a process that exits
  (crash, app restart, idle close) is started again with `--resume <session_id>` by the next message; one that
  exits 0 before taking a written message is restarted once at once and the message written again. A failure
  mid turn (it exits after taking a message and before its `result`) or with messages unanswered shows the error and puts the queued messages it never read back into the message
  box. **Too old**: when Claude Code refuses a flag (`unknown option`), the readiness line says "Update Claude
  Code: run `claude update`" (en+vi). **Session end**: changing conversation, agent or project, hiding the Agent
  section while idle, `session::IDLE_CLOSE` (30 min) idle, and app exit close stdin; the process gets 2 s, then SIGINT,
  then its process group is killed 5 s later (`Agent::shutdown` waits up to 3 s). Codex keeps the per-message
  path (`process::Turn`, `live: false`: its CLI reads the prompt to EOF; Send is disabled while it works).
  Edits the agent makes appear in History as normal commands, so Undo works. Tool calls with the same command
  fold into one row ("edit.razor ×12"). Enter sends; Shift+Enter starts a new line.
- **Conversations**: kept in `<project>.fcproj.bashcut/conversations/<id>.json` (`bashcut-conversation/1`: the
  transcript and the agent's session id), listed newest first in the picker; an unsaved project keeps none.
- **Gates**: the edit-workflow skill writes gates into `plan.json` (`{"gates": [{stage, summary, evidence, state}]}`,
  `state` `pending` / `approved` / `changes`, other keys kept); the panel shows the first pending one as a card.
  Approve / Ask for changes (a note is required) writes `state` and `note` back and sends the answer as the next
  user message. The answer is written only when the turn can start.
- **Stop (AG3, checked with `claude` 2.1.309 on macOS)**: a live agent whose `system/init` `capabilities` has
  `interrupt_receipt_v1` gets the stream-json **interrupt control request** on stdin and **keeps its process**:
  `{"type":"control_request","request_id":"bashcut-stop-<n>","request":{"subtype":"interrupt","cancel_queued":true}}`
  (`stream_json::interrupt_request`; `cancel_queued` only with `interrupt_cancel_queued_v1`). Checked during a
  60 s foreground Bash call: the ack `{"type":"control_response","response":{"subtype":"success","request_id":…,
  "response":{"still_queued":[],"cancelled":[]}}}` (`Event::Control`) arrives within milliseconds, then the tool
  result "The user doesn't want to proceed…", the text `[Request interrupted by user for tool use]` and a `result`
  with subtype `error_during_execution` (not shown); the process stays and the next message is answered in the
  same session. Without `cancel_queued` a message written before the interrupt still runs after it; with it, it
  never runs (no `isReplay` echo), so queued messages go back into the message box. The transcript says "Stopped."
  (`Đã dừng.`) and status is Idle at that `result`, never at the ack: the `isReplay` echo of a message can come
  only with the turn's first output, after the ack (seen in the app: a long reply with no tool call). **Fallback: SIGINT** (not
  SIGTERM, which drops the turn with exit 143 and no result) when the capability is missing, when written queued
  messages wait and `cancel_queued` is not offered (since AG4 such messages are held in the app and not written, so
  the request is sent without `cancel_queued` and the held ones go back into the message box), when the ack is an error or does not come within 3 s
  (`session::ACK_WAIT`), when no `result` follows within 5 s (`RESULT_WAIT`), and on a second Stop. Checked (AG2):
  Claude Code then echoes `[Request interrupted by user]`, sends the same error `result` and exits 0; the next
  message resumes the session in a new process; not exiting within 5 s kills its process group. A per-message turn
  (Codex) ends its process group as before; Batch agent steps (`bashcut-batch`) run their own per-step process and
  keep their stop. The project stays as it is. Tests: `crates/bashcut-agent/tests/fake_agent.rs` drives all of this
  with a scripted live stream-json fake (`LIVE_FAKE`, TA2).
- **Queued messages with ✕ (AG4, checked with `claude` 2.1.309 on macOS)**: queued messages are numbered in the
  order the agent gets them ("You · queued 1", `Bạn · đang chờ 1`, …) and each has ✕
  (`bashcut.agent.queued.<n>.cancel`; command `bashcut.agent.cancel {index}`, 0-based in `bashcut.agent.state` →
  `queued`; `queuedItems` adds `cancelling`). A live agent whose `system/init` has `interrupt_cancel_queued_v1`
  gets every message with a `uuid` (`stream_json::user_message_with_uuid`), written at once as in AG2, and ✕ sends
  `{"type":"control_request","request_id":"bashcut-cancel-<n>","request":{"subtype":"cancel_async_message",
  "message_uuid":"<uuid>"}}` (`stream_json::cancel_request`; the item shows "Removing…" until the answer). Checked:
  three messages during a 15 s Bash call, ✕ on the second: `{"response":{"cancelled":true}}` within milliseconds,
  the second never runs (no `isReplay` echo); an unknown uuid answers `cancelled: false`. The `isReplay` echo
  carries our `uuid`, so `Event::Picked {uuid}` takes exactly that message. `cancelled: false` (already taken, or
  folded into the running turn: Claude Code can take a queued message into the turn under way at a tool boundary,
  seen in the same check for the third message) keeps the item until its echo; it is answered. 2.1.309 has no
  capability named for `cancel_async_message` itself; its schema says a client that wants per-message control
  follows the `interrupt_cancel_queued_v1` interrupt with it, so that capability gates it. **Any other live agent**
  (no such capability, e.g. an agent plugin speaking stream-json, or a Claude Code whose `system/init` has not
  arrived yet for a message sent in its first second) gets a queued message only when the turn before it ends:
  the panel keeps it (`Pending::held`) and writes the oldest at the `result`, so ✕ just drops it and each runs as
  its own turn (no mid-turn pickup for those agents). **Stop with a queue**: Stop does not ask; it drops the whole
  queue (`cancel_queued: true`, held ones never written) and puts the queued text back into the message box (AG3
  behaviour, kept: nothing typed is lost, and one click stops everything). Claude Code merges messages it takes
  together into one turn; a message cancelled before that never reaches it.
- **Permission requests answered in the panel (AG8, checked with `claude` 2.1.309 on macOS)**: Claude Code runs with
  `--permission-prompt-tool stdio` (hidden in `--help`; `--permission-prompts` defaults to `host`), the stream-json
  control protocol the Agent SDK uses for `canUseTool`. Chosen over an MCP permission tool on our `bashcut-mcp` /
  control channel: no extra server, tool or round trip through the engine, the request arrives on the stdout we
  already read and the answer goes on the stdin we already write, and it carries Claude Code's own rule
  suggestions. Checked: `touch ag8-probe.txt` (not in the AG7 allow list) gives
  `{"type":"control_request","request_id":"<uuid>","request":{"subtype":"can_use_tool","tool_name":"Bash",
  "display_name":"Bash","input":{"command":"touch ag8-probe.txt","description":"…"},"description":"…",
  "permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch ag8-probe.txt"}],
  "behavior":"allow","destination":"localSettings"},{"type":"addDirectories",…},{"type":"setMode",…}],
  "blocked_path":"…","tool_use_id":"toolu_…"}}` and the agent waits. Answer (`stream_json::permission_response`):
  `{"type":"control_response","response":{"subtype":"success","request_id":<id>,"response":{"behavior":"allow",
  "updatedInput":<input as sent>}}}` runs the command (the file appeared); `{"behavior":"deny","message":"The user
  denied this in BashCutPro."}` puts the call in the `result`'s `permission_denials` and the model goes on;
  **Always** adds `"updatedPermissions":[{"type":"addRules","rules":<the request's addRules/allow suggestions>,
  "behavior":"allow","destination":"localSettings"}]` and Claude Code itself wrote
  `{"permissions":{"allow":["Bash(touch ag8-probe.txt)"]}}` to `<cwd>/.claude/settings.local.json`. We always pass
  `localSettings` (the turn's folder = the project's folder, never `userSettings`), only the suggested `addRules`
  rules (never `addDirectories` / `setMode`), and offer Always only for a saved project with a suggestion. The
  request becomes `Event::Permission`; the session keeps it (`Agent::permissions`, `bashcut.agent.state` →
  `permissions: [{tool, detail, always}]`) and the panel shows a card at the end of the transcript ("{agent} wants
  to use {tool}", the command / file / URL / path, else the description) with Allow once / Always for this project /
  Deny (`bashcut.agent.permission.<n>.once|always|deny`; command `bashcut.agent.permission {index, answer:
  once|always|deny}`). The answer stays as one transcript line (`Item::Permission`, `allowed` / `always` / `denied`
  / `withdrawn`). **Nothing hangs**: Stop first denies waiting requests ("The user stopped the turn…") then
  interrupts; a `control_cancel_request` (`Event::PermissionWithdrawn`), the turn's `result` and the process's end
  drop the card ("not answered"); more than 16 waiting requests, or an input over 256 KB, are denied at once; the
  30 s "no output" hint is not shown while a card waits (the turn timeout still applies). Codex and Batch are
  unchanged: Batch agent steps keep `--permission-prompts none`.
- **Sign-in problem in a live session (AG3 for AG6)**: after an `Event::Auth` the live process is closed at its
  `result`, so the next message starts a new one with the login as it is then; its non-zero exit adds no generic
  error under the sign-in line.
- **One writer per session (AG3)**: Open in Terminal resumes the same session id, so the panel first ends its own
  agent (`Agent::release`: Stop a running turn, close stdin, wait for the process to exit; "Ending the in-app
  session before the terminal opens…", `bashcut.agent.handoff`), then opens the terminal and adds "Continued in
  Terminal." (`Đã chuyển sang tiếp tục trong Terminal.`) when there was a session. A later Send in the panel resumes
  the session again; closing the terminal first is the user's call.
- **Hidden panel, driven over the control channel (AG3)**: a `bashcut.agent.*` command while the Agent section is
  hidden marks the agent as driven (`Agent::driven`): the hidden panel no longer closes it after each turn, so a
  control-channel or MCP client keeps one live process; `IDLE_CLOSE` still applies, and showing the section clears
  the mark.
- **Plugin hand-offs**: Runs ▸ Send to agent (`bashcut.plugins.sendToAgent`) sends the run's result and the
  plugin's `handoff.then` as the next message.
- **Agent replies (UX8)** render minimal Markdown (`bashcut_ui/markdown.rs`): `#`/`##` headings, `-`/`1.` lists,
  `**bold**`, `*em*`, inline code, fenced code in a monospace frame; replies past 256 KB are cut with "…".
- **Empty transcript (UX8)**: three kit recipes (Plan a vlog, Hook and script, Finish for posting) as example
  prompts; a click fills the message box with `Use the bcp:<id> recipe on this project.` and does not send. Under
  them: "The agent stops at each stage for your approval." Automation ids `bashcut.agent.example.<recipe id>`.
- **Commands**: `bashcut.agent.{send, stop, cancel, state, transcript, conversations, open, new, agents, use, gate, approve,
  change, terminal, signIn, install}`; automation ids `bashcut.agent.{input, send, stop, picker, conversations, new, transcript}`,
  `bashcut.gate.{approve, change, note}`.
- **Terminal (AG5, `bashcut_ui/terminal.rs`)**: under the header, **Open in Terminal** (automation id
  `bashcut.agent.terminal`; Claude Code and Codex when their program is found) opens the OS terminal in the turns'
  folder (`bashcut_agent::session::turn_dir`: the project's, else `<data dir>/agent`, so `--resume` finds the session) running the panel's program plus its
  resume arguments (`claude --resume <id>`; a session id other than `[A-Za-z0-9_-]` is dropped). **Sign in…**
  (`bashcut.agent.signIn`, Claude Code only; `agent_panel::sign_in_button` for AG6's auth-failure line) runs `claude`
  after the hint "Type /login…". **Install** (`bashcut.agent.install`) sits on the "Claude Code not found" line and runs
  the official installer (`curl -fsSL https://claude.ai/install.sh | bash`; Windows `irm https://claude.ai/install.ps1 |
  iex`), only on a click. The app never reads, stores or passes Claude credentials. Launchers, first that starts wins:
  macOS `osascript` telling Terminal to `do script` (the shell line is `item 1 of argv`, not AppleScript source);
  Windows `wt.exe -d . powershell.exe -NoLogo -NoExit -EncodedCommand <base64 UTF-16LE>`, else `cmd.exe /c start ""
  powershell.exe …` (raw args); Linux `x-terminal-emulator -e sh -c`, `gnome-terminal --working-directory=… -- sh -c`,
  `konsole --workdir … -e sh -c`, `xterm -e sh -c`, ending in `exec "$SHELL"` so the window stays. The folder is the
  child's working directory (plus a single-quoted `cd` in POSIX lines); no project text is parsed by a shell unquoted.
  Verified on 2026-10-23 against Claude Code 2.1.309 (`claude --help`: `-r, --resume [value]`; `/login` inside the
  interactive UI). The terminal session gets no `--mcp-config` (the panel's temp config is private to
  `bashcut-agent`); it uses the agent kit's `bashcutpro` server, except when the kit is missing (AG9, below).
- **Terminal commands (AG10)**: the three terminal buttons are also engine/palette commands with the same ids as
  their automation ids, and the buttons call the same functions in `bashcut_ui/agent.rs`: `bashcut.agent.terminal`
  "Open Agent in Terminal" (`agent::open_in_terminal`: Claude Code or Codex found, else an error; returns
  `{opened, pending}`), `bashcut.agent.signIn` "Sign In to Claude Code" (`agent::sign_in`; current agent Claude
  Code, found) and `bashcut.agent.install` "Install Claude Code" (`agent::install`; only when Claude Code is not
  found, else "Claude Code is already installed."; still only on this explicit command or the click, never
  automatic). Open in Terminal is a hand-off kept in egui context data: it opens at once when no turn runs, else
  `agent::poll` (every frame, panel shown or not) opens it once the in-app agent has ended; meanwhile the panel
  shows "Ending the in-app session…", and an error opening the window goes to the status bar. `bashcut.agent.agents`
  lists each agent's `actions` now (`terminal`, `signIn` or `install`). Palette labels of `bashcut.*` commands go
  through the string catalog (`strings::s`), so these three show in Vietnamese ("Mở trợ lý trong Terminal",
  "Đăng nhập Claude Code", "Cài Claude Code"); MCP `command_list` keeps the English labels. Switching agents
  (`use`) resets `Agent::plugins` to unknown, so the kit line never shows from the other agent's init.
- **installKit and Vietnamese palette labels (AG11)**: `bashcut.agent.installKit` "Install BashCutPro Kit" runs
  `agent::install_kit`, the same function as the kit line's **Install kit…** button (AG9): current agent Claude Code,
  found (else "Install kit works with Claude Code once it is installed."); refused with "The BashCutPro kit is
  already installed." once the session's last `system/init` lists `bcp`. Before any init (plugins unknown) the
  explicit command still opens the kit terminal: the user asked for it, and the two `claude plugin` commands are
  harmless when the kit is there. Only on this command or the click, never automatic. Every `bashcut.agent.*`
  palette label has a vi row ("Gửi cho trợ lý", "Dừng trợ lý", "Cài bộ công cụ BashCutPro", …); MCP
  `command_list` keeps the English labels.
- **Kit missing (AG9)**: `stream_json` keeps the `name`s of `system/init`'s `plugins` (`Event::Init.plugins`,
  `Agent::plugins`: `None` until a live session sent one; also in `bashcut.agent.state`). Checked on 2026-10-23
  with Claude Code 2.1.309 on Windows: `plugins` is `[{"name", "path", "source", "version"}]`, builtin ones with
  `path: "builtin"`; the kit shows as `name: "bcp"`. No file in `~/.claude` is read. When Claude Code's last init
  has no `bcp`, the readiness line `bashcut.agent.ready.kit` says "BashCutPro skills are not installed", shows the
  agent-kit README's two commands selectable (`bashcut.agent.ready.kit.commands`) and **Install kit…**
  (`bashcut.agent.installKit`), which opens a terminal running `<claude> plugin marketplace add
  dongnguyenvie/bashcutpro-agent-kit` then, when that worked, `<claude> plugin install bcp@bashcutpro-agent-kit`
  (`terminal::kit_job`, the program quoted). Never automatic. Before the first init nothing shows (unknown is not
  missing). While the kit is missing, **Open in Terminal** adds `--mcp-config <data dir>/agent/terminal-mcp.json`
  (`terminal::terminal_mcp_config`: the app's `bashcutpro` server, `<engine> mcp --bridge <address>`, rewritten on
  each click), so the terminal agent reaches the app without the kit; with the kit it is not passed (two copies
  of the same tools). The empty transcript's recipe buttons say on hover that they need the kit.
- **Not signed in (AG6)**: `stream_json.rs` turns sign-in and account problems into `Event::Auth {category,
  message}`, read from the event stream only (credential files are never looked at). Categories
  (`stream_json::AUTH_ERRORS`): `authentication_failed` (needs `/login`), `oauth_org_not_allowed`, `billing_error`.
  Checked on 2026-10-23 with Claude Code 2.1.309 on Windows and an empty `CLAUDE_CONFIG_DIR`: with no login `claude
  -p` sends `system/init` (`apiKeySource: "none"`), then a synthetic `assistant` message (`model: "<synthetic>"`)
  with top-level `"error": "authentication_failed"` and the text `Not logged in · Please run /login`, then a `result`
  with `is_error: true`, `subtype: "success"`, `terminal_reason: "api_error"` and the same text; no `api_retry`.
  Decoded: that `assistant` message, `system/api_retry` with one of the categories in `error` (a login rejected
  mid-turn, per the docs), and a failed `result` whose `error` is a category or whose text says "Not logged in",
  "Please run /login" or "Invalid API key" (Auth first, then the `Result`). The transcript keeps one
  `Item::Auth` per problem (repeats fold, no generic error after it); `Transcript::auth_problem` is the last one
  when no message was sent since. The panel then shows one readiness line, `bashcut.agent.ready.auth`: "Claude
  Code is not signed in" or "Claude Code account problem: billing" / "…: this organization is not allowed", with
  **Sign in…** (`sign_in_button`, moved off the terminal row while the line shows) and the agent's message as
  selectable text (`bashcut.agent.ready.auth.message`). The next Send clears it.
- **Phase 1 adapters**: Claude Code (`claude -p --output-format stream-json --input-format stream-json`) and Codex
  (`codex exec --json -` with the prompt on stdin, `resume <thread>` for later turns, the MCP server passed as `-c
  mcp_servers.bashcutpro.*`; `codex`, or `BASHCUT_CODEX`; events `codex-json`, #37); others as plugins.

### 6a. Batch section in detail (#46)

- **Build**: inputs (drop zone, Add files…, Add folder…; drops on the zone are taken by hook H12 before upstream's
  import sees them), steps (search over engine commands, plugin actions and kit recipes; a plugin step opens its action's form, the
  Plugins section's fields with labels in the UI language, read back from its command line and rebuilt by `setStep`
  on Apply; any other step opens its JSON under the command's signature, and Apply stays disabled while it does not
  parse),
  outputs (output profiles, exported with their loudness and caption rules, or export presets), name, items at once, on failure
  go on / stop, projects folder (default `batch/` next to the first input). Plugin steps keep the action's command
  line with absolute program paths and its non-secret settings as environment; secrets never go into a pipeline.
- **Start from** (UX12): the first row, "Start from: [preset / recent ▾]", loads a preset or opens a recent job
  (`bashcut.batch.startFrom` with `preset` or `job`); recent entries read "name · date (UTC) · n done / m failed",
  the counts kept in `batch.json` when a job finishes or is opened. Inputs show "n files · Clear all"
  (`bashcut.batch.clearInputs`; `clear` resets the whole draft) and a ✕ after each name; a chosen projects folder has
  a ✕ back to the default.
- **Start from elsewhere**: "Run on many files…" on a plugin action (Plugins section) and on an agent tool row
  prefill a draft with that step (`bashcut.batch.prefill`) and open the section.
- **Run**: `bashcut_batch::run` on a background thread (under `catch_unwind`) with the app's data folder; the panel
  re-reads the job book every 500 ms. Per item: status, the running step, a progress bar weighted by learned step
  times (kept in `<data dir>/bashcut/batch.json` with recent jobs), errors, outputs, **Open to fix / Retry / Reveal**.
  Retry failed, Cancel, Reveal folder for the whole batch. Presets save the draft as a `bashcut-batch` preset.
- **Commands**: `bashcut.batch.{state, draft, clear, addInput, removeInput, clearInputs, addStep, prefill, setStep, removeStep,
  moveStep, addOutput, removeOutput, set, run, cancel, retry, open, reveal, presets, savePreset, loadPreset,
  deletePreset, recent, openJob, startFrom, durations}`; `run` / `retry` take `cli` (default: `filmcraft-cli` next to the app).
- **Not yet**: output thumbnails (outputs are listed by file name).

## 7. Plugin views

`bashcut-view/1` is declarative JSON the plugin prints (initially) and updates (one JSON object per line on
stdout while a `view` action runs): rows of `text`, `input`, `select`, `toggle`, `button` (→ action), `list` (with
per-row buttons), `image` (a file path), `progress`. `view.rs` renders them with egui widgets and our theme.
Webviews are out: they would add an OS webview dependency to a pure-Rust app and a large attack surface; BashCut's
own research (spec 14) reached the same conclusion for most needs.

Built in #37 (U4): the model and rules are in [plugin-host.md](plugin-host.md) §Views. `bashcut_ui/view.rs` draws the
form at the end of the plugin card (the card opens when a new view arrives); a typed value stays until the plugin
sends a different value for that field. Running a `view` action opens the Plugins section. Images
are decoded off the PNG header with limits and cached by path, size and mtime. Automation ids
`bashcut.view.<plugin>.title`, `.row<n>`, `.<id>` (fields and buttons), `.<list>.<item>[.<button>]`. Port of the
translate-captions review list to a view is a follow-up in `bashcutpro-plugins`.

Pop-out windows (#37) remember their rect per section and monitor size (`windows` in `<data dir>/bashcut/ui.json`,
key `<section>@<w>x<h>`, at most 64 entries), so a window comes back where it was on that monitor.

## 8. Phases

| Phase | Ships | Hooks used |
|---|---|---|
| U0 | `bashcut_ui` skeleton: rail + empty side panel, state, strings, tests; `xtask` layers | H4–H6, H10, H11 |
| U1 | Plugins + Runs sections (P1 of #31), `bashcut.plugins.*` commands | H7–H9 |
| U2 | Agent section with the Claude Code adapter, gates from `plan.json` (done, #35: `crates/bashcut-agent`, `bashcut_ui/{agent,agent_panel}.rs`) | H4 |
| U3 | `bashcut-analysis` + engine commands (loudness, beats, picture checks) | H1–H3 |
| U4 | Plugin views (`bashcut-view/1`), Codex adapter, pop-out windows per monitor (done, #37: `bashcut_ui/view.rs`, `bashcut-plugin-host/src/view.rs`, `bashcut-agent/src/codex_json.rs`) | — |
| B | Batch runner #45 → Batch section #46 (§6a; `bashcut_ui/{batch,batch_panel}.rs`); templates #42, profiles #43 | H1–H3 (templates), H4, H12 |
| Q | Looks #47, voice polish #48, word captions #49 (design note first) | H1–H3; #49 may add one render hook |
| P | Command palette #52 | — (inside H6 / H8) |

Each phase: `cargo test -p` for our crates and `crates/ui-egui/tests/bashcut_ui.rs`, `cargo xtask layers`,
one manual look at the app through `ui.screenshot`. No release build per phase.

**Pixel diff of a refactor (T8).** `panels_render_for_a_pixel_diff` in `bashcut_ui.rs` is `#[ignore]` and only
renders: with `BASHCUT_UI_SHOTS=<dir>` it saves Agent (a tool group open), a failed run's toast, Runs (error + log,
docked and popped out), Batch's steps, the palette's parameters and the rail tooltip, each in en and vi. The data dir
has a fixed name, so two runs at the same commit match to the pixel. To check that a refactor draws the same: add a
temporary worktree at the old commit, append this test to its `bashcut_ui.rs` (it uses only the `Driver` of that
era), run it there and on the new commit with the same `CARGO_TARGET_DIR`, and compare the PNGs (no image goes in
the repo). Run 2026-10-23: RU17's parent (`03222fbb^1`), RU17 (`03222fbb`) and main (`0a77bf93`, again at `828dcd98`) gave 0 differing
pixels in all 16 shots; a 0.5 px change to `CODE` showed up as ~15 000 px in the Agent tool group, so the pass does
see a change.

## 9. Upstream sync checklist (for these hooks)

1. `bashcut/scripts/upstream.sh merge` (see `UPSTREAM.md` "How to sync") on `upstream-sync/<date>` with `git rerere` on.
2. Conflicts should only be in the H-lines; re-place a hook if upstream moved the function (search for
   `// BashCutPro`).
3. `cargo check --workspace`, `cargo test -p bashcut-plugin-host -p bashcut-agent -p bashcut-analysis`,
   `cargo test -p filmcraft-ui-egui --test bashcut_ui`, `cargo xtask layers`.
4. Update `UPSTREAM.md` (row for the merge, hook list unchanged or edited).

## 10. Changes to earlier decisions

- [plugin-host.md](plugin-host.md) approved a **floating window** for P1–P2. This design keeps that as the pop-out
  mode but makes the default a **side panel reserved by hook H6**, which still adds no `PanelKind`. Accepted
  2026-10-20 (decisions.md).
- The plugin host's touch points in plugin-host.md (`lib.rs` per-frame call, `menus.rs` routing) are H5–H9 here.
