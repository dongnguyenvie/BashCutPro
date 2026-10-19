//! The plugin host as the app holds it (`bashcut/plugin-host.md`): one [`Host`] per app in egui
//! context data (no field in `FilmcraftApp`), polled each frame, and the `bashcut.plugins.*`
//! commands that the Plugins and Runs sections, shortcuts, the control channel and MCP share.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use bashcut_plugin_host::Host;
use bashcut_plugin_host::host::RunContext;
use bashcut_plugin_host::settings::OsKeyStore;
use filmcraft_engine::shortcuts::CommandInfo;
use serde_json::{Value, json};

use crate::FilmcraftApp;

pub type Shared = Arc<Mutex<Host>>;

fn key() -> egui::Id {
    egui::Id::new("bashcut.plugins.host")
}

/// This app's host, created on first use with the session's data dir.
pub fn host(app: &FilmcraftApp, ctx: &egui::Context) -> Shared {
    if let Some(h) = ctx.data(|d| d.get_temp::<Shared>(key())) {
        return h;
    }
    let h: Shared = Arc::new(Mutex::new(Host::new(super::data_dir(app), Arc::new(OsKeyStore))));
    ctx.data_mut(|d| d.insert_temp(key(), h.clone()));
    h
}

/// Run `f` with the host locked.
pub fn with<R>(app: &FilmcraftApp, ctx: &egui::Context, f: impl FnOnce(&mut Host) -> R) -> R {
    let h = host(app, ctx);
    let mut g = h.lock().unwrap_or_else(PoisonError::into_inner);
    f(&mut g)
}

/// Collect run events (called by `layout` every frame); keeps frames coming while a job runs.
pub fn poll(ctx: &egui::Context) {
    // no host yet means nothing ever ran: don't create one just to poll it
    let Some(h) = ctx.data(|d| d.get_temp::<Shared>(key())) else { return };
    let running = h.lock().unwrap_or_else(PoisonError::into_inner).poll();
    if running {
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

/// The open app's control address, if it serves one (`--control <port>` or `FILMCRAFT_CONTROL_PORT`).
pub fn bridge_address() -> Option<String> {
    static ADDR: OnceLock<Option<String>> = OnceLock::new();
    ADDR.get_or_init(|| {
        let from_env = std::env::var("FILMCRAFT_CONTROL_PORT").ok().and_then(|p| p.trim().parse::<u16>().ok());
        let args: Vec<String> = std::env::args().collect();
        let from_args = args.iter().enumerate().find_map(|(i, a)| match a.strip_prefix("--control=") {
            Some(p) => p.parse::<u16>().ok(),
            None if a == "--control" => args.get(i + 1).and_then(|p| p.parse::<u16>().ok()),
            None => None,
        });
        from_args.or(from_env).map(|p| format!("127.0.0.1:{p}"))
    })
    .clone()
}

/// `filmcraft-cli` next to the running app, passed to plugins as `--cli`.
fn cli_path() -> Option<String> {
    let name = if cfg!(windows) { "filmcraft-cli.exe" } else { "filmcraft-cli" };
    let p = std::env::current_exe().ok()?.with_file_name(name);
    p.is_file().then(|| p.to_string_lossy().into_owned())
}

pub fn run_context() -> RunContext {
    let known = filmcraft_engine::commands::command_specs().iter().map(|c| c.id.to_string()).collect();
    RunContext { bridge: bridge_address(), cli: cli_path(), known_commands: known }
}

/// `bashcut.plugins.<name>`.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, name: &str, params: &Value) -> Result<Value, String> {
    if name == "sendToAgent" {
        return send_to_agent(app, ctx, params);
    }
    let mut rc = run_context();
    if let Some(b) = params.get("bridge").and_then(Value::as_str).filter(|b| !b.is_empty()) {
        rc.bridge = Some(b.to_string());
    }
    let r = with(app, ctx, |h| h.command(name, params, &rc)).map_err(|e| e.0);
    if name == "run" && r.is_ok() {
        // a run shows up in Runs
        super::State::update(ctx, |st| st.show("runs"));
    }
    r
}

/// Hand a finished run's result to the agent. The Agent section (#35) picks it up; until then the
/// result is returned so a caller (or an agent over MCP) can use it.
fn send_to_agent(app: &mut FilmcraftApp, ctx: &egui::Context, params: &Value) -> Result<Value, String> {
    let job = params.get("job").and_then(Value::as_u64).ok_or("`job` (number) is required")?;
    let j = with(app, ctx, |h| h.job(job).cloned()).ok_or(format!("no job {job}"))?;
    let result = j.outcome.as_ref().and_then(|o| o.result.clone());
    Ok(json!({"sent": false, "reason": "the Agent section is not built yet", "job": job, "then": j.then, "result": result}))
}

pub fn commands() -> Vec<CommandInfo> {
    [
        ("list", "List Plugins"),
        ("registry", "Check Plugin Registry"),
        ("install", "Install Plugin"),
        ("update", "Update Plugin"),
        ("remove", "Remove Plugin"),
        ("settings", "Plugin Settings"),
        ("setSecret", "Store Plugin Secret"),
        ("run", "Run Plugin Action"),
        ("runs", "List Runs"),
        ("job", "Run Details"),
        ("cancel", "Cancel Run"),
        ("clear", "Clear Finished Runs"),
        ("sendToAgent", "Send Run to Agent"),
    ]
    .into_iter()
    .map(|(id, label)| CommandInfo::new(&format!("bashcut.plugins.{id}"), label, &[], None))
    .collect()
}
