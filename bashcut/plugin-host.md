# Design note: `crates/bashcut-plugin-host`

Status: **approved by the owner 2026-10-20**: floating window, registry-only install ([#23](https://github.com/dongnguyenvie/BashCutPro/issues/23)).
No code is written until this note is approved. Names come from [decisions.md](decisions.md) (2026-10-20, "Names for
our plugin layer"); the plugin format is the one settled by #8 (`bashcut-plugin/1`, `bashcut-registry/1`,
`bashcut-result/1`, see `bashcutpro-plugins/README.md`).

## Problem

Plugins run outside the app (decision 2026-10-20). That covers agents and shells, but a user in the desktop app
cannot see, configure or run a plugin, and nothing in the app can show a plugin's panel (BashCut's director chat dock,
a translate-captions review list, a voice picker). #23 adds the smallest in-app host that does this while keeping
upstream merges easy.

## Goals

1. List installed plugins and their actions, run an action with its params, show progress and the result.
2. Keep each plugin's non-secret settings; pass secrets from the OS key store as environment variables.
3. Show a plugin's declarative view (forms, lists, buttons) in a panel; no plugin code runs inside the app.
4. Add `bashcut.plugins.*` command ids so agents (MCP `command_run`) and shortcuts reach the same features.

Not goals: an in-process ABI, real-time render effects (that is upstream G11, VST3 / AU / OpenFX), a plugin
store UI beyond installing from `registry.json`.

## Shape

```
crates/bashcut-plugin-host/        new crate, ours only
  src/manifest.rs                  parse bashcut-plugin.json / registry.json (serde), check app.commands
                                   against the running build's command list
  src/install.rs                   <data dir>/bashcut-plugins/<id>/ : fetch from registry source, settings.json
  src/runner.rs                    spawn the entry argv (+ per-OS override) with connect args
                                   (--bridge 127.0.0.1:<control port>), secrets as env; read stderr progress,
                                   parse one bashcut-result/1 object from stdout; exit 3 = hand-off
  src/view.rs                      declarative view model (bashcut-view/1): rows of text, inputs, lists,
                                   buttons -> action invocations; plugin pushes view updates as JSON lines
  src/commands.rs                  bashcut.plugins.list | install | remove | run | settings | view
crates/ui-egui/src/bashcut_plugins.rs   egui drawing of the window and views (ours, new file)
```

The runner talks to the app the same way an agent does: through the control channel the app already serves
(`filmcraft --control <port>`, `crates/ui-egui/src/control.rs`) and the MCP bridge. The host never proxies engine
calls, so a plugin behaves identically from a shell, an agent or the app.

## Touch points in upstream files (the whole core change)

| File | Change |
|---|---|
| `Cargo.toml` (workspace) | add the crate to `members` |
| `crates/ui-egui/Cargo.toml` | depend on `bashcut-plugin-host` |
| `crates/ui-egui/src/lib.rs` | `pub mod bashcut_plugins;` (next to `bashcut_brand`) and one call per frame to draw the window |
| `crates/ui-egui/src/menus.rs` | `external_commands()` and `menu_items_for()` append `bashcut_plugins::commands()` (Window ▸ BashCut Plugins); `invoke()` routes ids starting with `bashcut.plugins.` to the host |

About ten added lines in upstream files, no renames. The plugin UI is a **floating egui window**, not a new
`PanelKind`: `PanelKind` is a closed enum serialized into workspaces, so a new variant would touch upstream's dock,
layout files and `ALL` array on every merge. Docking can come later if upstream makes panels extensible.

## Security

- A plugin is a local program the user installed; the host shows its manifest (`app.commands`, `edits`, network use)
  before the first run and on update.
- Secrets live in the OS key store (Keychain / Credential Manager / Secret Service) and reach the process only as the
  env vars its manifest declares. Settings files hold non-secrets only.
- Runs are cancellable; a hung plugin is killed after its declared timeout.

## Phases

1. **P1** list / install / run / settings, result toast, progress bar. Unblocks running translate-captions,
   vieneu-tts, resources and image-gen from the app.
2. **P2** declarative views (`bashcut-view/1`), for review lists and pickers.
3. **P3** a dockable panel and the director chat dock, only if upstream panels become extensible or the owner
   accepts the extra merge cost.

## Testing

Never-crash rules apply (`AGENTS.md` §0): plugin output is hostile input, parsed with caps and no panics.

Unit tests in the crate with fake plugins (a Python stdlib script per case, as `bashcutpro-plugins` does);
`cargo test -p bashcut-plugin-host` only, no release build. One manual check in the app per phase.

## Owner answers (2026-10-20)

- Floating window, no `PanelKind` variant, for P1–P2: yes.
- Install from `registry.json` at a git ref only; no local-folder install.
