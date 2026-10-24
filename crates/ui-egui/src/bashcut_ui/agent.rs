//! The agent as the app holds it (`bashcut/in-app-ui.md` §6): one [`Agent`] per app in egui
//! context data (no field in `FilmcraftApp`), polled each frame, and the `bashcut.agent.*` commands
//! that the Agent section, shortcuts, the control channel and MCP share.
//!
//! AG10: the terminal actions (Open in Terminal, Sign in, Install) live here too, so the panel's
//! buttons and `bashcut.agent.terminal` / `signIn` / `install` run the same code. Open in Terminal
//! is a hand-off: it waits (in [`poll`]) until the in-app agent has ended, then opens the window.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use bashcut_agent::{Agent, AgentSpec, Context, Status};
use filmcraft_engine::shortcuts::CommandInfo;
use serde_json::{Value, json};

use super::strings::{f, s};
use super::terminal::{self, Job};
use super::{hints, plugins};
use crate::FilmcraftApp;

pub type Shared = Arc<Mutex<Agent>>;

fn key() -> egui::Id {
    egui::Id::new("bashcut.agent.host")
}

/// Claude Code and Codex (`BASHCUT_CLAUDE` / `BASHCUT_CODEX` override the programs) and the
/// installed agent plugins.
fn agents(app: &FilmcraftApp, ctx: &egui::Context) -> Vec<AgentSpec> {
    let mut v = vec![installed(AgentSpec::claude_code, "BASHCUT_CLAUDE", "claude"), installed(AgentSpec::codex, "BASHCUT_CODEX", "codex")];
    let launches = plugins::with(app, ctx, |h| h.agents());
    // same fields (RC6); `AgentSpec::check` turns an unknown event format into the turn's error
    v.extend(launches.into_iter().map(AgentSpec::from));
    v
}

/// Folders the installers put `claude` / `codex` in. An app opened from the Finder, the Dock or a
/// desktop menu gets the login `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin` on macOS), which has none of
/// them (PV3), so the bare name alone is "not found" although the user's terminal finds it.
fn install_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    if cfg!(windows) {
        return Vec::new();
    }
    let mut v = Vec::new();
    if let Some(h) = home {
        v.extend([".local/bin", ".claude/local", ".npm-global/bin", ".bun/bin", ".volta/bin"].iter().map(|d| h.join(d)));
    }
    v.extend(["/opt/homebrew/bin", "/usr/local/bin"].iter().map(PathBuf::from));
    v
}

/// The spec for an agent program: `env` when set, the bare `name` when `PATH` finds it, else the
/// first install folder holding it, run with that folder first on its `PATH` (an npm install is a
/// `#!/usr/bin/env node` script whose `node` sits next to it).
fn installed(spec: fn(Option<String>) -> AgentSpec, env: &str, name: &str) -> AgentSpec {
    let set = std::env::var(env).ok();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    installed_in(spec, set, name, std::env::var_os("PATH"), &install_dirs(home.as_deref()))
}

/// [`installed`] with the variable's value, `PATH` and the install folders given.
fn installed_in(spec: fn(Option<String>) -> AgentSpec, set: Option<String>, name: &str, path: Option<std::ffi::OsString>, dirs: &[PathBuf]) -> AgentSpec {
    let set = set.filter(|p| !p.trim().is_empty());
    match set.or_else(|| locate(name, path.as_deref(), dirs).map(|p| p.to_string_lossy().into_owned())) {
        Some(program) => {
            let dir = Path::new(&program).parent().map(Path::to_path_buf);
            let mut s = spec(Some(program));
            if let Some(dir) = dir.filter(|d| !d.as_os_str().is_empty()) {
                let rest = path.as_deref().map(std::env::split_paths).into_iter().flatten();
                if let Ok(joined) = std::env::join_paths(std::iter::once(dir).chain(rest)) {
                    s.env.push(("PATH".into(), joined.to_string_lossy().into_owned()));
                }
            }
            s
        }
        None => spec(None),
    }
}

/// `name` in one of `dirs` when `path` does not find it (`None`: use the bare name).
fn locate(name: &str, path: Option<&std::ffi::OsStr>, dirs: &[PathBuf]) -> Option<PathBuf> {
    if super::hints::program_found(name, path) {
        return None;
    }
    dirs.iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// This app's agent, created on first use.
pub fn host(app: &FilmcraftApp, ctx: &egui::Context) -> Shared {
    super::shared::get_or_init(ctx, key(), || Agent::new(agents(app, ctx)))
}

/// Make `agents` this context's agents (the first one in use) instead of Claude Code, Codex and the
/// plugins' (a test's fake agent). Only before the host exists: `false` once it was created.
pub fn use_agents(ctx: &egui::Context, agents: Vec<AgentSpec>) -> bool {
    if super::shared::existing::<Agent>(ctx, key()).is_some() {
        return false;
    }
    super::shared::get_or_init(ctx, key(), || Agent::new(agents));
    true
}

/// Run `f` with the agent locked.
pub fn with<R>(app: &FilmcraftApp, ctx: &egui::Context, f: impl FnOnce(&mut Agent) -> R) -> R {
    let h = host(app, ctx);
    let mut g = h.lock().unwrap_or_else(PoisonError::into_inner);
    f(&mut g)
}

/// The Agent section is on screen.
fn shown(ctx: &egui::Context) -> bool {
    let st = super::State::get(ctx);
    st.rail && st.on_screen(super::AGENT)
}

/// What happened that the user may want to know about (AGT2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum News {
    /// The turn ended.
    Finished,
    /// The turn ended with an error.
    Failed,
    /// The agent asks to use a tool.
    Permission(String),
    /// The turn ended at a plan gate waiting for an answer.
    Gate,
}

/// The previous frame's turn, to see what changed.
#[derive(Clone, Default)]
struct Seen {
    running: bool,
    /// The turn was stopped by the user: its end is no news.
    stopped: bool,
    permissions: usize,
}

/// Turn states in, news out: a turn that ended (not one the user stopped), or more permission
/// requests waiting than before. `gate` is asked only when a turn ended without error.
fn watch(seen: &mut Seen, running: bool, stopping: bool, failed: bool, permissions: &[String], gate: impl FnOnce() -> bool) -> Option<News> {
    let ended = seen.running && !running && !seen.stopped;
    let asked = permissions.len() > seen.permissions;
    seen.stopped = if running { seen.stopped || stopping } else { false };
    seen.running = running;
    seen.permissions = permissions.len();
    if asked && let Some(tool) = permissions.last() {
        return Some(News::Permission(tool.clone()));
    }
    match (ended, failed) {
        (true, true) => Some(News::Failed),
        (true, false) if gate() => Some(News::Gate),
        (true, false) => Some(News::Finished),
        _ => None,
    }
}

