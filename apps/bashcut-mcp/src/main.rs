//! `bashcut-mcp`: start `bashcutpro-cli mcp` (or `filmcraft-cli mcp`) for an agent (#51, `bashcut/agent-connect.md`).
//!
//! - When the BashCutPro window runs its control channel it publishes
//!   `<data dir>/bashcut/automation.json`; if that process is alive and the port answers, the agent
//!   is bridged to the window (`mcp --bridge`) with no environment to set.
//! - Otherwise the engine runs headless (`mcp --project P` when a project is given) and holds the
//!   project's engine lock, so a second engine on the same `.fcproj` is refused.
//! - Every request passes the write guard: writes only under the project's folder and
//!   `BASHCUTPRO_ROOTS`.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

use bashcut_connect::lock::ProjectLock;
use bashcut_connect::roots::{self, Roots};
use bashcut_connect::{automation, guard};
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

const HELP: &str = "\
bashcut-mcp: the engine's MCP server (bashcutpro-cli mcp) for an agent, bridged to the open
BashCutPro window when there is one, headless otherwise

USAGE
  bashcut-mcp [--project P.fcproj] [--headless] [--bridge 127.0.0.1:PORT] [--cli PATH]
              [--data-dir DIR] [--roots LIST] [--no-guard] [--print]

  (no option)    bridge to the window found through <data dir>/bashcut/automation.json, else
                 headless; MCP on stdin / stdout
  --project P    the project for a headless engine (also BASHCUTPRO_PROJECT); its folder is where
                 the agent may write. A second engine on the same project is refused.
  --headless     never bridge; refused when the window has this project open
  --bridge ADDR  bridge to this control address (also BASHCUTPRO_BRIDGE)
  --cli PATH     the engine (default: $BASHCUTPRO_CLI, $BASHCUT_FILMCRAFT_CLI or $FILMCRAFT_CLI,
                 else bashcutpro-cli, bcpro or filmcraft-cli next to this program, else the same on PATH)
  --data-dir DIR BashCutPro data dir (default: BASHCUTPRO_DATA_DIR, FILMCRAFT_DATA_DIR or the platform's)
  --roots LIST   more folders the agent may write to (path-list separator: ':' Unix, ';' Windows);
                 BASHCUTPRO_ROOTS adds to them
  --no-guard     pass requests through unchecked
  --print        print the decision (mode, command line, roots) as JSON and exit
  --version      print `bashcut-mcp <version>` and exit

EXIT STATUS
  the MCP server's · 2 usage error or refused (project open elsewhere)
";

const VALUED: &[&str] = &["--project", "--bridge", "--cli", "--data-dir", "--roots"];

struct Args {
    opts: Vec<(String, Option<String>)>,
    bad: Vec<String>,
}

impl Args {
    fn parse(it: impl IntoIterator<Item = String>) -> Args {
        let mut a = Args { opts: Vec::new(), bad: Vec::new() };
        let mut it = it.into_iter();
        while let Some(s) = it.next() {
            if let Some((k, v)) = s.split_once('=').filter(|(k, _)| k.starts_with("--")) {
                a.opts.push((k.to_string(), Some(v.to_string())));
            } else if VALUED.contains(&s.as_str()) {
                let v = it.next();
                a.opts.push((s, v));
            } else if s.starts_with("--") {
                a.opts.push((s, None));
            } else {
                a.bad.push(s);
            }
        }
        a
    }

    fn opt(&self, k: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(n, _)| n == k).and_then(|(_, v)| v.as_deref())
    }

    fn flag(&self, k: &str) -> bool {
        self.opts.iter().any(|(n, _)| n == k)
    }
}

fn env(k: &str) -> Option<String> {
    std::env::var_os(k).filter(|v| !v.is_empty()).map(|v| v.to_string_lossy().into_owned())
}

fn say(msg: &str) {
    let _ = writeln!(std::io::stderr(), "bashcut-mcp: {msg}");
}

/// How the agent connects.
struct Decision {
    mode: &'static str,
    argv: Vec<String>,
    project: Option<PathBuf>,
    roots: Roots,
    notes: Vec<String>,
}

