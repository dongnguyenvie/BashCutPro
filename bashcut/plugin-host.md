# Design note: `crates/bashcut-plugin-host`

Status: **approved by the owner 2026-10-20**: floating window, registry-only install. The in-app UI around it (side panel by default, floating as pop-out): [in-app-ui.md](in-app-ui.md), proposed. ([#23](https://github.com/dongnguyenvie/BashCutPro/issues/23)).
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
crates/bashcut-manifest/           the one bashcut-plugin.json reader (serde, checked; RC5), re-exported as
                                   bashcut_plugin_host::manifest; the engine's hooks and bashcut-doctor read
                                   manifests with it too. OS names: bashcut_dirs::Os (other Unixes = linux).
                                   Its settings.rs (settings.json, secrets, run env) is the host's `settings`
crates/bashcut-dirs/src/child.rs   the one child supervisor (RC3): spawn + stdin policy + process group,
                                   cancel/timeout TERM -> 1.5 s -> KILL, capped pipe readers; plugin runs,
                                   key store, git, agent turns and engine hooks all use it
crates/bashcut-plugin-host/        new crate, ours only
  src/registry.rs                  parse registry.json (serde)
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

As built in #31 (P1): `src/{manifest,registry,install,settings,runner,host}.rs`; `host.rs` holds the jobs and
serves the `bashcut.plugins.*` commands (`Host::command`), and the egui side is `crates/ui-egui/src/bashcut_ui/
{plugins,plugins_panel,runs_panel}.rs` (rail sections, [in-app-ui.md](in-app-ui.md) §4), not `bashcut_plugins.rs`
or a floating window. The workspace's `crates/*` glob picks the crate up, so the root `Cargo.toml` is not touched;
the one upstream line is the ui-egui dependency (H4). Views (`view.rs`, §Views) came with #37. Tests: unit tests per module
and `tests/fake_plugins.rs` (fake Python plugins and a local git registry).

Agent plugins (#35): a manifest with `"kind": "agent"` carries an `agent` section instead of being run as a tool:
`args` (appended to the entry), `mcpConfig` (args that pass the MCP config: `{config}` for its file, or `{mcpCommand}`
/ `{mcpArgs}` for the server's command and arguments as TOML/JSON values, Codex-style `-c`), `resume` (must contain
`{session}`), `prompt` (args after everything else, Codex's `-`), `events` (`claude-stream-json`, the default, or
`codex-json`; `bashcut_manifest::AGENT_EVENTS`, each decoded by a row of `bashcut_agent::format::FORMATS`) and
`timeoutSecs` (0: 2 h; at most 24 h). `Host::agents()` lists them as `AgentLaunch`es (`bashcut-manifest`, serde-compatible
subset of `bashcut_agent::AgentSpec`, converted with `AgentSpec::from`, RC6) with the plugin's settings as env; the
Agent section offers them next to Claude Code ([in-app-ui.md](in-app-ui.md) §6). A Codex-style plugin:
`"agent": {"args": ["exec", "--json"], "mcpConfig": ["-c", "mcp_servers.filmcraft.command={mcpCommand}", "-c",
"mcp_servers.filmcraft.args={mcpArgs}"], "resume": ["resume", "{session}"], "prompt": ["-"], "events": "codex-json"}`.

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
  env vars its manifest declares. Settings files hold non-secrets only. A non-secret setting with an `env` name that
  the user never saved reaches the run as its manifest `default` (FX6, `settings::env_for`), unless the default is
  empty or one of its `env` names is already set in the app's own environment (the run inherits that one); a saved value always
  wins. Secrets have no default: only a stored key is exported. A key-store call that gets no answer in 20 s
  (e.g. a locked keyring's unlock prompt) is killed and fails, so it cannot freeze the app.
- Runs are cancellable; a hung plugin is killed after a timeout: 30 min by default (`runner::DEFAULT_TIMEOUT`), or
  the `timeoutSeconds` param of `bashcut.plugins.run` (capped at 24 h). Manifests declare no timeout.

## Phases

1. **P1** list / install / run / settings, result toast, progress bar. Unblocks running translate-captions,
   vieneu-tts, resources and image-gen from the app.
2. **P2** declarative views (`bashcut-view/1`), for review lists and pickers.
3. **P3** a dockable panel and the director chat dock, only if upstream panels become extensible or the owner
   accepts the extra merge cost.

## Results (`bashcut-result/1` in Runs)

Runs shows a finished run's `message`; a failed one (failed, crashed, timed out) shows its full error: `error.code:
error.message` from the result, else the host's error (start failure, timeout, no result) with the plugin's last
stderr line, else the exit code. **Retry** reruns the same plugin, action and params (kept on the job as `params`, also
in `bashcut.plugins.job`); a failed install or registry check retries as itself.

Files a run wrote go in `data` under these keys, and each gets a **Reveal** button (at most 8 per run):

| Key | Value |
|---|---|
| `path` | one path string |
| `paths`, `files`, `outputs` | a list of path strings or `{"path": …}` objects |

Other `data` keys are the plugin's own (`view` is the form, see below).

## Views (`bashcut-view/1`, #37)

An action with `"view": true` shows a form in its plugin card. While it runs, every stdout line that parses as
`{"schema": "bashcut-view/1", "title"?, "rows": [...]}` replaces the shown view (other lines are ignored; lines over
1 MB are skipped); a final `result.data.view` counts too. The newest run of the plugin that printed a view owns the
form, so runs without a view (button presses) do not replace it.

Rows (`type`): `text {text, muted?}`, `input {id, label?, value?, multiline?}`, `select {id, label?, value?,
choices: [{value, label?}]}`, `toggle {id, label?, value?}`, `button {id, label, action, params?}`, `list {id,
label?, items: [{id, text, detail?, buttons: [{id, label, action, params?}]}]}`, `image {path, caption?}` (PNG, at
most 16 MB and 4096 px a side), `progress {value?: 0..1, label?}`. Ids are 1–64 characters of letters, digits and
`.-_:`. Caps: 200 rows, 1000 list items, 6 buttons per item, 200 choices, 16 KB per text; unknown or broken rows are
skipped.

A button runs another action of the same plugin with: the form's current values that the target action declares as
params, then the button's own `params`, then (list buttons) `"item": <item id>`. Commands
`bashcut.plugins.view {plugin}` (the job and view, or null) and `bashcut.plugins.click {plugin, button, item?,
itemButton?, values?}` (returns the new job).

## Languages (`i18n`, I18N1)

A manifest may carry translations of what a person reads: `"i18n": {"vi": {"name", "summary"}}` on the plugin,
`"i18n": {"vi": {"title", "help"}}` on a setting, an action or a param (format: `bashcutpro-plugins/README.md`,
"Manifest"). The field is optional and `bashcut-plugin/1` stays the schema: an older manifest without it loads as
before. The Plugins panel shows the BashCutPro UI language (`bashcut.ui.setLanguage`, else the app's) through
`Manifest::name_in` / `summary_in`, `Setting::title_in` / `help_in`, `Action::title_in` and `Param::title_in`; a
missing or empty translation falls back to the English field. A `registry.json` entry may carry the same
`"i18n": {"vi": {"name", "summary"}}` (VI3 fills it); the Install rows show it through `Entry::name_in` /
`summary_in`, with the same fallback. Command names in the palette stay English for now. Automation ids and labels keep the English names.

**Choice labels (VI2).** An `enum` setting or param may label its choices per language with a `choices` map inside
the same `i18n` entry, from choice value to label:

```json
{"id": "service", "title": "Service", "type": "enum", "choices": ["gemini", "openai"], "default": "gemini",
 "i18n": {"vi": {"title": "Dịch vụ", "choices": {"gemini": "Gemini (Google)", "openai": "OpenAI"}}}}
```

The combo box shows the label (`Setting::choice_in` / `Param::choice_in`, `manifest::choice_label`); the value
(`gemini`) is still what is saved, validated and passed to the plugin. A value missing from the map, or mapped to an
empty string, shows as the value itself, so the map may be partial. There is no English label map: in English the
value is shown. Optional and backward compatible (`bashcut-plugin/1` unchanged); a non-object `choices` there
rejects the manifest.

## Bool params (BF1)

Params reach the plugin as flags (`Manifest::command_line`); a param left out keeps the plugin's own default. A
`bool` param sends its `flag` when true. When false it sends nothing, **unless the manifest `default` is `true`**: then
it sends `--no-<name>` (the flag with its dashes stripped, e.g. `--keep` → `--no-keep`), so a default-true bool can be
turned off from the plugin form, Batch steps and agent calls. Such a plugin must accept the false form (Python:
`argparse.BooleanOptionalAction`); the plugins repo lint (`tools/check_registry.py`) fails a default-true bool whose
script does not mention `--no-<name>` or `BooleanOptionalAction`. Default-false bools are unchanged, so older plugins
keep working.

## Testing

Never-crash rules apply (`AGENTS.md` §0): plugin output is hostile input, parsed with caps and no panics.

Unit tests in the crate with fake plugins (a Python stdlib script per case, as `bashcutpro-plugins` does);
`cargo test -p bashcut-plugin-host` only, no release build. One manual check in the app per phase.

## Owner answers (2026-10-20)

- Floating window, no `PanelKind` variant, for P1–P2: yes.
- Install from `registry.json` at a git ref only; no local-folder install.
