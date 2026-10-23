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
//! - `ui.screenshot`, `ui.click`, `ui.key`… give up after [`UI_WAIT`] on the server thread (FX17, FX19, [`serve_ui`]).

use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use serde_json::{Value, json};

use super::strings::s;
use super::widgets::{META, note};
use crate::{ControlRequest, FilmcraftApp};

/// The address the channel listens on: loopback only, never `0.0.0.0`.
pub const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;
/// The port tried first (the one the docs use for `--control`).
pub const DEFAULT_PORT: u16 = 9876;
/// `BASHCUT_CONTROL=0` (or `off`, `false`, `no`) keeps the channel off.
pub const ENV: &str = "BASHCUT_CONTROL";

/// Longest wait for a `ui.*` reply on the server thread (FX17 for `ui.screenshot`, FX19 for the rest): upstream's
/// own give-ups (screenshot 10 s plus its 0.25 s settle, the 3 s Retry deadline, the wait for synthetic input) are
/// checked on the UI thread, which runs no frames while the window is minimised (Windows) or covered (macOS), so
/// the server thread enforces it and the agent is never left waiting upstream's 60 s request timeout.
pub const UI_WAIT: Duration = Duration::from_secs(11);
/// FX32: control input (a click, a key) drained in a pass where the window is minimised or covered is lost:
/// the pass runs `logic` only, no `ui`, so no widget sees it, yet `logic` then answers `ok: true`. A plugin
/// button clicked that way "did nothing". While the window is not visible this takes the queued events out of
/// `queue`, so they wait (and `logic` keeps the request open, raises the window, and the server thread ends it
/// with [`NOT_IN_FRONT`] after [`UI_WAIT`]); [`release`] puts them back after the hook's drain.
pub fn hold_while_hidden(raw_input: &egui::RawInput, queue: &mut Vec<egui::Event>) -> Vec<egui::Event> {
    if raw_input.viewport().visible() == Some(false) { std::mem::take(queue) } else { Vec::new() }
}

/// Put back what [`hold_while_hidden`] took, ahead of anything queued since.
pub fn release(held: Vec<egui::Event>, queue: &mut Vec<egui::Event>) {
    queue.splice(0..0, held);
}

/// The reply when no frame came in time.
pub const NOT_IN_FRONT: &str =
    "window not in front: no frame was drawn within 11 s (another app covers BashCutPro, or it is minimised); call ui.focus {} and try again";

/// The requests [`serve_ui`] caps: every `ui.*` request needs frames to be answered. Not `ui.focus` (it is the
/// way out, and [`NOT_IN_FRONT`] would tell it to call itself) and not `ui.menu.invoke` (runs any command, like
/// `engine.execute`, which may take longer); both keep upstream's 60 s.
pub fn capped(method: &str) -> bool {
    method.starts_with("ui.") && !matches!(method, "ui.focus" | "ui.menu.invoke")
}

/// Hook in upstream's control server (`serve`, apps/filmcraft/src/control_server.rs): a [`capped`] request is
/// sent and answered here within [`UI_WAIT`] (`Ok(None)` when the app is gone); any other request comes back as
/// `Err` for upstream's wait.
pub fn serve_ui(req: ControlRequest, tx: &Sender<ControlRequest>, ctx: &egui::Context, reply: &Receiver<Value>) -> Result<Option<Value>, ControlRequest> {
    serve_within(req, tx, ctx, reply, UI_WAIT)
}

/// [`serve_ui`] with the wait as a parameter (tests use a short one).
fn serve_within(
    req: ControlRequest,
    tx: &Sender<ControlRequest>,
    ctx: &egui::Context,
    reply: &Receiver<Value>,
    wait: Duration,
) -> Result<Option<Value>, ControlRequest> {
    if !capped(&req.method) {
        return Err(req);
    }
    if tx.send(req).is_err() {
        return Ok(None);
    }
    ctx.request_repaint();
    Ok(Some(wait_reply(reply, wait)))
}

