//! Agent auto-connect (#51, `bashcut/agent-connect.md`). While this window runs its control
//! channel (`filmcraft --control PORT` or `FILMCRAFT_CONTROL_PORT`), `<data dir>/bashcut/automation.json`
//! (`bashcut-automation/1`, mode 0600) says where it listens, which process it is and which project
//! it has open, so `bashcut-mcp` bridges agents to this window with no environment set, and refuses
//! a second (headless) engine on the same project. Written again when the open project changes,
//! removed when the window closes (only by the window it names); readers also check that `pid` is
//! alive. With two windows the first started keeps it; the other claims it once it is free (FX7).
//! SIGINT / SIGTERM remove it too (AG1, [`bashcut_dirs::signal`]).
//!
//! The channel binds 127.0.0.1 only and requires a session token (SY2, [`super::control::launch_token`]):
//! `token` is the 64-hex one this window serves, and the file is owner-only (0600), so only the
//! user's own processes can read it and drive the window. It runs by default (CC1, [`super::control`]);
//! the Agent section's setting and `BASHCUT_CONTROL=0` turn it off.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::json;

use crate::FilmcraftApp;

pub const SCHEMA: &str = "bashcut-automation/1";

/// `<data dir>/bashcut/automation.json`.
pub fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("bashcut").join("automation.json")
}

/// The control port this process runs: `--control PORT` on its command line, else
/// `FILMCRAFT_CONTROL_PORT` (the app's own rules, `apps/filmcraft/src/args.rs`).
pub fn control_port(args: &[String], env: Option<&str>) -> Option<u16> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--control" {
            return it.next().and_then(|p| p.trim().parse().ok());
        }
    }
    env.map(str::trim).filter(|p| !p.is_empty()).and_then(|p| p.parse().ok())
}

/// The port from the launch hook ([`super::control`], on by default), else the command line / env.
pub fn this_process_port() -> Option<u16> {
    if let Some(p) = super::control::launched() {
        return p;
    }
    static PORT: OnceLock<Option<u16>> = OnceLock::new();
    *PORT.get_or_init(|| {
        let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
        let env = std::env::var_os("FILMCRAFT_CONTROL_PORT").map(|v| v.to_string_lossy().into_owned());
        control_port(&args, env.as_deref())
    })
}

/// What we last wrote (`Some(project)`) and when the file was last checked, kept in egui temp
/// data so the file is rewritten only when the open project changes or another instance left it.
#[derive(Clone, Debug, PartialEq)]
struct Published {
    project: Option<String>,
    checked: std::time::Instant,
}

/// How often a window checks that the file is still its own or free to claim (FX7).
pub const CHECK: std::time::Duration = std::time::Duration::from_secs(3);

/// Called every pass from [`super::logic`], also while the window is hidden: publish, update or
/// remove `automation.json`. With two windows the first one started keeps the file (FX7,
/// `bashcut/decisions.md`); the other takes over within [`CHECK`] once it is gone or its owner died.
pub fn publish(app: &FilmcraftApp, ctx: &egui::Context, data_dir: Option<&Path>) {
    let (Some(dir), Some(port)) = (data_dir, this_process_port()) else { return };
    let path = file(dir);
    let id = egui::Id::new("bashcut.connect.published");
    if ctx.input(|i| i.viewport().close_requested()) {
        let _ = release(&path);
        ctx.data_mut(|d| d.remove::<Published>(id));
        return;
    }
    let project = app.session.path.clone();
    let last = ctx.data(|d| d.get_temp::<Published>(id));
    if let Some(l) = last.as_ref().filter(|l| l.project == project && l.checked.elapsed() < CHECK) {
        // egui wakes an idle window a frame early (the delay minus the predicted frame time), so
        // ask again for the rest or the 3 s checks stop once nothing else repaints (T9)
        ctx.request_repaint_after(CHECK.saturating_sub(l.checked.elapsed()));
        return;
    }
    // remembered even when the write fails, so a read-only data dir is not retried every frame
    if let Ok(Claim::Ours) = claim(&path, port, project.as_deref(), bashcut_dirs::pid_alive) {
        // a window ended by a signal never sees close_requested: remove the file then too (AG1)
        let path = path.clone();
        bashcut_dirs::signal::at_terminate("automation.json", move || {
            let _ = release(&path);
        });
    }
    ctx.data_mut(|d| d.insert_temp(id, Published { project, checked: std::time::Instant::now() }));
    // an idle window still runs a pass now and then to take the file over
    ctx.request_repaint_after(CHECK);
}