/// [`watch`] for the live agent, with the previous frame's state kept in context data.
fn news(ctx: &egui::Context, a: &Agent) -> Option<News> {
    let id = egui::Id::new("bashcut.agent.seen");
    let mut seen = super::shared::temp::<Seen>(ctx, id);
    let running = matches!(a.status, Status::Running | Status::Stopping);
    let permissions: Vec<String> = a.permissions().into_iter().map(|p| clip_line(&p.tool, &p.detail)).collect();
    let n = watch(&mut seen, running, a.status == Status::Stopping, matches!(a.status, Status::Failed { .. }), &permissions, || a.gate().is_some());
    super::shared::put_temp(ctx, id, seen);
    n
}

/// "Tool: detail" cut to one short line, for a notification body.
fn clip_line(tool: &str, detail: &str) -> String {
    let line = detail.lines().next().unwrap_or_default();
    bashcut_agent::clip(&format!("{tool}: {line}"), 100)
}

/// The Agent rail icon has news the user has not seen (set by [`attention`]).
#[derive(Clone, Default)]
struct Unread(bool);

fn unread_key() -> egui::Id {
    egui::Id::new("bashcut.agent.unread")
}

/// The Agent icon carries a dot (the rail draws it only while the section is hidden).
pub fn unread(ctx: &egui::Context) -> bool {
    super::shared::temp::<Unread>(ctx, unread_key()).0
}

/// The Agent section was shown: the news is seen.
pub fn clear_unread(ctx: &egui::Context) {
    if unread(ctx) {
        super::shared::put_temp(ctx, unread_key(), Unread(false));
    }
}

/// `news` happened: an unread dot while the Agent section is hidden, and one OS notification while
/// the app window is not focused (unless the user turned them off). Never a sound.
fn attention(ctx: &egui::Context, shown: bool, n: &News) {
    if !shown {
        super::shared::put_temp(ctx, unread_key(), Unread(true));
    }
    if ctx.input(|i| i.focused) || !super::State::get(ctx).notify {
        return;
    }
    let name = super::shared::existing::<Agent>(ctx, key()).map_or_else(String::new, |h| h.lock().unwrap_or_else(PoisonError::into_inner).agent.name.clone());
    let (title, body) = match n {
        News::Finished => (f("{agent} finished", &[("agent", name.as_str())]), s("Open BashCutPro to see the result.").to_string()),
        News::Failed => (f("{agent} stopped with an error", &[("agent", name.as_str())]), s("Open BashCutPro to see what went wrong.").to_string()),
        News::Permission(what) => (f("{agent} needs your permission", &[("agent", name.as_str())]), what.clone()),
        News::Gate => (f("{agent} needs your decision", &[("agent", name.as_str())]), s("A plan step is waiting for your answer.").to_string()),
    };
    os_notify(&title, &body);
}

/// What [`os_notify`] calls instead of starting the platform's tool, once [`notify_stand_in`] set it.
pub type Notifier = fn(&str, &str);

static NOTIFIER: std::sync::OnceLock<Notifier> = std::sync::OnceLock::new();

/// From now on notifications go to `f` (title, body) instead of the OS, for the rest of this process (headless tests:
/// nothing is shown). `false` when one was set already (the first one stays).
pub fn notify_stand_in(f: Notifier) -> bool {
    NOTIFIER.set(f).is_ok()
}

/// The platform's own notification tool for `os` with `title` and `body` (no new dependency): `osascript` on macOS,
/// a PowerShell balloon on Windows, `notify-send` elsewhere. Text goes in as arguments / environment, never into a
/// script, so an agent name cannot inject AppleScript or PowerShell.
fn notify_command(os: terminal::Os, title: &str, body: &str) -> std::process::Command {
    use std::process::Command;
    let mut cmd;
    match os {
        terminal::Os::Mac => {
            cmd = Command::new("osascript");
            cmd.args(["-e", "on run argv", "-e", "display notification (item 2 of argv) with title (item 1 of argv)", "-e", "end run", title, body]);
        }
        terminal::Os::Windows => {
            cmd = Command::new("powershell");
            cmd.args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; $n = New-Object System.Windows.Forms.NotifyIcon; \
                 $n.Icon = [System.Drawing.SystemIcons]::Information; $n.Visible = $true; \
                 $n.ShowBalloonTip(5000, $env:BASHCUT_NOTE_TITLE, $env:BASHCUT_NOTE_BODY, 'Info'); Start-Sleep -Seconds 6; $n.Dispose()",
            ]);
            cmd.env("BASHCUT_NOTE_TITLE", title).env("BASHCUT_NOTE_BODY", body);
            #[cfg(windows)]
            std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
        }
        terminal::Os::Linux => {
            cmd = Command::new("notify-send");
            cmd.args(["--app-name=BashCutPro", "--", title, body]);
        }
    }
    cmd
}

/// Show an OS notification ([`notify_command`]). Best effort: a missing tool is logged and nothing else happens.
fn os_notify(title: &str, body: &str) {
    use std::process::Stdio;
    if let Some(f) = NOTIFIER.get() {
        return f(title, body);
    }
    let mut cmd = notify_command(terminal::Os::current(), title, body);
    match cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        // reap it off the UI thread
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::debug!("agent notification: {e}"),
    }
}

