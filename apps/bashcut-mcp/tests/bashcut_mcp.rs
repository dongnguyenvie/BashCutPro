//! `bashcut-mcp` as a program: which engine it starts with no `--cli` (the shared
//! `bashcut_dirs::engine` lookup: variables, then `bashcutpro` / `bcpro` / `filmcraft-cli` next to it),
//! `--headless` with a live window, and the `BASHCUT_BRIDGE=off` it gives the engine. The process
//! environment is set per child (`Command::env`), never in this process. The serve cases skip when
//! neither `python3` nor `python` runs.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use bashcut_connect::automation;
use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_bashcut-mcp");
/// Variables of this test's own environment that would change a decision.
const CLEAR: &[&str] =
    &["BASHCUTPRO_CLI", "BASHCUT_FILMCRAFT_CLI", "FILMCRAFT_CLI", "BASHCUTPRO_BRIDGE", "BASHCUTPRO_PROJECT", "BASHCUTPRO_ROOTS", "BASHCUT_BRIDGE"];

static N: AtomicU64 = AtomicU64::new(0);

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bashcut-mcp-it-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data/bashcut")).unwrap();
    d
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// `program args…` with a clean environment for the decision.
fn command(program: &Path, d: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(program);
    c.args(["--data-dir", &d.join("data").to_string_lossy()]).args(args).current_dir(d).stdin(Stdio::null());
    for k in CLEAR {
        c.env_remove(k);
    }
    c
}

fn print(c: &mut Command) -> Value {
    let out = c.arg("--print").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn the_engine_is_found_like_bashcut_dirs_engine() {
    let d = scratch("cli");
    let bin = Path::new(BIN);
    let argv0 = |c: &mut Command| print(c)["argv"][0].as_str().unwrap_or_default().to_string();
    // BASHCUTPRO_CLI, then BASHCUT_FILMCRAFT_CLI, then FILMCRAFT_CLI; empty counts as unset
    let all = [("BASHCUTPRO_CLI", "/x/pro"), ("BASHCUT_FILMCRAFT_CLI", "/x/old"), ("FILMCRAFT_CLI", "/x/fc")];
    assert_eq!(argv0(command(bin, &d, &[]).envs(all)), "/x/pro");
    assert_eq!(argv0(command(bin, &d, &[]).envs(all).env("BASHCUTPRO_CLI", "")), "/x/old");
    assert_eq!(argv0(command(bin, &d, &[]).env("FILMCRAFT_CLI", "/x/fc")), "/x/fc");
    assert_eq!(argv0(command(bin, &d, &["--cli", "/x/flag"]).envs(all)), "/x/flag", "--cli wins");

    // a copy of bashcut-mcp with only `bcpro` beside it (a macOS bundle) and nothing on PATH
    let app = d.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let copy = app.join(exe("bashcut-mcp"));
    std::fs::copy(bin, &copy).unwrap();
    let empty = d.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(argv0(command(&copy, &d, &[]).env("PATH", &empty)), exe("filmcraft-cli"), "nothing found: filmcraft-cli");
    std::fs::write(app.join(exe("bcpro")), b"").unwrap();
    assert_eq!(Path::new(&argv0(command(&copy, &d, &[]).env("PATH", &empty))), app.join(exe("bcpro")));
    // the Pro name beats the alias
    std::fs::write(app.join(exe("bashcutpro")), b"").unwrap();
    assert_eq!(Path::new(&argv0(command(&copy, &d, &[]).env("PATH", &empty))), app.join(exe("bashcutpro")));
    // on PATH when nothing is beside it
    for n in ["bcpro", "bashcutpro"] {
        std::fs::remove_file(app.join(exe(n))).unwrap();
    }
    std::fs::write(empty.join(exe("bcpro")), b"").unwrap();
    assert_eq!(Path::new(&argv0(command(&copy, &d, &[]).env("PATH", &empty))), empty.join(exe("bcpro")));
    let _ = std::fs::remove_dir_all(&d);
}

/// Publish a live window (this process, a listening port) in `d`'s data dir.
fn window(d: &Path) -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let v = json!({"schema": automation::SCHEMA, "address": addr, "token": null, "pid": std::process::id(), "project": null});
    std::fs::write(automation::file(&d.join("data")), v.to_string()).unwrap();
    (listener, addr)
}

#[test]
fn headless_with_a_live_window_and_no_project_is_unbridged() {
    let d = scratch("headless");
    let (_listener, addr) = window(&d);
    let bin = Path::new(BIN);
    let bridged = print(&mut command(bin, &d, &["--cli", "fc"]));
    assert_eq!(bridged["mode"], "bridge");
    assert_eq!(bridged["argv"], json!(["fc", "mcp", "--bridge", addr]));
    let headless = print(&mut command(bin, &d, &["--cli", "fc", "--headless"]));
    assert_eq!(headless["mode"], "headless", "{headless}");
    let argv: Vec<&str> = headless["argv"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(&argv[..2], ["fc", "mcp"]);
    assert!(!argv.contains(&"--bridge") && !argv.contains(&"--project"), "{argv:?}");
    assert!(headless["notes"].as_array().is_some_and(|n| !n.is_empty()), "the unlimited writes are noted");
    let _ = std::fs::remove_dir_all(&d);
}

fn python() -> Option<String> {
    ["python3", "python"].into_iter().find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success())).map(str::to_string)
}

/// A fake engine: `<python> mcp …` runs the script `mcp` in the working folder, which prints the
/// `BASHCUT_BRIDGE` it got and its arguments, then exits 7.
const FAKE_ENGINE: &str = r#"
import json, os, sys
print(json.dumps({"bridge": os.environ.get("BASHCUT_BRIDGE"), "args": sys.argv[1:]}), flush=True)
sys.exit(7)
"#;

fn serve(py: &str, d: &Path, args: &[&str]) -> (Output, Value) {
    let mut c = command(Path::new(BIN), d, &["--cli", py, "--headless"]);
    // the agent's own setting must not reach the engine
    let out = c.args(args).env("BASHCUT_BRIDGE", "on").stderr(Stdio::piped()).output().unwrap();
    let line = String::from_utf8_lossy(&out.stdout).lines().find(|l| l.starts_with('{')).unwrap_or_default().to_string();
    let v = serde_json::from_str(&line).unwrap_or_else(|_| panic!("no engine line: {out:?}"));
    (out, v)
}

#[test]
fn serve_gives_the_engine_bashcut_bridge_off() {
    let Some(py) = python() else {
        eprintln!("skipped: no python3 / python");
        return;
    };
    let d = scratch("serve");
    std::fs::write(d.join("mcp"), FAKE_ENGINE).unwrap();
    let (_listener, _) = window(&d);
    // unguarded: the engine has the agent's stdio
    let (out, v) = serve(&py, &d, &[]);
    assert_eq!(v["bridge"], "off", "{v}");
    assert_eq!(out.status.code(), Some(7), "the server's exit status");
    assert!(!v["args"].as_array().unwrap().iter().any(|a| a == "--bridge"), "{v}");
    // guarded (a root is given): the engine's stdout is piped through
    let root = d.join("proj");
    std::fs::create_dir_all(&root).unwrap();
    let (out, v) = serve(&py, &d, &["--roots", &root.to_string_lossy()]);
    assert_eq!(v["bridge"], "off", "{v}");
    assert_eq!(out.status.code(), Some(7));
    let _ = std::fs::remove_dir_all(&d);
}