/// What [`claim`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    /// The file names this process (written, or already up to date).
    Ours,
    /// Another live window owns it; left alone.
    Theirs,
}

/// Who the file names: `Some(pid, address, project)` when it is well-formed.
fn owner(path: &Path) -> Option<(u32, String, Option<String>)> {
    if std::fs::metadata(path).ok()?.len() > 64 * 1024 {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    if v.get("schema")?.as_str()? != SCHEMA {
        return None;
    }
    let pid = u32::try_from(v.get("pid")?.as_u64()?).ok()?;
    let address = v.get("address")?.as_str()?.to_string();
    let project = v.get("project").and_then(|p| p.as_str()).map(str::to_string);
    Some((pid, address, project))
}

/// A window still answers there: its process is alive and its loopback address accepts a
/// connection (a reused pid of some other program does not count).
fn answers(pid: u32, address: &str, alive: impl Fn(u32) -> bool) -> bool {
    let Ok(addr) = address.parse::<std::net::SocketAddr>() else { return false };
    addr.ip().is_loopback() && alive(pid) && std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200)).is_ok()
}

/// Write the file unless another live window owns it: it is missing, malformed, ours, or names
/// a process that is gone or no longer answers. Rewritten only when what it says differs.
pub fn claim(path: &Path, port: u16, project: Option<&str>, alive: impl Fn(u32) -> bool) -> std::io::Result<Claim> {
    let me = std::process::id();
    let address = format!("127.0.0.1:{port}");
    match owner(path) {
        Some((pid, a, p)) if pid == me && a == address && p.as_deref() == project => Ok(Claim::Ours),
        Some((pid, a, _)) if pid != me && answers(pid, &a, alive) => Ok(Claim::Theirs),
        _ => write(path, port, project).map(|()| Claim::Ours),
    }
}

/// Remove the file when it names this process (on close); another window's file stays.
pub fn release(path: &Path) -> std::io::Result<bool> {
    match owner(path) {
        Some((pid, _, _)) if pid == std::process::id() => std::fs::remove_file(path).map(|()| true),
        _ => Ok(false),
    }
}

