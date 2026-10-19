# Design: BashCutPro's in-app UI and how it extends FilmCraft

Status: **accepted** (2026-10-20); tickets #34 (U0), #31 (U1), #35 (U2), #36 (U3), #37 (U4), tracking #40. Extends [plugin-host.md](plugin-host.md) (#23, #31) and follows the one-way
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

- **Rail**: a 36 px strip on the right edge, always there. One icon per section: **Agent**, **Plugins**, **Runs**.
  Plugins that declare a panel add their own icon. Click toggles the side panel; Cmd/Ctrl+Shift+A focuses Agent.
- **Side panel** (left of the rail): resizable (240–720 px), collapsible, width and open section remembered.
  It takes width from the dock area, so it behaves like a docked panel without being in FilmCraft's dock tree.
  Any section can pop out into a floating window (for a second monitor) and dock back.
- **Agent**: chat with the editing agent (Claude Code, Codex, or another agent plugin), streamed replies, tool calls
  collapsed to one line each ("timeline.razor ×12"), **gate cards** for the edit-workflow gates (brief, plan,
  rough cut, draft) with Approve / Change / Stop, and a Stop button that ends the turn.
- **Plugins**: installed plugins from `registry.json`, install / update / remove, settings form generated from the
  manifest (secrets go to the OS key store), actions with a params form, and the plugin's own view when it has one.
