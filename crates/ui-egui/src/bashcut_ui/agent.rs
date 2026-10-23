//! The agent as the app holds it (`bashcut/in-app-ui.md` §6): one [`Agent`] per app in egui
//! context data (no field in `FilmcraftApp`), polled each frame, and the `bashcut.agent.*` commands
//! that the Agent section, shortcuts, the control channel and MCP share.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bashcut_agent::{Agent, AgentSpec, Context};
use filmcraft_engine::shortcuts::CommandInfo;
use serde_json::{Value, json};

use super::plugins;
use crate::FilmcraftApp;

pub type Shared = Arc<Mutex<Agent>>;

fn key() -> egui::Id {
    egui::Id::new("bashcut.agent.host")
}

/// Claude Code and Codex (`BASHCUT_CLAUDE` / `BASHCUT_CODEX` override the programs) and the
/// installed agent plugins.
fn agents(app: &FilmcraftApp, ctx: &egui::Context) -> Vec<AgentSpec> {
    let mut v = vec![AgentSpec::claude_code(std::env::var("BASHCUT_CLAUDE").ok()), AgentSpec::codex(std::env::var("BASHCUT_CODEX").ok())];
    let launches = plugins::with(app, ctx, |h| h.agents());
    // same fields (RC6); `AgentSpec::check` turns an unknown event format into the turn's error
    v.extend(launches.into_iter().map(AgentSpec::from));
    v
}

/// This app's agent, created on first use.
pub fn host(app: &FilmcraftApp, ctx: &egui::Context) -> Shared {
    super::shared::get_or_init(ctx, key(), || Agent::new(agents(app, ctx)))
}

/// Run `f` with the agent locked.
pub fn with<R>(app: &FilmcraftApp, ctx: &egui::Context, f: impl FnOnce(&mut Agent) -> R) -> R {
    let h = host(app, ctx);
    let mut g = h.lock().unwrap_or_else(PoisonError::into_inner);
    f(&mut g)
}

/// Collect the running turn's events (called by `layout` every frame).
pub fn poll(ctx: &egui::Context) {
    let Some(h) = super::shared::existing::<Agent>(ctx, key()) else { return };
    let running = h.lock().unwrap_or_else(PoisonError::into_inner).poll();
    if running {
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
    }
}

/// `bashcut.agent.<name>`.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, name: &str, params: &Value) -> Result<Value, String> {
    if name == "agents" {
        // re-read the agent plugins (one may have been installed since)
        let list = agents(app, ctx);
        return Ok(with(app, ctx, |a| {
            if !a.is_running() {
                if let Some(cur) = list.iter().find(|s| s.id == a.agent.id) {
                    a.agent = cur.clone();
                }
                a.agents = list;
            }
            json!(a.agents.iter().map(|s| json!({"id": s.id, "name": s.name, "events": s.events})).collect::<Vec<_>>())
        }));
    }
    let c = context(app);
    let r = with(app, ctx, |a| a.command(name, params, &c)).map_err(|e| e.0);
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

pub fn commands() -> Vec<CommandInfo> {
    [
        ("send", "Send to Agent"),
        ("stop", "Stop Agent"),
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
    ]
    .into_iter()
    .map(|(id, label)| CommandInfo::new(&format!("bashcut.agent.{id}"), label, &[], None))
    .collect()
}
