//! Agent auto-connect (#51, `bashcut/agent-connect.md`). While this window runs its control
//! channel (`filmcraft --control PORT` or `FILMCRAFT_CONTROL_PORT`), `<data dir>/bashcut/automation.json`
//! (`bashcut-automation/1`, mode 0600) says where it listens, which process it is and which project
//! it has open, so `bashcut-mcp` bridges agents to this window with no environment set, and refuses
//! a second (headless) engine on the same project. Written again when the open project changes,
//! removed when the window closes; readers also check that `pid` is alive.
//!
//! The channel binds 127.0.0.1 only and has no token (`token` stays `null`): any local process of
//! any user that can reach the port can drive the window, which is why it only runs when asked for.

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

fn this_process_port() -> Option<u16> {
    static PORT: OnceLock<Option<u16>> = OnceLock::new();
    *PORT.get_or_init(|| {
        let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
        let env = std::env::var_os("FILMCRAFT_CONTROL_PORT").map(|v| v.to_string_lossy().into_owned());
        control_port(&args, env.as_deref())
    })
}

/// What we last wrote (`Some(project)`), kept in egui temp data so the file is rewritten only when
/// the open project changes.
#[derive(Clone, Debug, PartialEq)]
struct Published(Option<String>);

/// Called once per frame from [`super::layout`]: publish, update or remove `automation.json`.
pub fn publish(app: &FilmcraftApp, ctx: &egui::Context, data_dir: Option<&Path>) {
    let (Some(dir), Some(port)) = (data_dir, this_process_port()) else { return };
    let path = file(dir);
    let id = egui::Id::new("bashcut.connect.published");
    if ctx.input(|i| i.viewport().close_requested()) {
        let _ = std::fs::remove_file(&path);
        ctx.data_mut(|d| d.remove::<Published>(id));
        return;
    }
    let now = Published(app.session.path.clone());
    if ctx.data(|d| d.get_temp::<Published>(id)).as_ref() == Some(&now) {
        return;
    }
    // remembered even when the write fails, so a read-only data dir is not retried every frame
    let _ = write(&path, port, now.0.as_deref());
    ctx.data_mut(|d| d.insert_temp(id, now));
}

/// Write the file (temporary file, then rename; owner-only on Unix).
pub fn write(path: &Path, port: u16, project: Option<&str>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = json!({
        "schema": SCHEMA,
        "address": format!("127.0.0.1:{port}"),
        "token": null,
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