- **Runs**: plugin runs and agent turns with progress (from the plugin's stderr), result, log, Cancel, and
  "Send to agent" for a hand-off result (`bashcut-result/1`, exit 3).

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
| `crates/bashcut-analysis/` | L3 | read-only measurements for the engine gaps in [skill-needs.md](skill-needs.md): loudness (wraps `filmcraft-audio-dsp` BS.1770), beats / tempo (wraps `audio-dsp`'s remix onset analysis), black / frozen frames (its own frame-difference math over frames the engine adapter decodes). Depends only on L0–L1 crates (`time`, `frame`, `audio-dsp`), so the engine (L4) can depend on it without a same-layer edge; pure functions, wasm-safe. |
| `crates/engine/src/bashcut/` | (engine) | `mod.rs` with `commands() -> Vec<CommandSpec>`: `bashcut.audio.loudness`, `bashcut.audio.beats`, `bashcut.review.pictureChecks`, … They read the `Session`, call `bashcut-analysis`, return JSON. Headless MCP gets them for free because they are engine commands. |
| `crates/ui-egui/src/bashcut_ui/` | L5 | `mod.rs` (the hooks' entry points), `rail.rs`, `side_panel.rs`, `agent_panel.rs`, `plugins_panel.rs`, `runs_panel.rs`, `view.rs` (renders `bashcut-view/1` with egui widgets), `state.rs`, `strings/` (our i18n). Uses upstream's `Tokens`, `widgets` and automation registry read-only. |
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

| # | File (upstream) | Line(s) | Churn at that spot since 2026-09-01 |
|---|---|---|---|
| H1 | `crates/engine/Cargo.toml` | `bashcut-analysis = { path = "../bashcut-analysis" }` | dependency list, low |
| H2 | `crates/engine/src/lib.rs` | `pub mod bashcut; // BashCutPro` | mod list, low |
| H3 | `crates/engine/src/commands.rs` | in `command_specs()`: `SPECS.get_or_init(\|\| { let mut v = build(); v.extend(crate::bashcut::commands()); v })` | 1 change (the tail of `build()` changed 3×, so not there) |
| H4 | `crates/ui-egui/Cargo.toml` | `bashcut-plugin-host`, `bashcut-agent` path deps | low |
| H5 | `crates/ui-egui/src/lib.rs` | `pub mod bashcut_ui;` next to `pub mod bashcut_brand;` | already ours |
| H6 | `crates/ui-egui/src/lib.rs` (`ui()`) | right after `let body = …`: `let body = bashcut_ui::layout(self, ui, body);` | 2 changes |
| H7 | `crates/ui-egui/src/menus.rs` (`invoke`) | first line: `if let Some(r) = crate::bashcut_ui::invoke(app, ctx, id, &params) { return r; }` | 4 changes (highest; rerere replays the resolution) |
| H8 | `crates/ui-egui/src/menus.rs` (`external_commands`) | `v.extend(crate::bashcut_ui::commands());` before `v` | 3 changes |
| H9 | `crates/ui-egui/src/menus.rs` (`menu_items_for`) | `out.extend(crate::bashcut_ui::menu_items(session));` before `out` | 2 changes |
| H10 | `crates/ui-egui/src/i18n/mod.rs` | existing `bashcut_brand::text` hook gains the language: `bashcut_brand::text(self, text)` | already ours |
| H11 | `xtask/src/main.rs` | `mod bashcut;` and in `layer_of`: `.or_else(\|\| bashcut::layer_of(name))` | 2 changes |

About **15 lines in 8 upstream files**, all marked `// BashCutPro`, all listed in `UPSTREAM.md`.
`Cargo.lock` conflicts are resolved by taking upstream's file and running `cargo check` (it re-adds our crates).

What `bashcut_ui::layout` does in H6, in order: read our state; if the rail is hidden (setting or Import/Export
mode) return `body` unchanged; draw the rail at the right edge of `body`; if a section is open, draw the side panel
left of it; draw floating (popped-out) sections as `egui::Window`s and toasts as foreground `egui::Area`s; return
the shrunk rect for the dock. One call per frame covers docked, floating and overlay UI.

## 4. State, settings, automation, strings

- **UI state** (open section, width, popped-out sections, panel scroll, draft message) is a `bashcut_ui::State`
  kept in egui's context data (`ctx.data_mut(|d| d.get_temp_mut_or_default::<State>(id))`), so `FilmcraftApp`
  gains no field. It is saved to `<data dir>/bashcut/ui.json` when it changes, not in upstream's workspace files.
- **Background work** (plugin runs, agent processes) runs on threads owned by `bashcut-plugin-host` /
  `bashcut-agent`; the UI polls their channels each frame and asks for a repaint while anything runs. Nothing
  blocks the UI thread. All plugin and agent output is hostile input: capped sizes, no panics (AGENTS.md §0).
- **Automation ids**: every widget registers in `app.auto` with ids under `bashcut.` (`bashcut.rail.agent`,
  `bashcut.agent.input`, `bashcut.gate.approve`, `bashcut.plugins.<id>.run`), so `ui.inspect` / `ui.click` drive
  them like upstream widgets.
- **Strings**: English in code, translations in `bashcut_ui/strings/<lang>.tsv` (Vietnamese first), looked up by the
  H10 hook before upstream's catalogs. Upstream's `.tsv` files are never edited.
- **Theme and icons**: upstream `Tokens` (dark/light follow upstream); our icons are drawn in `bashcut_ui` (no
  edit to upstream's `icons.rs` enum).

## 5. Project data

Our per-project data goes in a sidecar folder next to the project, `<name>.fcproj.bashcut/`:
`conversations/`, `plan.json` (edit plan, gates and their state), `runs/` (plugin results and logs). The `.fcproj`
format is not changed. What must be visible in FilmCraft's own panels is written through commands as markers,
labels or subclips (§1 rule 1). A project opened in upstream FilmCraft still works; it ignores the sidecar.

## 6. Agent panel in detail

- **Start**: user picks an agent (from agent plugins: `kind: "agent"` in `bashcut-plugin.json`, with argv, how to
  pass the MCP config, and the event format). The app starts its control channel if needed, writes a temporary MCP
  config for `filmcraft-cli mcp --bridge`, and starts the agent with the kit's skills available.
- **Turn**: the message plus a short context line (project, active sequence, selection, playhead) goes to the agent;
  events stream back into the transcript. Edits the agent makes appear in History as normal commands, so Undo works.
- **Gates**: the edit-workflow skill writes gates into `plan.json` (`{stage, summary, evidence, state}`); the panel
  shows the pending one as a card. Approve / Change writes the answer back and sends it as the next user message.
- **Stop**: ends the agent's turn (signal to the child); the project stays as it is.
- **Phase 1 adapters**: Claude Code (`claude -p --output-format stream-json --input-format stream-json`) and Codex
  (`codex exec --json`); others as plugins later.

## 7. Plugin views

`bashcut-view/1` is declarative JSON the plugin prints (initially) and updates (one JSON object per line on
stdout while a `view` action runs): rows of `text`, `input`, `select`, `toggle`, `button` (→ action), `list` (with
per-row buttons), `image` (a file path), `progress`. `view.rs` renders them with egui widgets and our theme.
Webviews are out: they would add an OS webview dependency to a pure-Rust app and a large attack surface; BashCut's
own research (spec 14) reached the same conclusion for most needs.

## 8. Phases

| Phase | Ships | Hooks used |
|---|---|---|
| U0 | `bashcut_ui` skeleton: rail + empty side panel, state, strings, tests; `xtask` layers | H4–H6, H10, H11 |
| U1 | Plugins + Runs sections (P1 of #31), `bashcut.plugins.*` commands | H7–H9 |
| U2 | Agent section with the Claude Code adapter, gates from `plan.json` | — |
| U3 | `bashcut-analysis` + engine commands (loudness, beats, picture checks) | H1–H3 |
| U4 | Plugin views (`bashcut-view/1`), Codex adapter, pop-out windows | — |

Each phase: `cargo test -p` for our crates and `crates/ui-egui/tests/bashcut_ui.rs`, `cargo xtask layers`,
one manual look at the app through `ui.screenshot`. No release build per phase.

## 9. Upstream sync checklist (for these hooks)

1. `upstream.sh merge` on `upstream-sync/<date>` with `git rerere` on.
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
