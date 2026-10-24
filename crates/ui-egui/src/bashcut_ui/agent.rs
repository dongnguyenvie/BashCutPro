//! The agent as the app holds it (`bashcut/in-app-ui.md` §6): one [`Agent`] per app in egui
//! context data (no field in `FilmcraftApp`), polled each frame, and the `bashcut.agent.*` commands
//! that the Agent section, shortcuts, the control channel and MCP share.
//!
//! AG10: the terminal actions (Open in Terminal, Sign in, Install) live here too, so the panel's
//! buttons and `bashcut.agent.terminal` / `signIn` / `install` run the same code. Open in Terminal
//! is a hand-off: it waits (in [`poll`]) until the in-app agent has ended, then opens the window.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bashcut_agent::{Agent, AgentSpec, Context};
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

/// Collect the agent's events (called by `layout` every frame); an idle live agent is closed while
/// the Agent section is hidden (AG2: the next message resumes its session), unless a client other
/// than the panel drives it (AG3: `bashcut.agent.*` called while hidden; the idle close still applies).
pub fn poll(ctx: &egui::Context) {
    let Some(h) = super::shared::existing::<Agent>(ctx, key()) else { return };
    let shown = shown(ctx);
    let mut a = h.lock().unwrap_or_else(PoisonError::into_inner);
    let running = a.poll();
    let handing_off = finish_hand_off(ctx, &mut a) == Some(false);
    if shown {
        a.driven = false;
    } else if !running && !a.driven {
        a.end_session();
    }
    drop(a);
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
            if present {
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
    let job = with(app, ctx, |a| resume(a).map(|argv| (terminal_argv(argv, not_installed(a), bashcut_dirs::kit::kit_dir().as_deref(), &c), cwd())));
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
    let error = open_terminal(&cwd, &Job::Program(argv), None).err();
    let opened = error.is_none();
    ctx.data_mut(|d| d.insert_temp(hand_off_key(), HandOff { job: None, error }));
    opened.then_some(true)
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