/// Collect the agent's events (called by `layout` every frame); an idle live agent is closed while
/// the Agent section is hidden (AG2: the next message resumes its session), unless a client other
/// than the panel drives it (AG3: `bashcut.agent.*` called while hidden; the idle close still applies).
pub fn poll(ctx: &egui::Context) {
    let Some(h) = super::shared::existing::<Agent>(ctx, key()) else { return };
    let shown = shown(ctx);
    let mut a = h.lock().unwrap_or_else(PoisonError::into_inner);
    let running = a.poll();
    let handing_off = finish_hand_off(ctx, &mut a) == Some(false);
    let news = news(ctx, &a);
    if shown {
        a.driven = false;
    } else if !running && !a.driven {
        a.end_session();
    }
    drop(a);
    if let Some(n) = news {
        attention(ctx, shown, &n);
    }
    if running || handing_off {
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

/// One line about where the user is, sent before every message.
pub fn context_line(app: &FilmcraftApp) -> String {
    let s = &app.session;
    let mut parts = Vec::new();
    match &s.path {
        Some(p) => parts.push(format!("Project {p}")),
        None => parts.push("Unsaved project".to_string()),
    }
    if let Some(id) = s.state.active_sequence {
        let name = s.project.item(id).map(|i| i.name.clone()).unwrap_or_default();
        parts.push(format!("sequence \"{}\"", bashcut_agent::clip(&name, 120)));
        // display only; edits go through commands in exact ticks
        parts.push(format!("playhead {:.3} s", s.playhead().seconds()));
    }
    let n = s.state.selection.len();
    if n > 0 {
        parts.push(format!("{n} clip(s) selected"));
    }
    parts.join(", ")
}

pub fn context(app: &FilmcraftApp) -> Context {
    Context {
        project: app.session.path.as_ref().map(std::path::PathBuf::from),
        line: context_line(app),
        cli: plugins::cli_path(),
        bridge: plugins::bridge_address(),
        control_token: plugins::control_token_for(plugins::bridge_address().as_deref()),
    }
}

/// The terminal actions `spec` offers now (AG10), as `bashcut.agent.<name>` names; `kit_present`: the session's init
/// already lists the kit, so `installKit` (Claude Code, found, kit not listed: AG14) is left out.
fn actions(spec: &AgentSpec, kit_present: bool) -> Vec<&'static str> {
    let found = hints::agent_found(spec);
    let mut v = Vec::new();
    if found && matches!(spec.id.as_str(), "claude-code" | "codex") {
        v.push("terminal");
    }
    if spec.id == "claude-code" {
        v.push(if found { "signIn" } else { "install" });
        if found && !kit_present {
            v.push("installKit");
        }
    }
    v
}

/// `bashcut.agent.<name>`.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, name: &str, params: &Value) -> Result<Value, String> {
    match name {
        "agents" => {
            // re-read the agent plugins (one may have been installed since)
            let list = agents(app, ctx);
            return Ok(with(app, ctx, |a| {
                if !a.is_running() {
                    if let Some(cur) = list.iter().find(|s| s.id == a.agent.id) {
                        a.agent = cur.clone();
                    }
                    a.agents = list;
                }
                let list = a.agents.iter().map(|s| json!({"id": s.id, "name": s.name, "events": s.events, "actions": actions(s, kit_present(a))}));
                json!(list.collect::<Vec<_>>())
            }));
        }
        "terminal" => return open_in_terminal(app, ctx).map(|opened| json!({"opened": opened, "pending": !opened})),
        "terminalEvent" => return terminal_event(app, ctx, params),
        "signIn" => {
            let program = with(app, ctx, |a| (a.agent.id == "claude-code").then(|| resume(a)).flatten().and_then(|v| v.into_iter().next()));
            let program = program.ok_or_else(|| s("Sign in works with Claude Code once it is installed.").to_string())?;
            sign_in(&program, context(app).project.as_deref())?;
            return Ok(json!({"opened": true}));
        }
        "install" => {
            // only on this explicit command (or the button); never while Claude Code is found
            let missing = with(app, ctx, |a| a.agents.iter().any(|s| s.id == "claude-code" && !hints::agent_found(s)));
            if !missing {
                return Err(s("Claude Code is already installed.").into());
            }
            install(context(app).project.as_deref())?;
            return Ok(json!({"opened": true}));
        }
        "installKit" => {
            // only on this explicit command (or the button); refused once the session's init lists the kit
            let (program, present) = with(app, ctx, |a| {
                let program = (a.agent.id == "claude-code").then(|| resume(a)).flatten().and_then(|v| v.into_iter().next());
                (program, kit_present(a))
            });
            program.ok_or_else(|| s("Download kit works with Claude Code once it is installed.").to_string())?;
            // a kit that names commands this build lacks may be updated even though the session lists it (PLK5)
            if present && kit_gap_now().is_none() {
                return Err(s("The BashCutPro kit is already installed.").into());
            }
            install_kit(&bashcut_agent::session::turn_dir(context(app).project.as_deref()))?;
            return Ok(json!({"opened": true}));
        }
        _ => {}
    }
    let c = context(app);
    let driven = !shown(ctx);
    let r = with(app, ctx, |a| {
        a.driven |= driven;
        a.command(name, params, &c)
    })
    .map_err(|e| e.0);
    if r.is_ok() && name == "send" {
        // the conversation is the panel's again
        ctx.data_mut(|d| d.remove::<TerminalWatch>(watch_key()));
    }
    if r.is_ok() && matches!(name, "send" | "approve" | "change") {
        super::State::update(ctx, |st| st.show(super::AGENT));
        ctx.request_repaint();
    }
    r
}

/// Send `text` as the next message (plugin hand-offs).
pub fn send(app: &mut FilmcraftApp, ctx: &egui::Context, text: &str) -> Result<Value, String> {
    invoke(app, ctx, "send", &json!({"text": text}))
}

/// The agent's program with its resume arguments for this conversation's session (Claude Code,
/// Codex, once found): what Open in Terminal runs.
pub fn resume(a: &Agent) -> Option<Vec<String>> {
    (matches!(a.agent.id.as_str(), "claude-code" | "codex") && hints::agent_found(&a.agent))
        .then(|| a.agent.argv.first().map(|p| terminal::resume_argv(p, &a.agent.resume_args, &a.conversation.session)))
        .flatten()
}

/// Claude Code's last `system/init` listed no [`terminal::KIT_PLUGIN`] (unknown before one: `false`): the kit is not
/// installed globally (the in-app session loads the kit folder itself, AG15).
fn not_installed(a: &Agent) -> bool {
    a.agent.id == "claude-code" && a.plugins.as_ref().is_some_and(|p| !p.iter().any(|n| n == terminal::KIT_PLUGIN))
}

/// The agent kit cannot be used: no kit folder ([`bashcut_dirs::kit::kit_dir`]) and no global install (a user with
/// one still works).
pub fn kit_missing(a: &Agent) -> bool {
    kit_missing_with(a, bashcut_dirs::kit::kit_dir().as_deref())
}

/// What the kit's index names that this build lacks (PLK5): `(command count, skill names)`. The registry is the
/// in-process one, core ids and `bashcut.*` ([`bashcut_dirs::kit::kit_all_command_gaps`]); no CLI is started. `None`
/// when there is no kit, no index (an older kit), or nothing is missing.
pub fn kit_gap_now() -> Option<(usize, Vec<String>)> {
    let kit = bashcut_dirs::kit::kit_dir()?;
    let file = std::fs::File::open(kit.join(bashcut_dirs::kit::INDEX_FILE)).ok()?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(file, bashcut_dirs::kit::MAX_INDEX_BYTES as u64 + 1), &mut text).ok()?;
    let mut ids: Vec<String> = filmcraft_engine::commands::command_specs().iter().map(|c| c.id.to_string()).collect();
    ids.extend(crate::menus::external_commands().into_iter().map(|c| c.id));
    let gaps = bashcut_dirs::kit::kit_all_command_gaps(&text, &ids)?;
    let mut all: Vec<&String> = gaps.missing_by_skill.iter().flat_map(|(_, ids)| ids).collect();
    all.sort();
    all.dedup();
    let count = all.len();
    (count > 0).then(|| (count, gaps.missing_by_skill.into_iter().map(|(name, _)| name).collect()))
}

/// How often the panel looks at the kit again (a pull changes the index; a frame is too often).
const KIT_GAP_EVERY: f64 = 5.0;

/// [`kit_gap_now`], read at most every [`KIT_GAP_EVERY`] seconds.
pub fn kit_gap(ctx: &egui::Context) -> Option<(usize, Vec<String>)> {
    type Seen = Option<(f64, Option<(usize, Vec<String>)>)>;
    let id = egui::Id::new("bashcut.agent.kit_gap");
    let now = ctx.input(|i| i.time);
    let seen: Seen = super::shared::temp(ctx, id);
    if let Some((at, gap)) = seen
        && now - at < KIT_GAP_EVERY
    {
        return gap;
    }
    let gap = kit_gap_now();
    super::shared::put_temp::<Seen>(ctx, id, Some((now, gap.clone())));
    gap
}

