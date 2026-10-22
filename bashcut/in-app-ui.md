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
| `crates/bashcut-agent/` | L4 (no workspace deps) | agent sessions: start an agent CLI as a child process with an MCP config pointing at the app (`filmcraft-cli mcp --bridge 127.0.0.1:<control port>`), parse its event stream into a transcript model (message, tool call, gate, error), stop, resume; conversations saved in the project sidecar (§5). Adapters per agent (Claude Code `stream-json`, Codex) behind one trait. |
| `crates/bashcut-batch/`, `apps/bashcut-batch/` | L4 (no workspace deps) / app | batch pipelines, parallel headless items through `filmcraft-cli`, job book, presets (#45). `apps/*` glob: no upstream edit. |
| `crates/bashcut-analysis/` | L3 | read-only measurements for the engine gaps in [skill-needs.md](skill-needs.md): loudness (wraps `filmcraft-audio-dsp` BS.1770), beats / tempo (wraps `audio-dsp`'s remix onset analysis), black / frozen frames (its own frame-difference math over frames the engine adapter decodes). Depends only on L0–L1 crates (`time`, `frame`, `audio-dsp`), so the engine (L4) can depend on it without a same-layer edge; pure functions, wasm-safe. |
| `crates/engine/src/bashcut/` | (engine) | `mod.rs` with `commands() -> Vec<CommandSpec>`: `bashcut.audio.loudness`, `bashcut.audio.beats`, `bashcut.review.pictureChecks`, … They read the `Session`, call `bashcut-analysis`, return JSON. Headless MCP gets them for free because they are engine commands. |
| `crates/ui-egui/src/bashcut_ui/` | L5 | `mod.rs` (the hooks' entry points), `rail.rs`, `side_panel.rs`, `agent_panel.rs`, `plugins_panel.rs`, `runs_panel.rs`, `batch_panel.rs`, `looks_panel.rs`, `palette.rs`, `view.rs` (renders `bashcut-view/1` with egui widgets), `state.rs`, `strings/` (our i18n). Uses upstream's `Tokens`, `widgets` and automation registry read-only. |
| `crates/ui-egui/tests/bashcut_ui.rs` | test | headless scripted UI tests for the rail and panels (new file). |
| `xtask/src/bashcut.rs` | tool | the layers of our crates (`bashcut-analysis` 3, `bashcut-plugin-host` 4, `bashcut-agent` 4), so `cargo xtask layers` knows them; the plugin host and agent depend on no workspace crate, and only `ui-egui` (L5) uses them. |

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
| H11 | `xtask/src/main.rs` | `mod bashcut;` and, as the first line of `layer_of`: `bashcut::our_layer_first!(name);` (macro, as H7) | 2 changes |

About **16 lines in 8 upstream files**, all marked `// BashCutPro`, all listed in `UPSTREAM.md`.
`Cargo.lock` conflicts are resolved by taking upstream's file and running `cargo check` (it re-adds our crates).

What `bashcut_ui::layout` does in H6, in order: read our state; if the rail is hidden (setting or Import/Export
mode) return `body` unchanged; draw the rail at the right edge of `body`; if a section is open, draw the side panel
left of it; draw floating (popped-out) sections as `egui::Window`s and toasts as foreground `egui::Area`s; return
the shrunk rect for the dock. One call per frame covers docked, floating and overlay UI.

## 4. State, settings, automation, strings

Built in #34: `bashcut_ui/{mod,rail,side_panel,state,strings}.rs`. Sections are rows of `bashcut_ui::SECTIONS`
(id, title, icon, blurb, `draw` fn); a later ticket adds its row and a `draw` in its own file. Commands so far:
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
`--bridge 127.0.0.1:<port>` when the app was started with `--control <port>` (or `FILMCRAFT_CONTROL_PORT`), and get
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
or "Could not check …", with the registry location as its tooltip.

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
- **Strings**: English in code, translations in `bashcut_ui/strings/<lang>.tsv` (Vietnamese first), looked up by
  `bashcut_ui::strings::s` (interface language, or our own override `bashcut.ui.setLanguage {language:"vi"}`).
  Upstream's `.tsv` files are never edited and no i18n hook is needed. `strings.rs` tests guard the catalog: every
  `s("…")` / `f("…")` literal, section, menu label and status word has a row, strings looked up through a variable
  (library kinds, palette kinds, kit recipe titles and summaries) are listed explicitly, and a key may appear only
  once (a later row would silently replace an earlier one). The palette shows recipe titles and summaries in the UI
  language; plugin manifest text uses the manifest's own `i18n` (`plugin-host.md`, incl. choice labels).
- **Theme and icons**: upstream `Tokens` (dark/light follow upstream); our icons are drawn in `bashcut_ui` (no
  edit to upstream's `icons.rs` enum).

## 5. Project data

Our per-project data goes in a sidecar folder next to the project, `<name>.fcproj.bashcut/`:
`conversations/`, `plan.json` (edit plan, gates and their state), `runs/` (plugin results and logs). The `.fcproj`
format is not changed. What must be visible in FilmCraft's own panels is written through commands as markers,
labels or subclips (§1 rule 1). A project opened in upstream FilmCraft still works; it ignores the sidecar.

## 6. Agent panel in detail

- **Start**: user picks an agent: Claude Code (`claude`, or `BASHCUT_CLAUDE`) or an agent plugin (`kind: "agent"`
  in `bashcut-plugin.json`, with an `agent` section: `args`, `mcpConfig` (`{config}`), `resume` (`{session}`) and
  `events`, only `claude-stream-json` so far). The app writes a temporary MCP config for
  `filmcraft-cli mcp --bridge 127.0.0.1:<control port>` and starts the agent with the kit's skills available; without
  `--control <port>` an agent that needs the app says so instead of starting.
- **Turn** (`crates/bashcut-agent`, `process.rs`): one child process per turn. The message plus a short context line
  (project, active sequence, selection, playhead) goes in as one stream-json user message, stdin closes, and events
  stream back into the transcript (`stream_json.rs`, `transcript.rs`). The next turn resumes the agent's session
  (`--resume <id>`). Edits the agent makes appear in History as normal commands, so Undo works. Tool calls with the
  same command fold into one row ("edit.razor ×12"). Enter sends; Shift+Enter starts a new line.
- **Conversations**: kept in `<project>.fcproj.bashcut/conversations/<id>.json` (`bashcut-conversation/1`: the
  transcript and the agent's session id), listed newest first in the picker; an unsaved project keeps none.
- **Gates**: the edit-workflow skill writes gates into `plan.json` (`{"gates": [{stage, summary, evidence, state}]}`,
  `state` `pending` / `approved` / `changes`, other keys kept); the panel shows the first pending one as a card.
  Approve / Ask for changes (a note is required) writes `state` and `note` back and sends the answer as the next
  user message. The answer is written only when the turn can start.
- **Stop**: ends the agent's turn (its process group); the project stays as it is.
- **Plugin hand-offs**: Runs ▸ Send to agent (`bashcut.plugins.sendToAgent`) sends the run's result and the
  plugin's `handoff.then` as the next message.
- **Agent replies (UX8)** render minimal Markdown (`bashcut_ui/markdown.rs`): `#`/`##` headings, `-`/`1.` lists,
  `**bold**`, `*em*`, inline code, fenced code in a monospace frame; replies past 256 KB are cut with "…".
- **Empty transcript (UX8)**: three kit recipes (Plan a vlog, Hook and script, Finish for posting) as example
  prompts; a click fills the message box with `Use the bcp:<id> recipe on this project.` and does not send. Under
  them: "The agent stops at each stage for your approval." Automation ids `bashcut.agent.example.<recipe id>`.
- **Commands**: `bashcut.agent.{send, stop, state, transcript, conversations, open, new, agents, use, gate, approve,
  change}`; automation ids `bashcut.agent.{input, send, stop, picker, conversations, new, transcript}`,
  `bashcut.gate.{approve, change, note}`.
- **Phase 1 adapters**: Claude Code (`claude -p --output-format stream-json --input-format stream-json`) and Codex
  (`codex exec --json -` with the prompt on stdin, `resume <thread>` for later turns, the MCP server passed as `-c
  mcp_servers.filmcraft.*`; `codex`, or `BASHCUT_CODEX`; events `codex-json`, #37); others as plugins.

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
