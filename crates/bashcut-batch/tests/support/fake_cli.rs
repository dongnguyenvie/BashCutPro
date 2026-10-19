//! A stand-in for `filmcraft-cli` used by `tests/batch.rs`: same argument shape (`run <script>`,
//! `export <out>`, `--project`, `--save`, `--save-as`, `--preset`) and exit statuses, no engine.
//!
//! A "project" is a text file listing the command ids run on it. Script ids it understands:
//! `fake.fail` (prints a failed line, exit 1), `fake.failIf` (fails when `params.name` equals
//! `params.fail`), `fake.sleep` (`params.ms`), `fake.crash` (exit 101), `fake.usage` (exit 2);
//! any other id succeeds. `export --preset offline` reports offline media (exit 1);
//! `plugin-handoff` acts as a plugin that hands off to the agent (exit 3). With
//! `FAKE_CLI_LOG=<file>` every process appends `start <ms>` / `end <ms>` lines, for concurrency checks.

use serde_json::{Value, json};
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VALUED: &[&str] = &["--project", "--save-as", "--preset", "--format", "--settings", "--data-dir"];

fn main() {
    let code = real_main();
    log("end");
    std::process::exit(code);
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

fn log(what: &str) {
    let Some(path) = std::env::var_os("FAKE_CLI_LOG") else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{what} {} {}", now_ms(), std::process::id());
    }
}

fn real_main() -> i32 {
    log("start");
    let mut pos: Vec<String> = Vec::new();
    let mut opts: Vec<(String, Option<String>)> = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if VALUED.contains(&a.as_str()) {
            let v = it.next();
            opts.push((a, v));
        } else if a.starts_with("--") {
            opts.push((a, None));
        } else {
            pos.push(a);
        }
    }
    let opt = |k: &str| opts.iter().rev().find(|(n, _)| n == k).and_then(|(_, v)| v.clone());
    let flag = |k: &str| opts.iter().any(|(n, _)| n == k);
    let mut project = match opt("--project") {
        Some(p) => match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("fake-cli: {p}: {e}");
                return 1;
            }
        },
        None => String::new(),
    };
    match pos.first().map(String::as_str) {
        Some("run") => {
            let Some(script) = pos.get(1) else { return 2 };
            let Ok(text) = std::fs::read_to_string(script) else { return 1 };
            for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
                let Ok(v) = serde_json::from_str::<Value>(line) else { return 2 };
                let id = v["id"].as_str().unwrap_or_default().to_string();
                let params = &v["params"];
                let fail = match id.as_str() {
                    "fake.fail" => true,
                    "fake.failIf" => params["name"].is_string() && params["name"] == params["fail"],
                    "fake.sleep" => {
                        std::thread::sleep(Duration::from_millis(params["ms"].as_u64().unwrap_or(0)));
                        false
                    }
                    "fake.crash" => return 101,
                    // an output profile export: writes the file like `export` does
                    "bashcut.profiles.apply" => {
                        let out = params["path"].as_str().unwrap_or_default();
                        std::fs::write(out, format!("export profile={}\n{project}", params["profile"].as_str().unwrap_or_default())).is_err()
                    }
                    "fake.usage" => {
                        eprintln!("fake-cli: line {}: bad params", n + 1);
                        return 2;
                    }
                    _ => false,
                };
                if fail {
                    println!("{}", json!({"line": n + 1, "id": id, "ok": false, "error": "fake failure"}));
                    return 1;
                }
                println!("{}", json!({"line": n + 1, "id": id, "ok": true, "result": null}));
                project.push_str(&id);
                project.push('\n');
            }
            let target = if let Some(p) = opt("--save-as") {
                Some(p)
            } else if flag("--save") {
                opt("--project")
            } else {
                None
            };
            if target.is_some_and(|t| std::fs::write(t, &project).is_err()) {
                return 1;
            }
            0
        }
        Some("export") => {
            let Some(out) = pos.get(1) else { return 2 };
            let preset = opt("--preset").unwrap_or_default();
            if preset == "offline" {
                println!("{}", json!({"path": out, "offlineMedia": ["clip.mp4"]}));
                eprintln!("fake-cli: offline media drawn as placeholders");
                return 1;
            }
            if let Ok(ms) = std::env::var("FAKE_CLI_EXPORT_MS").unwrap_or_default().parse::<u64>() {
                std::thread::sleep(Duration::from_millis(ms));
            }
            let body = format!("export preset={preset}\n{project}");
            if std::fs::write(out, body).is_err() {
                return 1;
            }
            println!("{}", json!({"path": out}));
            0
        }
        // a plugin that needs the calling agent (`bashcut-result/1`, exit 3)
        Some("plugin-handoff") => {
            println!("{}", json!({"ok": false, "message": "needs the agent", "data": {"ask": "pick a voice"}}));
            3
        }
        _ => 2,
    }
}