/// Hook at the top of upstream's `ui.focus {}` (FX17): un-minimise the window first. Upstream's
/// `ViewportCommand::Focus` alone leaves a minimised window minimised on Windows, so the [`NOT_IN_FRONT`]
/// advice would not help there. `Visible(true)` also unhides a macOS window hidden with Cmd-H (`Focus` alone
/// leaves the app hidden, so a held click never lands).
pub fn restore(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
}

/// The UI's reply, or [`NOT_IN_FRONT`] after `timeout`. A late reply goes to a dropped receiver (harmless).
fn wait_reply(reply: &Receiver<Value>, timeout: Duration) -> Value {
    reply.recv_timeout(timeout).unwrap_or_else(|_| json!({"ok": false, "error": NOT_IN_FRONT}))
}

/// The setting's label (English key for [`s`]).
pub const ALLOW: &str = "Allow AI agents to control BashCutPro";
/// Shown under the setting when it differs from what this window runs.
pub const RESTART: &str = "Restart BashCutPro to apply.";

/// The port this window was given at launch; unset in tests and other entry points.
static PORT: OnceLock<Option<u16>> = OnceLock::new();
/// The session token of this window, decided once by [`launch_token`].
static TOKEN: OnceLock<Option<String>> = OnceLock::new();
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

