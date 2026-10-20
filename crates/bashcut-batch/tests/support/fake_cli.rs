//! A stand-in for `filmcraft-cli` used by `tests/batch.rs`: same argument shape (`run <script>`,
//! `export <out>`, `--project`, `--save`, `--save-as`, `--preset`) and exit statuses, no engine.
//!
//! A "project" is a text file listing the command ids run on it. Script ids it understands:
//! `fake.fail` (prints a failed line, exit 1), `fake.failIf` (fails when `params.name` equals
//! `params.fail`), `fake.sleep` (`params.ms`), `fake.crash` (exit 101), `fake.usage` (exit 2);
//! any other id succeeds. `export --preset offline` reports offline media (exit 1);
//! `plugin-handoff` acts as a plugin that hands off to the agent (exit 3). With
//! `FAKE_CLI_LOG=<file>` every process appends `start <ms>` / `end <ms>` lines, for concurrency checks.
//! `exec transcript.models` prints `$FAKE_CLI_MODELS` (exit 1 when unset); `sequence.transcribe`
//! records `sequence.transcribe model=<params.model>` in the project.
//!
//! Sequences (#195): `file.newSequenceFromClip` makes item 2 ("source"); a `sequence:<id>:<name>[:blocking]`
//! line adds one (blocking: its review has a finding until a `fixed:<name>` line). `sequence.open`
//! records `sequence.open item=<n>` (the last one is active); `project.inspect` lists them.

