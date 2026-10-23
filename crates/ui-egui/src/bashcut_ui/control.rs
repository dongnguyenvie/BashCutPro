//! Control channel on by default (CC1, `bashcut/decisions.md`): an app opened from the Dock or the
//! Start menu serves agents with no setup. The launch hook in `apps/filmcraft/src/main.rs` asks
//! [`launch_port`] for the port when `--control` / `FILMCRAFT_CONTROL_PORT` gave none; upstream's
//! server then binds it on 127.0.0.1 only, and [`super::connect`] writes `automation.json` so
//! `bashcut-mcp`, plugins and the Agent section find it.
//!
//! - Port: [`DEFAULT_PORT`] when free, else one the OS picks (always on [`LOOPBACK`]).
//! - Opt-out: the setting "Allow AI agents to control BashCutPro" (Agent section; off writes
//!   `<data dir>/bashcut/agents-off`), or `BASHCUT_CONTROL=0`. An explicit `--control` still wins.
//!   The server starts with the window, so a change applies at the next start.

use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::strings::s;
use super::widgets::{META, note};
use crate::FilmcraftApp;

/// The address the channel listens on: loopback only, never `0.0.0.0`.
pub const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;
/// The port tried first (the one the docs use for `--control`).
pub const DEFAULT_PORT: u16 = 9876;
/// `BASHCUT_CONTROL=0` (or `off`, `false`, `no`) keeps the channel off.
pub const ENV: &str = "BASHCUT_CONTROL";

/// The setting's label (English key for [`s`]).
pub const ALLOW: &str = "Allow AI agents to control BashCutPro";
/// Shown under the setting when it differs from what this window runs.
pub const RESTART: &str = "Restart BashCutPro to apply.";

/// The port this window was given at launch; unset in tests and other entry points.
static PORT: OnceLock<Option<u16>> = OnceLock::new();
/// Why the channel is off at launch, when it is.
static OFF: OnceLock<Off> = OnceLock::new();

/// Why the default channel did not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Off {
    /// It runs.
    No,
    /// The user turned the setting off.
    Setting,
    /// `BASHCUT_CONTROL=0`.
    Env,
    /// No loopback port could be bound.
    NoPort,
}

/// `<data dir>/bashcut/agents-off`: present while the user has turned agents off.
pub fn marker(data_dir: &Path) -> PathBuf {
    data_dir.join("bashcut").join("agents-off")
}

/// The setting: agents may control the app unless the user turned it off.
pub fn allowed(data_dir: Option<&Path>) -> bool {
    data_dir.is_none_or(|d| !marker(d).exists())
}

/// Save the setting.
pub fn set_allowed(data_dir: &Path, allow: bool) -> std::io::Result<()> {
    let path = marker(data_dir);
    if allow {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, "Delete this file (or tick the setting in the Agent section) to let AI agents control BashCutPro.\n")
    }
}

/// `BASHCUT_CONTROL` turns the default channel off.
pub fn env_off(value: Option<&str>) -> bool {
    value.is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no"))
}

/// A free loopback port: `preferred` when it can be bound, else one the OS picks.
pub fn free_port(preferred: u16) -> Option<u16> {
    if TcpListener::bind((LOOPBACK, preferred)).is_ok() {
        return Some(preferred);
    }
    TcpListener::bind((LOOPBACK, 0)).ok()?.local_addr().ok().map(|a| a.port())
}

/// The launch decision: an explicit port wins, else the default channel unless it is turned off.
pub fn decide(explicit: Option<u16>, env: Option<&str>, allow: bool, pick: impl FnOnce() -> Option<u16>) -> (Option<u16>, Off) {
    if explicit.is_some() {
        return (explicit, Off::No);
    }
    if env_off(env) {
        return (None, Off::Env);
    }
    if !allow {
        return (None, Off::Setting);
    }
    match pick() {
        Some(p) => (Some(p), Off::No),
        None => (None, Off::NoPort),
    }
}

/// The launch hook: the control port this window serves (`None`: no channel).
pub fn launch_port(explicit: Option<u16>, data_dir: Option<&Path>) -> Option<u16> {
    // a window created again after a graphics fallback keeps the first decision
    if let Some(p) = PORT.get() {
        return *p;
    }
    let env = std::env::var(ENV).ok();
    let (port, off) = decide(explicit, env.as_deref(), allowed(data_dir), || free_port(DEFAULT_PORT));
    match (port, off) {
        (Some(p), _) if explicit.is_none() => log::info!("BashCutPro: control channel for agents on {LOOPBACK}:{p} (opt out: {ENV}=0 or the Agent section)"),
        (None, Off::NoPort) => log::warn!("BashCutPro: no free loopback port for the control channel"),
        _ => {}
    }
    let _ = PORT.set(port);
    let _ = OFF.set(off);
    port
}

/// The port this window serves: from [`launch_port`], else `None` when it never ran.
pub fn launched() -> Option<Option<u16>> {
    PORT.get().copied()
}

