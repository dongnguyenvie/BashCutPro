//! `--bridge` defaults to the running BashCutPro (CC2, `bashcut/decisions.md`): for a subcommand
//! that talks to the bridge, with no `--bridge` and nothing that asks for the headless engine, the
//! address published in `<data dir>/bashcut/automation.json` (CC1) is added to the arguments.
//! `BASHCUT_BRIDGE=off` keeps the headless engine. `build.rs` routes upstream's argument list
//! through [`with_default_bridge`] and generates `BRIDGE_SUBCOMMANDS` from upstream's `main.rs`.

use std::path::PathBuf;
use std::time::Duration;

include!(concat!(env!("OUT_DIR"), "/bridge_subcommands.rs"));

/// Options that mean the headless engine (a project to open, save or a data directory to read).
const HEADLESS: &[&str] = &["--project", "--demo", "--save", "--save-as", "--data-dir"];
/// How long the published port may take to accept a connection.
const CONNECT: Duration = Duration::from_millis(300);

pub fn with_default_bridge(args: Vec<String>) -> Vec<String> {
    let off = std::env::var("BASHCUT_BRIDGE").is_ok_and(|v| v.trim().eq_ignore_ascii_case("off"));
    bridged(args, off, bashcut_dirs::data_dir)
}

/// [`with_default_bridge`] with `BASHCUT_BRIDGE=off` and the data dir (read only when needed) passed in.
fn bridged(args: Vec<String>, off: bool, data_dir: impl FnOnce() -> Option<PathBuf>) -> Vec<String> {
    if off || !wants_bridge(&args) {
        return args;
    }
    let Some(live) = data_dir().and_then(|d| bashcut_connect::automation::live(&d, CONNECT)) else {
        return args;
    };
    crate::diag(format_args!("bashcutpro-cli: using the running BashCutPro at {} (BASHCUT_BRIDGE=off for headless)", live.address));
    let mut out = args;
    out.extend(["--bridge".to_string(), live.address]);
    out
}

/// A bridge subcommand with no `--bridge` (in any spelling) and no headless option.
fn wants_bridge(args: &[String]) -> bool {
    let a = crate::args::Args::parse(args.iter().cloned());
    let given = |k: &str| args.iter().any(|s| s == k || s.strip_prefix(k).is_some_and(|r| r.starts_with('=')));
    a.pos(0).is_some_and(|sub| BRIDGE_SUBCOMMANDS.contains(&sub)) && !given("--bridge") && !HEADLESS.iter().any(|k| given(k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::path::Path;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// A data dir publishing a window with `pid` at a listening port.
    fn published(tag: &str, pid: u32) -> (PathBuf, TcpListener, String) {
        let d = std::env::temp_dir().join(format!("bashcutpro-bridge-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let file = bashcut_connect::automation::file(&d);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let json = serde_json::json!({"schema": bashcut_connect::automation::SCHEMA, "address": addr, "token": null, "pid": pid, "project": null});
        std::fs::write(file, json.to_string()).unwrap();
        (d, listener, addr)
    }

    #[test]
    fn bridge_defaults_to_the_running_app() {
        let (d, _listener, addr) = published("live", std::process::id());
        let run = |args: &[&str], off: bool| bridged(v(args), off, || Some(d.clone()));
        for sub in ["exec", "run", "inspect", "mcp", "commands"] {
            let mut want = v(&[sub, "x"]);
            want.extend(["--bridge".to_string(), addr.clone()]);
            assert_eq!(run(&[sub, "x"], false), want, "{sub}");
        }
        // options before the subcommand still count
        assert_eq!(run(&["--json", "exec", "x"], false).last(), Some(&addr));
        let unchanged: &[&[&str]] = &[
            &["exec", "x", "--project", "a.fcproj"],
            &["exec", "x", "--project=a.fcproj"],
            &["run", "--demo"],
            &["exec", "x", "--save", "b.fcproj"],
            &["exec", "x", "--save-as=b.fcproj"],
            &["mcp", "--data-dir", "/d"],
            &["exec", "x", "--bridge", "127.0.0.1:1"],
            &["exec", "x", "--bridge=127.0.0.1:1"],
            &["render", "a.fcproj"],
            &["probe", "a.mp4"],
            &["help"],
            &["--help"],
            &[],
        ];
        for args in unchanged {
            assert_eq!(run(args, false), v(args), "{args:?}");
        }
        assert_eq!(run(&["exec", "x"], true), v(&["exec", "x"]), "BASHCUT_BRIDGE=off");
        // no data dir, or nothing published there
        assert_eq!(bridged(v(&["exec", "x"]), false, || None), v(&["exec", "x"]));
        assert_eq!(bridged(v(&["exec", "x"]), false, || Some(Path::new("/no/such/dir").to_path_buf())), v(&["exec", "x"]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_dead_window_is_not_used() {
        // a pid no process has (above every OS's limit)
        let (d, _listener, _) = published("dead", u32::MAX - 1);
        assert_eq!(bridged(v(&["exec", "x"]), false, || Some(d.clone())), v(&["exec", "x"]));
        let _ = std::fs::remove_dir_all(&d);
        // a live pid but a closed port
        let (d, listener, _) = published("closed", std::process::id());
        drop(listener);
        assert_eq!(bridged(v(&["exec", "x"]), false, || Some(d.clone())), v(&["exec", "x"]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_data_dir_is_read_only_when_a_bridge_is_wanted() {
        let called = std::cell::Cell::new(false);
        let args = bridged(v(&["render", "a.fcproj"]), false, || {
            called.set(true);
            None
        });
        assert_eq!(args, v(&["render", "a.fcproj"]));
        assert!(!called.get());
    }
}
