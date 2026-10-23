# Agents reach the open app: auto-connect, project lock, allowed roots

Issue #51. An agent started with the kit should reach the open BashCutPro window without anyone setting
`BASHCUTPRO_BRIDGE`, fall back to a headless engine when no window is open, never run two engines on one `.fcproj`,
and write only where it should.

| Path | What |
|---|---|
| `crates/ui-egui/src/bashcut_ui/connect.rs` | the window publishes `<data dir>/bashcut/automation.json` |
| `crates/bashcut-connect/` | reading that file, project engine lock, allowed roots, MCP write guard (L4, no workspace deps) |
| `apps/bashcut-mcp/` | `bashcut-mcp`: the MCP command agents run (L6) |
| `bashcut/scripts/bashcutpro-mcp.sh` | reference launcher for the kit's `scripts/bashcutpro-mcp.sh` (runs `bashcut-mcp`) |

No upstream file changes: the window side runs from our `bashcut_ui::layout` (hook already in place).

## `automation.json` (`bashcut-automation/1`)

```json
{"schema": "bashcut-automation/1", "address": "127.0.0.1:9876", "token": null, "pid": 4242,
 "project": "/Users/me/Films/trip.fcproj", "app": "BashCutPro", "version": "0.9.0"}
```

Written (owner-only, 0600 on Unix; temp file + rename) when the window runs its control channel, i.e. it was started
with `--control PORT` or `FILMCRAFT_CONTROL_PORT`. It is written again when the open project changes, and removed
when the window closes, only by the window it names, also when it is ended by SIGINT / SIGTERM (AG1,
`bashcut_dirs::signal`, Unix; the process then exits with 130). A window that crashed or was killed (SIGKILL) leaves the file behind, so readers also
check that `pid` is alive and that the address (loopback only) accepts a connection.

The SIGINT / SIGTERM handler skips nothing upstream does (T12): upstream has no signal handling, so before AG1
the signal ended the process at once and no Rust code ran. The handler does the same plus removing this file.
Unsaved edits are not saved on the way out. They stay in the recovery journal (`<data dir>/Recovery/<session>/`,
`cleanExit: false`; the OS releases its lock) and the next launch offers them, as after a crash. Proof on macOS
(2026-10-23, `cargo xtask dev -- --control PORT` with a throwaway `BASHCUTPRO_DATA_DIR`): two `file.newBin`, then
`kill -TERM`. The exit code was 130, `automation.json` was gone, `snapshot.fcproj` (revision 4) was kept, and
the next launch's `file.recoveryList` listed that session. Edits made inside the journal's 1 s debounce before
the signal are lost, as they were before AG1.

Two windows (FX7, `decisions.md`): the first one started keeps the file. The other checks every 3 s and claims the
file once it is missing, malformed, or names a process that is gone or whose address no longer answers. When the
first window quits, agents reach the second one within a few seconds.

## `bashcut-mcp`

```bash
bashcut-mcp                         # bridge to the window if one is published and alive, else headless
bashcut-mcp --project trip.fcproj   # headless on this project when no window is open
bashcut-mcp --print                 # what it would do, as JSON
```

1. **Bridge** (`bashcutpro-cli mcp --bridge <address>`): `--bridge` / `BASHCUTPRO_BRIDGE`, else the published window.
   The window's project decides where the agent may write. A different `--project` gives a note: the agent edits the
   window.
2. **Headless** (`bashcutpro-cli mcp --project P`): when no window answers, or with `--headless`. Refused when the
   window has `P` open. While it runs, `bashcut-mcp` holds `<P>.bashcut/engine.lock` (the project's sidecar folder,
   `{"pid", "mode"}`), so a second headless engine on `P` is refused. A lock whose process is gone is taken over.
3. **Write guard**: `bashcut-mcp` relays MCP stdio and checks each `tools/call`. A command that writes (its id says
   save, export, render, screenshot, open, new, move, delete, install…) with a target path (`path`, `out`, `output`,
   `dest`, `target`, `file`, `dir`, `folder`…) outside the allowed roots gets a tool error back and never reaches
   the engine. `command_run`, `command_batch`, `ui_control` (`engine.execute`) and `ui_screenshot` are unpacked.
   Reads pass (importing media from anywhere is normal).
   **Allowed roots**: the project's folder, `BASHCUTPRO_ROOTS` (path list) and `--roots`. `..` is resolved and
   symlinks in the existing part of a path are followed. With no project and no roots there is no limit, and a note
   says so. This guards against mistakes; it is not a sandbox. The agent's own file tools are limited by the agent's
   settings (for Claude Code, the kit's permission rules).

## Control channel authentication

FilmCraft's control server (`apps/filmcraft/src/control_server.rs`) binds **127.0.0.1 only** and has **no token**.
It refuses lines that are not JSON objects with a `method` (that stops browser cross-origin requests), and it caps
line length and the number of connections. Any local process, of any user who can reach the loopback port, can drive
the window while the channel runs. So:

- the channel runs only when asked for (`--control`), and `automation.json` is owner-only;
- `token` is in the format already, so a channel that gains one (upstream, or a `bashcut` wrapper) needs no format
  change;
- do not run the channel on shared multi-user machines.

## Not done yet (UI)

- The "Let agents edit this window" switch in the Agent section, to start the channel at runtime. The control server
  is started by the app binary (`apps/filmcraft/src/main.rs`, from `--control`) and handed to `FilmcraftApp` when it
  is built, so starting it later needs either a hook in `main.rs` (a hot upstream file) or our own listener in
  `bashcut_ui` feeding the app's control queue. Left for the Agent section ticket (#35), see the issue comment.
- A warning in the window when it opens a project that a headless engine holds (`engine.lock`).

## Checks

- Unit tests: `cargo test -p bashcut-connect` (window found / stale / foreign files, lock refused and taken over,
  roots incl. `..` and symlinks, guard on run / batch / control / screenshot / batch arrays), `cargo test -p
  bashcut-mcp` (bridge vs headless decisions), `connect.rs` tests in `filmcraft-ui-egui` (port detection, file
  contents and mode).
- Manual (live test pending): open the app with `--control 9876`, start Claude Code with the kit; `command_list` reaches
  the window with no env set. With the app closed it runs headless. A second headless engine on the same project is
  refused.