/// The launch hook: the control channel's session token (`FILMCRAFT_CONTROL_TOKEN` when valid, else
/// a random one per window; `None` when no randomness is available).
pub fn launch_token() -> Option<String> {
    TOKEN
        .get_or_init(|| {
            let token = bashcut_dirs::token::choose(std::env::var(bashcut_dirs::token::ENV).ok().as_deref(), bashcut_dirs::token::generate);
            if token.is_none() {
                log::warn!("BashCutPro: could not make a control token; the control channel stays off");
            }
            token
        })
        .clone()
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
    // the box, then its label wrapping in the rest of the row: egui's checkbox kept a wrapped
    // (Vietnamese) label to one row's height on some setups, over the transcript below (FX25)
    let width = ui.available_width();
    let r = ui
        .allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.set_max_width(width);
            ui.spacing_mut().item_spacing.x = ui.spacing().icon_spacing;
            let tick = ui.add(egui::Checkbox::without_text(&mut on));
            let label = ui.add(egui::Label::new(egui::RichText::new(s(ALLOW)).size(11.0)).wrap().sense(egui::Sense::click()));
            if label.clicked() {
                on = !on;
            }
            tick | label
        })
        .inner
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
    use std::sync::mpsc::channel;

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

    /// FX17: the UI thread never answers (window covered, frame loop stopped): the server thread replies
    /// with the "call ui.focus" error after the wait instead of hanging for upstream's 60 s.
    #[test]
    fn screenshot_without_a_frame_fails_with_ui_focus_hint() {
        let (_req, reply) = ControlRequest::new("ui.screenshot", json!({}));
        let t0 = std::time::Instant::now();
        let v = wait_reply(&reply, Duration::from_millis(50));
        assert!(t0.elapsed() < Duration::from_secs(5));
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("call ui.focus {}"), "{v}");
        assert!(UI_WAIT >= Duration::from_secs(10) && UI_WAIT < Duration::from_secs(60));
    }

    /// A reply in time is passed through; `ui.focus`, `ui.menu.invoke` and non-`ui.*` methods go back to
    /// upstream's wait untouched.
    #[test]
    fn screenshot_reply_passes_through_and_other_methods_are_left_to_upstream() {
        let (tx, rx) = channel::<ControlRequest>();
        let ctx = egui::Context::default();
        for method in ["ui.focus", "ui.menu.invoke", "engine.execute", "bashcut.agent.send"] {
            let (req, reply) = ControlRequest::new(method, json!({}));
            let back = serve_ui(req, &tx, &ctx, &reply).err().unwrap();
            assert_eq!(back.method, method);
        }
        assert!(rx.try_recv().is_err(), "not sent");
        let ui = std::thread::spawn(move || {
            for method in ["ui.screenshot", "ui.click"] {
                let req = rx.recv().unwrap();
                assert_eq!(req.method, method);
                req.reply.send(json!({"ok": true, "result": method})).unwrap();
            }
        });
        for method in ["ui.screenshot", "ui.click"] {
            let (req, reply) = ControlRequest::new(method, json!({"id": "x"}));
            let v = serve_ui(req, &tx, &ctx, &reply).ok().flatten().unwrap();
            assert_eq!(v, json!({"ok": true, "result": method}));
        }
        ui.join().unwrap();
        // app gone: no reply line
        let (dead_tx, dead_rx) = channel::<ControlRequest>();
        drop(dead_rx);
        let (req, reply) = ControlRequest::new("ui.screenshot", json!({}));
        assert!(matches!(serve_ui(req, &dead_tx, &ctx, &reply), Ok(None)));
    }

    /// FX19: every frame-bound `ui.*` request is capped (upstream's Retry deadline and input wait run on frames),
    /// the way out and arbitrary commands are not.
    #[test]
    fn frame_bound_ui_requests_are_capped() {
        for method in ["ui.click", "ui.move", "ui.drag", "ui.scroll", "ui.key", "ui.type", "ui.elements", "ui.inspect", "ui.set", "ui.screenshot"] {
            assert!(capped(method), "{method}");
        }
        for method in ["ui.focus", "ui.menu.invoke", "engine.execute", "engine.commands", "app.quit", "bashcut.agent.send", "uix"] {
            assert!(!capped(method), "{method}");
        }
    }

    /// FX19: a `ui.click` / `ui.key` the UI thread receives but never answers (minimised window: the Retry
    /// deadline and the input wait run on frames that never come) gets the "call ui.focus" error from the server
    /// thread instead of upstream's 60 s timeout.
    #[test]
    fn click_and_key_without_a_frame_fail_with_ui_focus_hint() {
        for method in ["ui.click", "ui.key"] {
            let (tx, rx) = channel::<ControlRequest>();
            let ctx = egui::Context::default();
            let (req, reply) = ControlRequest::new(method, json!({"id": "bashcut.rail.agent"}));
            let t0 = std::time::Instant::now();
            let v = serve_within(req, &tx, &ctx, &reply, Duration::from_millis(50)).ok().flatten().unwrap();
            assert!(t0.elapsed() < Duration::from_secs(5));
            assert_eq!(v, json!({"ok": false, "error": NOT_IN_FRONT}));
            // the request reached the UI channel and is still held there, unanswered
            let held = rx.try_recv().unwrap();
            assert_eq!(held.method, method);
        }
    }

    /// FX32: control input is not drained while the window is hidden (no `ui` pass would see it).
    #[test]
    fn control_input_waits_while_the_window_is_hidden() {
        let events = || vec![egui::Event::Text("a".into()), egui::Event::Text("b".into()), egui::Event::Text("c".into())];
        let input = |minimized, occluded| {
            let mut raw = egui::RawInput::default();
            let info = raw.viewports.entry(raw.viewport_id).or_default();
            info.minimized = minimized;
            info.occluded = occluded;
            raw
        };
        // minimised, occluded, or both: everything queued is held, in order
        for (minimized, occluded) in [(Some(true), Some(false)), (Some(false), Some(true)), (Some(true), Some(true)), (Some(true), None), (None, Some(true))] {
            let mut queue = events();
            let held = hold_while_hidden(&input(minimized, occluded), &mut queue);
            assert!(queue.is_empty(), "{minimized:?} {occluded:?}");
            assert_eq!(held, events());
        }
        // visible, or not known yet: nothing is held
        for (minimized, occluded) in [(Some(false), Some(false)), (None, None), (Some(false), None), (None, Some(false))] {
            let mut queue = events();
            let held = hold_while_hidden(&input(minimized, occluded), &mut queue);
            assert!(held.is_empty(), "{minimized:?} {occluded:?}");
            assert_eq!(queue, events());
        }
        // release puts the held events back ahead of what was queued since, keeping their order
        let mut queue = events();
        let held = hold_while_hidden(&input(Some(true), None), &mut queue);
        queue.push(egui::Event::Text("d".into()));
        release(held, &mut queue);
        let mut expected = events();
        expected.push(egui::Event::Text("d".into()));
        assert_eq!(queue, expected);
        // releasing nothing changes nothing
        release(Vec::new(), &mut queue);
        assert_eq!(queue, expected);
    }
}