fn kit_missing_with(a: &Agent, kit: Option<&Path>) -> bool {
    kit.is_none() && not_installed(a)
}

/// Claude Code's last `system/init` listed [`terminal::KIT_PLUGIN`] (unknown before one: `false`).
fn kit_present(a: &Agent) -> bool {
    a.agent.id == "claude-code" && a.plugins.as_ref().is_some_and(|p| p.iter().any(|n| n == terminal::KIT_PLUGIN))
}

/// `argv` plus, while the kit is not installed globally, the kit folder as `--plugin-dir` (AG15) and the app's own
/// `bashcutpro` server (AG9).
fn terminal_argv(mut argv: Vec<String>, not_installed: bool, kit: Option<&Path>, c: &Context) -> Vec<String> {
    if !not_installed {
        return argv;
    }
    if let Some(k) = kit {
        argv.extend(["--plugin-dir".to_string(), k.to_string_lossy().into_owned()]);
    }
    if let (Some(cli), Some(bridge)) = (&c.cli, &c.bridge) {
        match terminal::terminal_mcp_config(cli, bridge) {
            Ok(path) => argv.extend(["--mcp-config".to_string(), path.to_string_lossy().into_owned()]),
            Err(e) => log::warn!("bashcut terminal MCP config: {e}"),
        }
    }
    argv
}

/// `argv` (Claude Code) plus the per-session hooks file (AGT1): `--settings <data dir>/agent/terminal-hooks.json`, so the
/// panel sees the terminal's state. Without the app's control address (or on a write error) the line is unchanged.
fn with_hooks(mut argv: Vec<String>, c: &Context) -> Vec<String> {
    let (Some(cli), Some(bridge), Some(program)) = (&c.cli, &c.bridge, argv.first()) else { return argv };
    let events = terminal::hook_events(terminal::claude_version(program));
    match terminal::terminal_hooks_config(cli, bridge, &events) {
        Ok(path) => argv.extend(["--settings".to_string(), path.to_string_lossy().into_owned()]),
        Err(e) => log::warn!("bashcut terminal hooks: {e}"),
    }
    argv
}

/// Open `job` in a terminal window in `cwd`; the error is the sentence shown to the user.
pub fn open_terminal(cwd: &Path, job: &Job, hint: Option<&str>) -> Result<(), String> {
    terminal::open(cwd, job, hint).map_err(|e| f("Could not open a terminal: {error}", &[("error", &e)]))
}

/// Sign in: `program` (`claude`) in a terminal with a hint to type `/login` (AG6). The login happens
/// in Claude Code's own flow; the app never reads or stores tokens.
pub fn sign_in(program: &str, project: Option<&Path>) -> Result<(), String> {
    let hint = s("Type /login and follow the steps in your browser, then close this window.");
    open_terminal(&bashcut_agent::session::turn_dir(project), &Job::Program(vec![program.to_string()]), Some(hint))
}

/// Install: the official Claude Code installer in a terminal (user-started only, never automatic).
pub fn install(project: Option<&Path>) -> Result<(), String> {
    let hint = s("Installing Claude Code with the official installer. When it finishes, run claude and type /login, then go back to BashCutPro.");
    open_terminal(&bashcut_agent::session::turn_dir(project), &terminal::install_job(terminal::Os::current()), Some(hint))
}

/// The folder Download kit puts the kit in: `<data dir>/bashcut-kit`, the first place the lookup reads (AG15).
pub fn kit_target() -> Option<PathBuf> {
    bashcut_dirs::data_dir().map(|d| d.join(bashcut_dirs::kit::DATA_SUBDIR))
}

/// `git` starts from `PATH` (Download kit needs it; without it the panel shows the zip URL).
pub fn git_found() -> bool {
    hints::program_found("git", std::env::var_os("PATH").as_deref())
}

/// Download kit (AG16's button and `bashcut.agent.installKit`): `git clone` of the agent kit into [`kit_target`], or
/// `git pull` when it is already there ([`terminal::kit_job`]), in a terminal in `cwd`; user-started only, never
/// automatic. The next message loads the kit with `--plugin-dir` (AG15).
pub fn install_kit(cwd: &Path) -> Result<(), String> {
    let dir = kit_target().ok_or_else(|| s("There is no data folder to keep the kit in.").to_string())?;
    if !git_found() {
        return Err(f("Git was not found. Download {url} and unpack it into {folder}.", &[("url", terminal::KIT_ZIP_URL), ("folder", &dir.to_string_lossy())]));
    }
    let hint = s("Downloading the BashCutPro kit. When it finishes, close this window: your next message in BashCutPro loads it.");
    let job = terminal::kit_job(terminal::Os::current(), "git", &dir, dir.join(".git").exists());
    open_terminal(cwd, &job, Some(hint))
}

fn hand_off_key() -> egui::Id {
    egui::Id::new("bashcut.agent.handoff")
}

/// Open in Terminal between the click and the window: the terminal's command and folder, and the
/// error of the last one that could not open (the panel shows it).
#[derive(Clone, Default)]
struct HandOff {
    job: Option<(Vec<String>, PathBuf)>,
    error: Option<String>,
}

fn hand_off_state(ctx: &egui::Context) -> HandOff {
    ctx.data(|d| d.get_temp::<HandOff>(hand_off_key())).unwrap_or_default()
}

/// Open in Terminal (the button and `bashcut.agent.terminal`): ends the in-app agent of this
/// conversation first (AG3: one writer per session), then the terminal resumes the session alone.
/// `true` when the window opened now, `false` while the turn is still stopping ([`poll`] opens it).
pub fn open_in_terminal(app: &FilmcraftApp, ctx: &egui::Context) -> Result<bool, String> {
    let c = context(app);
    let cwd = || bashcut_agent::session::turn_dir(c.project.as_deref());
    let job = with(app, ctx, |a| {
        resume(a).map(|argv| {
            let argv = terminal_argv(argv, not_installed(a), bashcut_dirs::kit::kit_dir().as_deref(), &c);
            (if a.agent.id == "claude-code" { with_hooks(argv, &c) } else { argv }, cwd())
        })
    });
    let job = job.ok_or_else(|| s("Open in Terminal works with Claude Code or Codex once it is installed.").to_string())?;
    ctx.data_mut(|d| d.insert_temp(hand_off_key(), HandOff { job: Some(job), error: None }));
    let done = with(app, ctx, |a| finish_hand_off(ctx, a));
    ctx.request_repaint();
    match done {
        Some(opened) => Ok(opened),
        None => Err(take_hand_off_error(ctx).unwrap_or_default()),
    }
}