/// Why the channel is off (`Off::No` when it runs or the launch hook never ran).
pub fn off_reason() -> Off {
    OFF.get().copied().unwrap_or(Off::No)
}

/// The setting row (Agent section): a checkbox, and "restart to apply" once it differs from what
/// this window runs.
pub fn setting(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let Some(dir) = super::data_dir(app) else { return };
    let t = app.tokens;
    let was = allowed(Some(&dir));
    let mut on = was;
    let r = ui
        .checkbox(&mut on, egui::RichText::new(s(ALLOW)).size(11.0))
        .on_hover_text(s("Lets agents and plugins on this computer edit the open project (127.0.0.1 only)."));
    app.auto.add_response("bashcut.agent.allowControl", &r, s(ALLOW));
    if on != was
        && let Err(e) = set_allowed(&dir, on)
    {
        app.ui.status = e.to_string();
    }
    let running = super::hints::control_on();
    if allowed(Some(&dir)) != running && off_reason() != Off::Env {
        note(ui, s(RESTART), META, t.text_dim);
    }
}

/// Next to a "control channel is off" line: "Turn on" when the user turned it off.
pub fn turn_on_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str) {
    let Some(dir) = super::data_dir(app) else { return };
    if off_reason() != Off::Setting {
        return;
    }
    if !allowed(Some(&dir)) {
        let r = ui.small_button(s("Turn on"));
        app.auto.add_response(id, &r, s("Turn on"));
        if r.clicked()
            && let Err(e) = set_allowed(&dir, true)
        {
            app.ui.status = e.to_string();
        }
    } else {
        note(ui, s(RESTART), META, app.tokens.text_dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bashcut-control-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn on_by_default_explicit_wins_and_both_opt_outs_work() {
        assert_eq!(decide(None, None, true, || Some(4242)), (Some(4242), Off::No));
        assert_eq!(decide(Some(9000), Some("0"), false, || Some(4242)), (Some(9000), Off::No));
        assert_eq!(decide(None, Some("0"), true, || Some(4242)), (None, Off::Env));
        assert_eq!(decide(None, Some(" OFF "), true, || Some(4242)), (None, Off::Env));
        assert_eq!(decide(None, Some("1"), true, || Some(4242)), (Some(4242), Off::No));
        assert_eq!(decide(None, None, false, || Some(4242)), (None, Off::Setting));
        assert_eq!(decide(None, None, true, || None), (None, Off::NoPort));
    }

    #[test]
    fn setting_round_trips_through_the_marker() {
        let d = temp("setting");
        assert!(allowed(Some(&d)), "on with no file");
        assert!(set_allowed(&d, false).is_ok());
        assert!(!allowed(Some(&d)));
        assert_eq!(decide(None, None, allowed(Some(&d)), || Some(1)).1, Off::Setting);
        assert!(set_allowed(&d, true).is_ok());
        assert!(set_allowed(&d, true).is_ok(), "turning on twice is fine");
        assert!(allowed(Some(&d)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn port_is_loopback_and_falls_back_when_taken() {
        let busy = TcpListener::bind((LOOPBACK, 0)).unwrap();
        let taken = busy.local_addr().unwrap().port();
        let p = free_port(taken).unwrap();
        assert_ne!(p, taken, "a taken port falls back to a free one");
        let l = TcpListener::bind((LOOPBACK, p)).unwrap();
        assert!(l.local_addr().unwrap().ip().is_loopback());
        assert!(LOOPBACK.is_loopback() && !LOOPBACK.is_unspecified());
    }

    /// No flags: a port is chosen, the discovery file names it on loopback, and a client that only
    /// reads the file reaches the server.
    #[test]
    fn a_client_finds_the_default_channel_with_no_config() {
        let d = temp("discover");
        let (port, off) = decide(None, None, allowed(Some(&d)), || free_port(DEFAULT_PORT));
        assert_eq!(off, Off::No);
        let port = port.unwrap();
        // stands in for upstream's control server (same bind); 9876 can be taken between the probe
        // and this bind (a BashCutPro window on this machine), then any free port stands in (T12: flaky)
        let server = TcpListener::bind((LOOPBACK, port)).or_else(|_| TcpListener::bind((LOOPBACK, 0))).unwrap();
        let port = server.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut c, _) = server.accept().unwrap();
            let mut line = String::new();
            BufReader::new(c.try_clone().unwrap()).read_line(&mut line).unwrap();
            writeln!(c, "{{\"ok\":true}}").unwrap();
        });
        let file = super::super::connect::file(&d);
        super::super::connect::write(&file, port, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["pid"], std::process::id());
        let addr: std::net::SocketAddr = v["address"].as_str().unwrap().parse().unwrap();
        assert!(addr.ip().is_loopback());
        let mut c = TcpStream::connect(addr).unwrap();
        writeln!(c, "{{\"method\":\"ping\"}}").unwrap();
        let mut reply = String::new();
        BufReader::new(c).read_line(&mut reply).unwrap();
        assert!(reply.contains("\"ok\":true"));
        t.join().unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
}