use serde_json::{Value, json};
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VALUED: &[&str] = &["--project", "--save-as", "--preset", "--format", "--settings", "--data-dir", "--mode", "--prompt", "--session", "--sleep-ms"];

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
        // one write per line: `writeln!` writes in pieces, and concurrent processes interleaved them
        let _ = f.write_all(format!("{what} {} {}\n", now_ms(), std::process::id()).as_bytes());
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
                // the review gate: one blocking finding until an agent wrote "fixed" into the project
                let result = if id == "project.inspect" {
                    inspect(&project)
                } else if id == "bashcut.review.run" {
                    let target = params["sequence"].as_u64().or_else(|| active(&project));
                    if params["sequence"].is_u64() && target != active(&project) {
                        println!("{}", json!({"line": n + 1, "id": id, "ok": false, "error": "not the active sequence"}));
                        return 1;
                    }
                    // the source sequence keeps the plain rule above
                    let blocking = match target.and_then(|t| sequences(&project).into_iter().find(|s| s.0 == t && s.0 != 2)) {
                        Some((_, name, bad)) => u64::from(bad && !project.contains(&format!("fixed:{name}\n"))),
                        None => u64::from(!project.contains("fixed\n")),
                    };
                    let findings: Vec<Value> = (0..blocking)
                        .map(|_| json!({"category": "captions", "check": "captions.readingSpeed", "severity": "blocking", "start": 1.0, "end": 2.0, "message": "24 characters a second", "fix": "split the caption"}))
                        .collect();
                    json!({"ok": blocking == 0, "verdict": if blocking == 0 { "pass" } else { "needsFix" }, "blocking": blocking, "warnings": 0, "scores": {"captions": 100 - 40 * blocking}, "findings": findings})
                } else {
                    Value::Null
                };
                println!("{}", json!({"line": n + 1, "id": id, "ok": true, "result": result}));
                project.push_str(&id);
                if id == "sequence.open" {
                    project.push_str(&format!(" item={}", params["item"].as_u64().unwrap_or(0)));
                }
                if id == "sequence.transcribe" {
                    project.push_str(&format!(" model={}", params["model"].as_str().unwrap_or("-")));
                }
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
        Some("exec") if pos.get(1).map(String::as_str) == Some("transcript.models") => match std::env::var("FAKE_CLI_MODELS") {
            Ok(models) => {
                println!("{models}");
                0
            }
            Err(_) => 1,
        },
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
        // an agent (Claude Code's stream-json): edits the project file itself, as an agent saving
        // through MCP would; --mode ok (writes "fixed" on a fix round), fixfirst, stubborn, loop, sleep;
        // clips makes three clip sequences (clip-1 and clip-3 blocking, clip-2 left active) and on a fix
        // round fixes each clip the prompt names; clipsstubborn makes them and never fixes
        Some("fake-agent") => {
            log("agent-start");
            let mode = opt("--mode").unwrap_or_default();
            let prompt = opt("--prompt").unwrap_or_default();
            let session = opt("--session").filter(|s| !s.is_empty());
            if let Some(ms) = opt("--sleep-ms").and_then(|m| m.parse::<u64>().ok()) {
                std::thread::sleep(Duration::from_millis(ms));
            }
            if mode == "sleep" {
                std::thread::sleep(Duration::from_secs(10));
            }
            if let Some(s) = &session {
                project.push_str(&format!("resume:{s}\n"));
            }
            project.push_str("agent\n");
            // the kit (#204): what the runner passed in the environment and the prompt
            if let Some(k) = std::env::var_os("BASHCUT_KIT") {
                project.push_str(&format!("kit-env:{}\n", k.to_string_lossy()));
            }
            if prompt.contains("/skills/<name>/SKILL.md") {
                project.push_str("kit-in-prompt\n");
            }
            // #223: the interpreter for the kit's scripts, in the environment and the prompt
            if let Some(p) = std::env::var_os("BASHCUT_PYTHON") {
                let p = p.to_string_lossy().into_owned();
                project.push_str(&format!("python-env:{p}\n"));
                if prompt.contains(&format!("scripts with {p} (BASHCUT_PYTHON")) {
                    project.push_str("python-in-prompt\n");
                }
            }
            let fix_round = prompt.contains("found blocking problems");
            if mode.starts_with("clips") && !project.contains("sequence:3:") {
                project.push_str("sequence:3:clip-1:blocking\nsequence:4:clip-2\nsequence:5:clip-3:blocking\nsequence.open item=4\n");
            }
            if mode == "clips" && fix_round {
                for (_, name, _) in sequences(&project) {
                    if prompt.contains(&format!("\"{name}\"")) {
                        project.push_str(&format!("fixed:{name}\n"));
                    }
                }
            }
            if mode == "fixfirst" || (mode == "ok" && fix_round) {
                project.push_str("fixed\n");
            }
            if opt("--project").is_some_and(|p| std::fs::write(p, &project).is_err()) {
                return 1;
            }
            let sid = session.unwrap_or_else(|| format!("sess-{}", std::process::id()));
            println!("{}", json!({"type": "system", "subtype": "init", "session_id": sid}));
            let subtype = if mode == "loop" { "error_max_turns" } else { "success" };
            println!(
                "{}",
                json!({"type": "result", "subtype": subtype, "is_error": false, "num_turns": 3, "session_id": sid, "total_cost_usd": 0.01, "usage": {"input_tokens": 100, "output_tokens": 20}, "result": "done"})
            );
            log("agent-end");
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

/// (id, name, blocking) of each sequence in a fake project.
fn sequences(project: &str) -> Vec<(u64, String, bool)> {
    let mut v = Vec::new();
    if project.lines().any(|l| l == "file.newSequenceFromClip") {
        v.push((2, "source".to_string(), false));
    }
    for l in project.lines() {
        let mut parts = l.split(':');
        if parts.next() != Some("sequence") {
            continue;
        }
        let (Some(id), Some(name)) = (parts.next().and_then(|i| i.parse().ok()), parts.next()) else { continue };
        v.push((id, name.to_string(), parts.next() == Some("blocking")));
    }
    v
}

fn active(project: &str) -> Option<u64> {
    let opened = project.lines().rev().find_map(|l| l.strip_prefix("sequence.open item=")).and_then(|i| i.parse().ok());
    opened.or_else(|| sequences(project).first().map(|s| s.0))
}

/// `project.inspect`: the media item at the root, sequences made by an agent in a "Clips" bin.
fn inspect(project: &str) -> Value {
    let seq = |(id, name, _): &(u64, String, bool)| json!({"item": id, "name": name, "type": "Sequence"});
    let all = sequences(project);
    let (source, clips): (Vec<_>, Vec<_>) = all.iter().partition(|s| s.0 == 2);
    let mut children = vec![json!({"item": 1, "name": "clip.mp4", "type": "Movie"})];
    children.extend(source.into_iter().map(seq));
    children.push(json!({"id": 9, "name": "Clips", "children": clips.into_iter().map(seq).collect::<Vec<_>>()}));
    json!({"root": {"id": 0, "name": "root", "children": children}, "activeSequence": active(project)})
}