/// Write the file (temporary file, then rename; owner-only on Unix).
pub fn write(path: &Path, port: u16, project: Option<&str>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = json!({
        "schema": SCHEMA,
        "address": format!("127.0.0.1:{port}"),
        "token": super::control::launch_token(),
        "pid": std::process::id(),
        "project": project,
        "app": "BashCutPro",
        "version": env!("CARGO_PKG_VERSION"),
    });
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(body.to_string().as_bytes())?;
    drop(f);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_port_comes_from_the_flag_then_the_environment() {
        assert_eq!(control_port(&args(&["--demo", "--control", "9876"]), Some("1234")), Some(9876));
        assert_eq!(control_port(&args(&["--demo"]), Some(" 1234 ")), Some(1234));
        assert_eq!(control_port(&args(&["--demo"]), Some("")), None);
        assert_eq!(control_port(&args(&["--control"]), None), None);
        assert_eq!(control_port(&args(&["--control", "70000"]), None), None);
    }

    #[test]
    fn the_file_says_where_and_who() {
        let dir = std::env::temp_dir().join(format!("bashcut-connect-ui-{}", std::process::id()));
        let path = file(&dir);
        write(&path, 9876, Some("/p/a.fcproj")).unwrap();
        write(&path, 9876, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(v["address"], "127.0.0.1:9876");
        assert_eq!(v["pid"], std::process::id());
        assert!(v["project"].is_null());
        // SY2: the file carries the window's session token, the one the control channel requires
        let token = super::super::control::launch_token().expect("this machine has randomness");
        assert!(bashcut_dirs::token::valid(&token));
        assert_eq!(v["token"], json!(token));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bashcut-connect-fx7-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn put(path: &Path, pid: u32, address: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json!({"schema": SCHEMA, "address": address, "token": null, "pid": pid, "project": "/p/other.fcproj"}).to_string()).unwrap();
    }

    fn pid_in(path: &Path) -> u64 {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        v["pid"].as_u64().unwrap()
    }

    /// FX7: a second window started while the first runs leaves the first one's file alone, and
    /// closing it does not delete that file.
    #[test]
    fn a_live_window_keeps_the_file_and_another_window_does_not_remove_it() {
        let d = dir("live");
        let path = file(&d);
        let first = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = first.local_addr().unwrap().to_string();
        let other = std::process::id() + 1;
        put(&path, other, &address);
        let before = std::fs::read_to_string(&path).unwrap();
        assert_eq!(claim(&path, 19000, None, |p| p == other).unwrap(), Claim::Theirs);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "not overwritten");
        assert!(!release(&path).unwrap(), "not ours to remove");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "still there after our close");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// FX7: the file is claimed when it is missing, malformed, or names a window that is gone
    /// (dead pid, or a live pid whose address no longer answers), and removed on our close.
    #[test]
    fn a_missing_or_stale_file_is_claimed_and_ours_is_removed_on_close() {
        let d = dir("stale");
        let path = file(&d);
        let me = u64::from(std::process::id());
        assert_eq!(claim(&path, 19000, Some("/p/a.fcproj"), |_| true).unwrap(), Claim::Ours, "missing");
        assert_eq!(pid_in(&path), me);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(claim(&path, 19000, None, |_| true).unwrap(), Claim::Ours, "malformed");
        assert_eq!(pid_in(&path), me);
        let gone = std::process::id() + 1;
        let live = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        put(&path, gone, &live);
        assert_eq!(claim(&path, 19000, None, |_| false).unwrap(), Claim::Ours, "dead pid");
        assert_eq!(pid_in(&path), me);
        // a live pid (reused by some other program) whose address is closed
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        put(&path, gone, &closed);
        assert_eq!(claim(&path, 19000, None, |_| true).unwrap(), Claim::Ours, "nothing answers");
        assert_eq!(pid_in(&path), me);
        // ours: a project change is written, the same content is not rewritten
        assert_eq!(claim(&path, 19000, Some("/p/b.fcproj"), |_| true).unwrap(), Claim::Ours);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["project"], "/p/b.fcproj");
        assert_eq!(v["address"], "127.0.0.1:19000");
        assert!(release(&path).unwrap());
        assert!(!path.exists(), "removed on our close");
        assert!(!release(&path).unwrap(), "nothing left to remove");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// FX7 end to end with the real liveness check: the first window quits (its file goes), and the
    /// second claims it on its next check.
    #[test]
    fn the_second_window_takes_over_when_the_first_quits() {
        let d = dir("handover");
        let path = file(&d);
        let mut first = std::process::Command::new(if cfg!(windows) { "ping" } else { "sleep" })
            .args(if cfg!(windows) { &["-n", "30", "127.0.0.1"][..] } else { &["30"][..] })
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        put(&path, first.id(), &listener.local_addr().unwrap().to_string());
        assert_eq!(claim(&path, 19000, None, bashcut_dirs::pid_alive).unwrap(), Claim::Theirs);
        assert_eq!(pid_in(&path), u64::from(first.id()));
        // the first window quits without cleaning up (a crash): the next check takes over
        first.kill().unwrap();
        first.wait().unwrap();
        drop(listener);
        assert_eq!(claim(&path, 19000, None, bashcut_dirs::pid_alive).unwrap(), Claim::Ours);
        assert_eq!(pid_in(&path), u64::from(std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
    }
}