/// Opens the pending hand-off once `a` has ended: `Some(true)` opened, `Some(false)` still waiting,
/// `None` nothing pending or the window could not open (the error waits in [`take_hand_off_error`]).
fn finish_hand_off(ctx: &egui::Context, a: &mut Agent) -> Option<bool> {
    let (argv, cwd) = hand_off_state(ctx).job?;
    if !a.release() {
        return Some(false);
    }
    a.continued_in_terminal();
    let hooked = argv.iter().any(|a| a == "--settings");
    let error = open_terminal(&cwd, &Job::Program(argv), None).err();
    let opened = error.is_none();
    ctx.data_mut(|d| {
        if opened && hooked {
            d.insert_temp(watch_key(), TerminalWatch(Some(terminal::Watch::default()), Instant::now()));
        } else {
            d.remove::<TerminalWatch>(watch_key());
        }
    });
    ctx.data_mut(|d| d.insert_temp(hand_off_key(), HandOff { job: None, error }));
    opened.then_some(true)
}

fn watch_key() -> egui::Id {
    egui::Id::new("bashcut.agent.terminal.watch")
}

/// What the terminal's hooks reported since Open in Terminal (`None`: no hooked terminal).
#[derive(Clone)]
struct TerminalWatch(Option<terminal::Watch>, Instant);

/// The phase of the terminal opened with hooks, for the panel's status line.
pub fn terminal_phase(ctx: &egui::Context) -> Option<terminal::Phase> {
    ctx.data(|d| d.get_temp::<TerminalWatch>(watch_key())).and_then(|w| w.0).map(|w| w.phase)
}

/// No hook event this long after the terminal's turn ended: its window may have been force-closed (a closed window sends
/// no `SessionEnd`).
const CLOSED_AFTER: Duration = Duration::from_secs(10 * 60);

/// The terminal opened with hooks still holds the session (AGT3): the panel's message box stays off until it ends or
/// the user takes the conversation back.
pub fn terminal_live(ctx: &egui::Context) -> bool {
    terminal_phase(ctx).is_some_and(|p| p != terminal::Phase::Closed)
}

/// The terminal's turn ended and no hook event came for [`CLOSED_AFTER`]: probably closed (AGT3 heartbeat rule).
pub fn terminal_maybe_closed(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<TerminalWatch>(watch_key()))
        .is_some_and(|w| w.0.is_some_and(|w| matches!(w.phase, terminal::Phase::Done | terminal::Phase::Failed)) && w.1.elapsed() >= CLOSED_AFTER)
}

/// Take back (AGT3): stop watching the terminal and make the panel the writer again (a notice in the transcript).
pub fn take_back(app: &FilmcraftApp, ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<TerminalWatch>(watch_key()));
    with(app, ctx, |a| a.returned_from_terminal());
    ctx.request_repaint();
}

/// `bashcut.agent.terminalEvent {event}`: one hook event of the terminal session (the hook JSON, an object or its text).
/// When the session ended the panel takes the conversation back (a notice in the transcript; the next message
/// resumes it here).
fn terminal_event(app: &FilmcraftApp, ctx: &egui::Context, params: &Value) -> Result<Value, String> {
    let event = match params.get("event") {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text).map_err(|e| format!("event: {e}"))?,
        Some(v @ Value::Object(_)) => v.clone(),
        _ => return Err("need `event`: the hook JSON".into()),
    };
    let Some(TerminalWatch(Some(mut watch), _)) = ctx.data(|d| d.get_temp::<TerminalWatch>(watch_key())) else {
        return Ok(json!({"ignored": true}));
    };
    let ended = watch.apply(&event);
    let phase = watch.phase;
    ctx.data_mut(|d| d.insert_temp(watch_key(), TerminalWatch(Some(watch), Instant::now())));
    if ended {
        with(app, ctx, |a| a.returned_from_terminal());
    }
    ctx.request_repaint();
    Ok(json!({"phase": format!("{phase:?}")}))
}

/// Open in Terminal is waiting for the in-app agent to end.
pub fn handing_off(ctx: &egui::Context) -> bool {
    hand_off_state(ctx).job.is_some()
}

/// The error of the last Open in Terminal that could not open a window, once.
pub fn take_hand_off_error(ctx: &egui::Context) -> Option<String> {
    let mut st = hand_off_state(ctx);
    let e = st.error.take()?;
    ctx.data_mut(|d| d.insert_temp(hand_off_key(), st));
    Some(e)
}