fn main() -> ExitCode {
    let a = Args::parse(std::env::args_os().skip(1).map(|s| s.to_string_lossy().into_owned()));
    if a.flag("--help") || a.flag("-h") {
        let _ = write!(std::io::stdout(), "{HELP}");
        return ExitCode::SUCCESS;
    }
    if a.flag("--version") || a.bad.iter().any(|b| b == "-V") {
        let _ = writeln!(std::io::stdout(), "bashcut-mcp {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if let Some(bad) = a.bad.first() {
        say(&format!("unexpected argument {bad:?} (bashcut-mcp --help)"));
        return ExitCode::from(2);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let d = match decide(&a, &cwd) {
        Ok(d) => d,
        Err(e) => {
            say(&e);
            return ExitCode::from(2);
        }
    };
    let guarded = !a.flag("--no-guard") && !d.roots.is_empty();
    if a.flag("--print") {
        let v = json!({
            "mode": d.mode,
            "argv": d.argv,
            "project": d.project.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "roots": d.roots.list().iter().map(|r| bashcut_connect::roots::shown(r)).collect::<Vec<_>>(),
            "guard": guarded,
            "notes": d.notes,
        });
        let _ = writeln!(std::io::stdout(), "{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    for n in &d.notes {
        say(n);
    }
    // a headless engine holds the project's lock while it runs (released when main returns)
    let _lock = match (d.mode, &d.project) {
        ("headless", Some(p)) => match ProjectLock::acquire(p, "headless") {
            Ok(l) => Some(l),
            Err(e) => {
                say(&e);
                return ExitCode::from(2);
            }
        },
        _ => None,
    };
    match serve(&d.argv, guarded.then_some(&d.roots), &cwd) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            say(&e);
            ExitCode::from(1)
        }
    }
}

fn decide(a: &Args, cwd: &Path) -> Result<Decision, String> {
    let cli = find_cli(a);
    let data_dir = a.opt("--data-dir").map(PathBuf::from).or_else(bashcut_dirs::data_dir);
    let project = a.opt("--project").map(str::to_string).or_else(|| env("BASHCUTPRO_PROJECT")).map(|p| roots::absolute(Path::new(&p), cwd));
    let headless = a.flag("--headless");
    let window = data_dir.as_deref().and_then(|d| automation::live(d, Duration::from_millis(500)));
    let mut notes = Vec::new();
    let extra: Vec<PathBuf> = a.opt("--roots").map(|l| std::env::split_paths(l).filter(|p| !p.as_os_str().is_empty()).collect()).unwrap_or_default();
    let roots_env = std::env::var_os("BASHCUTPRO_ROOTS");
    let same = |p: &Path, w: &Option<String>| w.as_deref().is_some_and(|wp| roots::absolute(Path::new(wp), cwd) == p);

    let explicit = a.opt("--bridge").map(str::to_string).or_else(|| env("BASHCUTPRO_BRIDGE"));
    let bridge = if headless { None } else { explicit.or_else(|| window.as_ref().map(|w| w.address.clone())) };
    if let Some(addr) = bridge {
        // the window's project decides where the agent may write
        let wproject = window.as_ref().and_then(|w| w.project.as_deref()).map(|p| roots::absolute(Path::new(p), cwd));
        if let (Some(p), Some(w)) = (&project, &window)
            && !same(p, &w.project)
        {
            notes.push(format!(
                "the window has {} open; the agent edits the window, not {}",
                w.project.as_deref().unwrap_or("an unsaved project"),
                p.display()
            ));
        }
        let project = wproject.or(project);
        let roots = Roots::new(project.as_deref(), roots_env.as_deref(), &extra, cwd);
        if roots.is_empty() {
            notes.push("the window has no saved project open: writes are not limited (save the project, or set BASHCUTPRO_ROOTS)".into());
        }
        let argv = vec![cli, "mcp".into(), "--bridge".into(), addr];
        return Ok(Decision { mode: "bridge", argv, project, roots, notes });
    }

    let mut argv = vec![cli, "mcp".to_string()];
    if let Some(p) = &project {
        if let Some(w) = window.as_ref().filter(|w| same(p, &w.project)) {
            return Err(format!(
                "BashCutPro (pid {}) has {} open: connect to the window (leave out --headless) or close the project there",
                w.pid,
                p.display()
            ));
        }
        if let Some(pid) = bashcut_connect::lock::holder(p).filter(|pid| *pid != std::process::id()) {
            return Err(format!("{} is already open in another engine (pid {pid}); stop it or connect to it", p.display()));
        }
        argv.extend(["--project".to_string(), p.to_string_lossy().into_owned()]);
    } else if headless {
        notes.push("--headless given: headless engine without a project (an open window is left alone), writes are not limited (pass --project or set BASHCUTPRO_ROOTS)".into());
    } else {
        notes.push(
            "no project given and no window found: headless engine without a project, writes are not limited (pass --project or set BASHCUTPRO_ROOTS)".into(),
        );
    }
    if let Some(dd) = a.opt("--data-dir") {
        argv.extend(["--data-dir".to_string(), dd.to_string()]);
    }
    let roots = Roots::new(project.as_deref(), roots_env.as_deref(), &extra, cwd);
    Ok(Decision { mode: "headless", argv, project, roots, notes })
}

/// The engine (#154): `--cli`, else [`bashcut_dirs::engine::default_cli`] (the one lookup, RC1).
fn find_cli(a: &Args) -> String {
    a.opt("--cli").map(str::to_string).unwrap_or_else(|| bashcut_dirs::engine::default_cli().to_string_lossy().into_owned())
}

/// Run the MCP server; with `roots`, every agent line passes [`guard::check`] first. Returns the
/// server's exit status.
fn serve(argv: &[String], roots: Option<&Roots>, cwd: &Path) -> Result<u8, String> {
    let (program, args) = argv.split_first().ok_or("empty command line")?;
    let mut cmd = Command::new(program);
    // `decide` already chose bridge or headless: the engine must not add its own default bridge
    // (CC2) to a headless `mcp` with no project (CC3)
    cmd.args(args).env("BASHCUT_BRIDGE", "off").stderr(Stdio::inherit());
    let Some(roots) = roots else {
        let status = cmd.stdin(Stdio::inherit()).stdout(Stdio::inherit()).status().map_err(|e| format!("{program}: {e}"))?;
        return Ok(exit_code(status.code()));
    };
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().map_err(|e| format!("{program}: {e}"))?;
    let (Some(mut to_engine), Some(from_engine)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("could not connect to the MCP server's stdin / stdout".into());
    };
    // agent → engine, through the guard (refusals go straight back to the agent)
    let roots = roots.clone();
    let cwd = cwd.to_path_buf();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().split(b'\n') {
            let Ok(line) = line else { break };
            if let Some(reply) = guard::check(&String::from_utf8_lossy(&line), &roots, &cwd) {
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{reply}");
                let _ = out.flush();
                continue;
            }
            if to_engine.write_all(&line).and_then(|()| to_engine.write_all(b"\n")).and_then(|()| to_engine.flush()).is_err() {
                break;
            }
        }
        // dropping `to_engine` closes the server's stdin: it ends its session
    });
    // engine → agent, until the server closes its stdout
    pump(from_engine);
    let status = child.wait().map_err(|e| format!("{program}: {e}"))?;
    Ok(exit_code(status.code()))
}