pub fn commands() -> Vec<CommandInfo> {
    [
        ("send", "Send to Agent"),
        ("stop", "Stop Agent"),
        ("cancel", "Remove Queued Agent Message"),
        ("permission", "Answer Agent Permission Request"),
        ("state", "Agent State"),
        ("transcript", "Agent Transcript"),
        ("conversations", "Agent Conversations"),
        ("open", "Open Agent Conversation"),
        ("new", "New Agent Conversation"),
        ("agents", "List Agents"),
        ("use", "Use Agent"),
        ("gate", "Pending Gate"),
        ("approve", "Approve Gate"),
        ("change", "Request Gate Changes"),
        ("terminal", "Open Agent in Terminal"),
        ("terminalEvent", "Terminal Agent Event"),
        ("signIn", "Sign In to Claude Code"),
        ("install", "Install Claude Code"),
        ("installKit", "Download BashCutPro Kit"),
    ]
    .into_iter()
    .map(|(id, label)| CommandInfo::new(&format!("bashcut.agent.{id}"), label, &[], None))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick(seen: &mut Seen, running: bool, stopping: bool, failed: bool, perms: &[&str], gate: bool) -> Option<News> {
        let perms: Vec<String> = perms.iter().map(|p| p.to_string()).collect();
        watch(seen, running, stopping, failed, &perms, || gate)
    }

    #[test]
    fn watch_reports_ended_turns_and_new_permissions() {
        // a turn that ends: Finished, Failed, or Gate (asked only when it ended without error)
        for (failed, gate, expected) in [(false, false, News::Finished), (true, false, News::Failed), (false, true, News::Gate), (true, true, News::Failed)] {
            let mut seen = Seen::default();
            assert_eq!(tick(&mut seen, false, false, false, &[], false), None, "idle from the start");
            assert_eq!(tick(&mut seen, true, false, false, &[], false), None, "running is no news");
            assert_eq!(tick(&mut seen, true, false, false, &[], true), None, "the gate is not asked while running");
            assert_eq!(tick(&mut seen, false, false, failed, &[], gate), Some(expected), "failed {failed}, gate {gate}");
            assert_eq!(tick(&mut seen, false, false, failed, &[], gate), None, "once");
        }
        let mut seen = Seen::default();
        let asked = std::cell::Cell::new(0);
        tick(&mut seen, true, false, false, &[], false);
        let n = watch(&mut seen, false, false, true, &[], || {
            asked.set(asked.get() + 1);
            true
        });
        assert_eq!((n, asked.get()), (Some(News::Failed), 0), "a failed turn never asks for a gate");

        // a stopped turn (stopping seen while it ran) ends without news, also when its end reports an error
        let mut seen = Seen::default();
        tick(&mut seen, true, false, false, &[], false);
        tick(&mut seen, true, true, false, &[], false);
        tick(&mut seen, true, false, false, &[], false);
        assert_eq!(tick(&mut seen, false, false, true, &[], false), None, "stopped: no news");
        // the next turn is a fresh one
        tick(&mut seen, true, false, false, &[], false);
        assert_eq!(tick(&mut seen, false, false, false, &[], false), Some(News::Finished));

        // more permission requests than before: the newest one, not the ones already waiting
        let mut seen = Seen::default();
        tick(&mut seen, true, false, false, &[], false);
        assert_eq!(tick(&mut seen, true, false, false, &["Bash: ls"], false), Some(News::Permission("Bash: ls".into())));
        assert_eq!(tick(&mut seen, true, false, false, &["Bash: ls"], false), None, "same request again");
        assert_eq!(tick(&mut seen, true, false, false, &["Bash: ls", "Edit: a.rs"], false), Some(News::Permission("Edit: a.rs".into())));
        assert_eq!(tick(&mut seen, true, false, false, &["Edit: a.rs"], false), None, "one answered: fewer is no news");
        assert_eq!(tick(&mut seen, true, false, false, &["Edit: a.rs", "Write: b"], false), Some(News::Permission("Write: b".into())));
        // a permission while the turn ends the same frame is reported first
        assert_eq!(tick(&mut seen, false, false, false, &["Edit: a.rs", "Write: b", "Bash: x"], false), Some(News::Permission("Bash: x".into())));
    }

    /// The words `cmd` gets as arguments, and its environment.
    fn parts(cmd: &std::process::Command) -> (String, Vec<String>, Vec<(String, String)>) {
        let args = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let envs = cmd.get_envs().filter_map(|(k, v)| Some((k.to_string_lossy().into_owned(), v?.to_string_lossy().into_owned()))).collect();
        (cmd.get_program().to_string_lossy().into_owned(), args, envs)
    }

    #[test]
    fn notification_text_is_an_argument_never_part_of_a_script() {
        // an agent name written to break out of an AppleScript / PowerShell / shell string
        let title = "Evil\" & (do shell script \"touch /tmp/x\") & \"'; $(id); `id` $env:Path";
        let body = "Bash: rm -rf ~ \\ \" ' ; | & > <\nline 2";

        let (program, args, envs) = parts(&notify_command(terminal::Os::Mac, title, body));
        assert_eq!(program, "osascript");
        assert_eq!(args[args.len() - 2..], [title, body], "title and body are the last two words, handed to the script as argv");
        let script = &args[..args.len() - 2];
        assert!(script.iter().all(|w| !w.contains("Evil") && !w.contains("rm -rf")), "{script:?}");
        assert!(script.iter().any(|w| w.contains("item 2 of argv") && w.contains("item 1 of argv")), "{script:?}");
        assert!(envs.is_empty());

        let (program, args, envs) = parts(&notify_command(terminal::Os::Linux, title, body));
        assert_eq!(program, "notify-send");
        assert_eq!(args, ["--app-name=BashCutPro", "--", title, body], "after `--`, so a title like `-h` is no option");

        let (program, args, envs_win) = parts(&notify_command(terminal::Os::Windows, title, body));
        assert_eq!(program, "powershell");
        assert!(args.iter().all(|w| !w.contains("Evil") && !w.contains("rm -rf")), "the script holds no user text: {args:?}");
        assert!(args.last().is_some_and(|w| w.contains("$env:BASHCUT_NOTE_TITLE") && w.contains("$env:BASHCUT_NOTE_BODY")));
        assert!(envs_win.contains(&("BASHCUT_NOTE_TITLE".into(), title.into())), "{envs_win:?}");
        assert!(envs_win.contains(&("BASHCUT_NOTE_BODY".into(), body.into())), "{envs_win:?}");
        let _ = envs;

        // empty text still builds a command with both words
        let (_, args, _) = parts(&notify_command(terminal::Os::Mac, "", ""));
        assert_eq!(args[args.len() - 2..], ["", ""]);
    }

    /// A file that exists (the test binary), so an agent run with it counts as found.
    fn found() -> String {
        std::env::current_exe().unwrap().to_string_lossy().into_owned()
    }

    /// A program that is nowhere.
    const MISSING: &str = "/no/such/folder/bashcut-ta4-missing-agent";

    fn plugin_agent(program: &str) -> AgentSpec {
        AgentSpec { id: "bashcut.my-agent".into(), name: "My agent".into(), ..AgentSpec::codex(Some(program.into())) }
    }

    /// An app and context whose agent host holds `agents` (the first one in use).
    fn app_with(agents: Vec<AgentSpec>) -> (FilmcraftApp, egui::Context) {
        let app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let ctx = egui::Context::default();
        super::super::shared::get_or_init(&ctx, key(), || Agent::new(agents));
        (app, ctx)
    }

    #[test]
    fn agents_list_terminal_actions() {
        assert_eq!(actions(&AgentSpec::claude_code(Some(found())), false), ["terminal", "signIn", "installKit"]);
        assert_eq!(actions(&AgentSpec::claude_code(Some(found())), true), ["terminal", "signIn"], "kit listed: no installKit");
        assert_eq!(actions(&AgentSpec::claude_code(Some(MISSING.into())), false), ["install"]);
        assert_eq!(actions(&AgentSpec::codex(Some(found())), false), ["terminal"]);
        assert!(actions(&AgentSpec::codex(Some(MISSING.into())), false).is_empty());
        assert!(actions(&plugin_agent(&found()), false).is_empty(), "a plugin agent has no terminal actions");
        // every action is a bashcut.agent.* command
        let ids: Vec<String> = commands().into_iter().map(|c| c.id).collect();
        for a in ["terminal", "signIn", "install", "installKit"] {
            assert!(ids.contains(&format!("bashcut.agent.{a}")), "{a}");
        }
    }

    #[test]
    fn use_agent_clears_the_kit_plugins() {
        let mut a = Agent::new(vec![AgentSpec::claude_code(Some(found())), AgentSpec::codex(Some(found()))]);
        a.plugins = Some(vec![terminal::KIT_PLUGIN.into()]);
        assert!(kit_present(&a) && !kit_missing_with(&a, None));
        // the same agent keeps what its init said
        a.use_agent("claude-code").unwrap();
        assert_eq!(a.plugins.as_deref(), Some(&[terminal::KIT_PLUGIN.to_string()][..]));
        a.use_agent("codex").unwrap();
        assert_eq!(a.plugins, None, "the plugins were Claude Code's");
        a.use_agent("claude-code").unwrap();
        assert_eq!(a.plugins, None, "unknown until this one's init");
        assert!(!kit_present(&a) && !kit_missing_with(&a, None));
        a.plugins = Some(vec!["other".into()]);
        assert!(kit_missing_with(&a, None) && !kit_present(&a));
        assert!(!kit_missing_with(&a, Some(Path::new("/kit"))), "a kit folder is enough");
    }

    #[test]
    fn terminal_argv_adds_the_kit_folder() {
        let argv = || vec!["claude".to_string(), "--resume".into(), "abc".into()];
        let kit = Path::new("/opt/my kit");
        let c = Context::default();
        // kit listed globally: the resume line as it is
        assert_eq!(terminal_argv(argv(), false, Some(kit), &c), argv());
        // not listed: the kit folder
        let v = terminal_argv(argv(), true, Some(kit), &c);
        assert_eq!(v[..3], argv()[..]);
        assert_eq!(v[3..], ["--plugin-dir", "/opt/my kit"]);
        // not listed, no kit folder, no bridge: nothing added
        assert_eq!(terminal_argv(argv(), true, None, &c), argv());
        // not listed with a bridge: the app's server too, in a config file; listed: none
        let c = Context { cli: Some("/apps/bashcutpro-cli".into()), bridge: Some("127.0.0.1:9876".into()), ..Context::default() };
        let v = terminal_argv(argv(), true, Some(kit), &c);
        let at = v.iter().position(|a| a == "--mcp-config");
        assert!(at.is_some_and(|i| v.get(i + 1).is_some_and(|p| p.ends_with("terminal-mcp.json"))), "{v:?}");
        assert_eq!(v.windows(2).filter(|w| w[0] == "--plugin-dir").count(), 1, "{v:?}");
        assert_eq!(terminal_argv(argv(), false, Some(kit), &c), argv());
    }

    #[test]
    #[ignore = "test ticket: bashcut.agent.terminal while a turn runs returns pending, poll opens it once the agent ended and the transcript says Continued in Terminal"]
    fn terminal_command_hands_off_after_the_turn_ends() {}

    /// The notices of `app`'s conversation that say "Back in the panel.".
    fn back_notices(app: &FilmcraftApp, ctx: &egui::Context) -> usize {
        with(app, ctx, |a| {
            a.conversation.transcript.items.iter().filter(|i| matches!(i, bashcut_agent::Item::Notice { message } if message == "Back in the panel.")).count()
        })
    }

    fn event(app: &mut FilmcraftApp, ctx: &egui::Context, e: Value) -> Result<Value, String> {
        invoke(app, ctx, "terminalEvent", &json!({"event": e}))
    }

    #[test]
    fn terminal_event_command_updates_the_phase_and_hands_back() {
        use terminal::Phase;
        let (mut app, ctx) = app_with(vec![AgentSpec::claude_code(Some(found()))]);
        with(&app, &ctx, |a| a.conversation.session = "sess-1".into());
        // before Open in Terminal (no hooked terminal): ignored, nothing changes
        assert_eq!(event(&mut app, &ctx, json!({"hook_event_name": "UserPromptSubmit"})), Ok(json!({"ignored": true})));
        assert_eq!(terminal_phase(&ctx), None);
        assert!(!terminal_live(&ctx));
        // a terminal without hooks has no watch either
        ctx.data_mut(|d| d.insert_temp(watch_key(), TerminalWatch(None, Instant::now())));
        assert_eq!(event(&mut app, &ctx, json!({"hook_event_name": "SessionEnd"})), Ok(json!({"ignored": true})));
        assert_eq!(back_notices(&app, &ctx), 0);

        // Open in Terminal with hooks: the watch starts at Opened
        ctx.data_mut(|d| d.insert_temp(watch_key(), TerminalWatch(Some(terminal::Watch::default()), Instant::now())));
        assert_eq!(terminal_phase(&ctx), Some(Phase::Opened));
        assert!(terminal_live(&ctx) && !terminal_maybe_closed(&ctx));
        for (e, phase) in [
            (json!({"hook_event_name": "UserPromptSubmit"}), "Working"),
            (json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_use_id": "t1"}), "Waiting"),
            (json!({"hook_event_name": "PreToolUse", "tool_name": "Read", "tool_use_id": "t2"}), "Waiting"),
            (json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": "t1"}), "Working"),
            (json!({"hook_event_name": "Stop"}), "Done"),
            (json!({"hook_event_name": "UserPromptSubmit"}), "Working"),
            (json!({"hook_event_name": "StopFailure"}), "Failed"),
        ] {
            assert_eq!(event(&mut app, &ctx, e.clone()), Ok(json!({"phase": phase})), "{e}");
            assert_eq!(format!("{:?}", terminal_phase(&ctx).unwrap()), phase);
            assert!(terminal_live(&ctx));
        }
        // the hook JSON may arrive as text; garbage and a missing event are errors
        assert_eq!(invoke(&mut app, &ctx, "terminalEvent", &json!({"event": "{\"hook_event_name\": \"Stop\"}"})), Ok(json!({"phase": "Done"})));
        assert!(invoke(&mut app, &ctx, "terminalEvent", &json!({"event": "not json"})).is_err());
        assert!(invoke(&mut app, &ctx, "terminalEvent", &json!({})).is_err());
        assert_eq!(terminal_phase(&ctx), Some(Phase::Done), "errors change nothing");

        // /clear and resume do not end it; the real end hands the conversation back once
        assert_eq!(event(&mut app, &ctx, json!({"hook_event_name": "SessionEnd", "reason": "clear"})), Ok(json!({"phase": "Done"})));
        assert_eq!(back_notices(&app, &ctx), 0);
        assert_eq!(event(&mut app, &ctx, json!({"hook_event_name": "SessionEnd", "reason": "prompt_input_exit"})), Ok(json!({"phase": "Closed"})));
        assert_eq!(back_notices(&app, &ctx), 1, "Back in the panel.");
        assert!(!terminal_live(&ctx), "the composer is the panel's again");
        assert_eq!(event(&mut app, &ctx, json!({"hook_event_name": "SessionEnd"})), Ok(json!({"phase": "Closed"})));
        assert_eq!(event(&mut app, &ctx, json!({"hook_event_name": "Stop"})), Ok(json!({"phase": "Closed"})), "closed ignores the rest");
        assert_eq!(back_notices(&app, &ctx), 1, "once");
        // a new session in the same window (SessionStart) starts over
        assert_eq!(event(&mut app, &ctx, json!({"hook_event_name": "SessionStart"})), Ok(json!({"phase": "Opened"})));
        assert!(terminal_live(&ctx));
    }

    #[test]
    fn terminal_without_a_session_adds_no_back_notice() {
        let (mut app, ctx) = app_with(vec![AgentSpec::claude_code(Some(found()))]);
        ctx.data_mut(|d| d.insert_temp(watch_key(), TerminalWatch(Some(terminal::Watch::default()), Instant::now())));
        assert_eq!(event(&mut app, &ctx, json!({"hook_event_name": "SessionEnd"})), Ok(json!({"phase": "Closed"})));
        assert_eq!(back_notices(&app, &ctx), 0, "nothing was continued");
    }

    #[test]
    fn terminal_done_with_no_event_for_ten_minutes_may_be_closed() {
        use terminal::Phase;
        let (mut app, ctx) = app_with(vec![AgentSpec::claude_code(Some(found()))]);
        let watch_at = |phase_events: &[Value], age: Duration| {
            let mut w = terminal::Watch::default();
            for e in phase_events {
                w.apply(e);
            }
            let at = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
            ctx.data_mut(|d| d.insert_temp(watch_key(), TerminalWatch(Some(w), at)));
        };
        let (stop, work, fail) =
            (json!({"hook_event_name": "Stop"}), json!({"hook_event_name": "UserPromptSubmit"}), json!({"hook_event_name": "StopFailure"}));
        watch_at(std::slice::from_ref(&stop), Duration::from_secs(9 * 60));
        assert!(!terminal_maybe_closed(&ctx), "9 minutes");
        watch_at(std::slice::from_ref(&stop), Duration::from_secs(10 * 60 + 1));
        assert!(terminal_maybe_closed(&ctx) && terminal_live(&ctx), "Done and 10 minutes");
        watch_at(&[fail], Duration::from_secs(11 * 60));
        assert!(terminal_maybe_closed(&ctx), "Failed too");
        // working / opened for a long time is a long turn, not a closed window
        watch_at(std::slice::from_ref(&work), Duration::from_secs(60 * 60));
        assert!(!terminal_maybe_closed(&ctx));
        watch_at(&[], Duration::from_secs(60 * 60));
        assert_eq!(terminal_phase(&ctx), Some(Phase::Opened));
        assert!(!terminal_maybe_closed(&ctx));
        // any event restarts the clock
        watch_at(std::slice::from_ref(&stop), Duration::from_secs(20 * 60));
        assert!(terminal_maybe_closed(&ctx));
        event(&mut app, &ctx, work).unwrap();
        assert!(!terminal_maybe_closed(&ctx));
        // no watch: never
        take_back(&app, &ctx);
        assert!(!terminal_maybe_closed(&ctx) && !terminal_live(&ctx));
        // the line is translated
        super::super::strings::set_language(Some("vi"));
        assert_eq!(s("Terminal: closed?"), "Terminal: đã đóng chưa?");
        super::super::strings::set_language(None);
    }

    #[test]
    fn install_and_sign_in_commands_refuse_when_not_applicable() {
        super::super::strings::set_language(Some("en"));
        // install: never while Claude Code is found
        let (mut app, ctx) = app_with(vec![AgentSpec::claude_code(Some(found())), AgentSpec::codex(Some(found()))]);
        assert_eq!(invoke(&mut app, &ctx, "install", &json!({})), Err("Claude Code is already installed.".into()));
        // sign in: Codex in use, or Claude Code missing
        with(&app, &ctx, |a| a.use_agent("codex")).unwrap();
        assert_eq!(invoke(&mut app, &ctx, "signIn", &json!({})), Err("Sign in works with Claude Code once it is installed.".into()));
        let (mut app, ctx) = app_with(vec![AgentSpec::claude_code(Some(MISSING.into()))]);
        assert_eq!(invoke(&mut app, &ctx, "signIn", &json!({})), Err("Sign in works with Claude Code once it is installed.".into()));
        // in Vietnamese too
        super::super::strings::set_language(Some("vi"));
        let e = invoke(&mut app, &ctx, "signIn", &json!({})).unwrap_err();
        assert_eq!(e, s("Sign in works with Claude Code once it is installed."));
        assert_ne!(e, "Sign in works with Claude Code once it is installed.");
        super::super::strings::set_language(None);
    }

    #[test]
    fn install_kit_command_refuses_when_kit_present() {
        super::super::strings::set_language(Some("en"));
        let (mut app, ctx) = app_with(vec![AgentSpec::claude_code(Some(found())), AgentSpec::codex(Some(found()))]);
        with(&app, &ctx, |a| a.plugins = Some(vec!["other".into(), terminal::KIT_PLUGIN.into()]));
        assert_eq!(invoke(&mut app, &ctx, "installKit", &json!({})), Err("The BashCutPro kit is already installed.".into()));
        with(&app, &ctx, |a| a.use_agent("codex")).unwrap();
        assert_eq!(invoke(&mut app, &ctx, "installKit", &json!({})), Err("Download kit works with Claude Code once it is installed.".into()));
        let (mut app, ctx) = app_with(vec![AgentSpec::claude_code(Some(MISSING.into()))]);
        assert_eq!(invoke(&mut app, &ctx, "installKit", &json!({})), Err("Download kit works with Claude Code once it is installed.".into()));
        super::super::strings::set_language(None);
        // opening the kit terminal (kit missing or not known yet) needs a real terminal: manual check
    }

    /// PV3: `claude` in `~/.local/bin` with a login `PATH` resolves to that file and puts its folder
    /// first on the child's `PATH`; found on `PATH`, or `BASHCUT_CLAUDE` set, keeps the given program.
    #[test]
    #[cfg(unix)]
    fn agent_program_found_outside_the_login_path() {
        let home = std::env::temp_dir().join(format!("bashcut-agent-locate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("claude"), "#!/bin/sh\n").unwrap();
        let dirs = install_dirs(Some(&home));
        assert_eq!(dirs.first(), Some(&bin), "~/.local/bin is looked at first");
        let path_of = |s: &AgentSpec| s.env.iter().find(|(k, _)| k == "PATH").map(|(_, v)| v.clone());

        // the login PATH of an app opened from the Finder: found in ~/.local/bin
        let login = Some(std::ffi::OsString::from("/usr/bin:/bin:/usr/sbin:/sbin"));
        let s = installed_in(AgentSpec::claude_code, None, "claude", login.clone(), &dirs);
        assert_eq!(s.argv.first().map(String::as_str), Some(&*bin.join("claude").to_string_lossy()), "{:?}", s.argv);
        let child: Vec<PathBuf> = std::env::split_paths(&path_of(&s).unwrap_or_default()).collect();
        assert_eq!(child.first(), Some(&bin), "its folder first: {child:?}");
        assert!(child.contains(&PathBuf::from("/usr/bin")), "the rest of PATH stays: {child:?}");

        // on PATH: the bare name, PATH untouched
        let on_path = Some(std::env::join_paths([bin.clone(), PathBuf::from("/usr/bin")]).unwrap());
        let s = installed_in(AgentSpec::claude_code, None, "claude", on_path, &dirs);
        assert_eq!(s.argv, AgentSpec::claude_code(None).argv, "bare name");
        assert_eq!(path_of(&s), None);

        // BASHCUT_CLAUDE wins over the install folder; blank is unset
        let s = installed_in(AgentSpec::claude_code, Some("/opt/x/claude".into()), "claude", login.clone(), &dirs);
        assert_eq!(s.argv.first().map(String::as_str), Some("/opt/x/claude"));
        assert_eq!(path_of(&s).and_then(|p| std::env::split_paths(&p).next()), Some(PathBuf::from("/opt/x")));
        let s = installed_in(AgentSpec::claude_code, Some("  ".into()), "claude", login.clone(), &dirs);
        assert_eq!(s.argv.first().map(String::as_str), Some(&*bin.join("claude").to_string_lossy()), "blank is unset");

        // nowhere: the bare name, so the panel says it is missing
        std::fs::remove_file(bin.join("claude")).unwrap();
        let s = installed_in(AgentSpec::claude_code, None, "claude", login, &dirs);
        assert_eq!(s.argv, AgentSpec::claude_code(None).argv);
        let _ = std::fs::remove_dir_all(&home);
    }
}