fn pump(from: impl Read) {
    for line in BufReader::new(from).split(b'\n') {
        let Ok(line) = line else { break };
        let mut out = std::io::stdout().lock();
        if out.write_all(&line).and_then(|()| out.write_all(b"\n")).and_then(|()| out.flush()).is_err() {
            break;
        }
    }
}

fn exit_code(code: Option<i32>) -> u8 {
    code.and_then(|c| u8::try_from(c).ok()).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn args(v: &[&str]) -> Args {
        Args::parse(v.iter().map(|s| s.to_string()))
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bashcut-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("data/bashcut")).unwrap();
        std::fs::create_dir_all(d.join("proj")).unwrap();
        d
    }

    fn window(d: &Path, addr: &str, project: Option<&Path>) {
        let v = json!({"schema": automation::SCHEMA, "address": addr, "token": null, "pid": std::process::id(), "project": project.map(|p| p.to_string_lossy().into_owned())});
        std::fs::write(automation::file(&d.join("data")), v.to_string()).unwrap();
    }

    #[test]
    fn bridges_to_a_live_window_and_limits_writes_to_its_project() {
        let d = scratch("bridge");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let project = d.join("proj/a.fcproj");
        window(&d, &addr, Some(&project));
        let data = d.join("data").to_string_lossy().into_owned();
        let dec = decide(&args(&["--data-dir", &data, "--cli", "fc"]), &d).unwrap();
        assert_eq!(dec.mode, "bridge");
        assert_eq!(dec.argv, vec!["fc".to_string(), "mcp".into(), "--bridge".into(), addr]);
        assert_eq!(dec.project.as_deref(), Some(project.as_path()));
        assert!(dec.roots.allows(&d.join("proj/x.mp4").to_string_lossy(), &d));
        assert!(!dec.roots.allows(&d.join("x.mp4").to_string_lossy(), &d));
        // a headless engine on the window's project is refused
        let p = project.to_string_lossy().into_owned();
        let err = decide(&args(&["--data-dir", &data, "--cli", "fc", "--headless", "--project", &p]), &d).err().unwrap();
        assert!(err.contains("has"), "{err}");
        drop(listener);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn headless_without_a_window() {
        let d = scratch("headless");
        let data = d.join("data").to_string_lossy().into_owned();
        let p = d.join("proj").join("a.fcproj").to_string_lossy().into_owned();
        let dec = decide(&args(&["--data-dir", &data, "--cli", "fc", "--project", &p]), &d).unwrap();
        assert_eq!(dec.mode, "headless");
        assert_eq!(dec.argv[..4], ["fc".to_string(), "mcp".into(), "--project".into(), p.clone()]);
        assert!(dec.argv.contains(&"--data-dir".to_string()));
        assert_eq!(dec.roots.list().len(), 1);
        // a stale window file (nothing listens) does not count
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        window(&d, &format!("127.0.0.1:{port}"), None);
        assert_eq!(decide(&args(&["--data-dir", &data, "--cli", "fc", "--project", &p]), &d).unwrap().mode, "headless");
        // no project: no limit, and a note says so
        let dec = decide(&args(&["--data-dir", &data, "--cli", "fc"]), &d).unwrap();
        assert!(dec.roots.is_empty() || std::env::var_os("BASHCUTPRO_ROOTS").is_some());
        assert!(!dec.notes.is_empty() || std::env::var_os("BASHCUTPRO_PROJECT").is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn headless_by_choice_says_so_even_with_a_live_window() {
        // FX6: with --headless and a live window, the note must not claim no window was found
        let d = scratch("headless-choice");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        window(&d, &listener.local_addr().unwrap().to_string(), None);
        let data = d.join("data").to_string_lossy().into_owned();
        let dec = decide(&args(&["--data-dir", &data, "--cli", "fc", "--headless"]), &d).unwrap();
        assert_eq!(dec.mode, "headless");
        if std::env::var_os("BASHCUTPRO_PROJECT").is_none() {
            assert!(dec.notes.iter().any(|n| n.starts_with("--headless given")), "{:?}", dec.notes);
        }
        assert!(dec.notes.iter().all(|n| !n.contains("no window found")), "{:?}", dec.notes);
        // without --headless and with no window, the old note stays
        drop(listener);
        let dec = decide(&args(&["--data-dir", &data, "--cli", "fc"]), &d).unwrap();
        if std::env::var_os("BASHCUTPRO_PROJECT").is_none() && std::env::var_os("BASHCUTPRO_BRIDGE").is_none() {
            assert!(dec.notes.iter().any(|n| n.contains("no window found")), "{:?}", dec.notes);
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
