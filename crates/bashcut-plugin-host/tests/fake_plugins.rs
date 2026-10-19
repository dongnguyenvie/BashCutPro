//! The plugin host against fake Python plugins (stdlib only): ok, error, hand-off, hang → timeout,
//! cancel, oversized output, garbage output, and install from a local git registry.
//! Skips (with a note) when `python3` or `git` is not on PATH.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bashcut_plugin_host::host::RunContext;
use bashcut_plugin_host::runner::{End, Event, RunSpec, Running};
use bashcut_plugin_host::settings::MemoryStore;
use bashcut_plugin_host::{Host, JobState};
use serde_json::{Value, json};

static N: AtomicU64 = AtomicU64::new(0);

fn has(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok_and(|o| o.status.success())
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bashcut-host-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// One fake plugin: `plugin.py <mode>` behaves as the mode says.
const PLUGIN_PY: &str = r#"
import json, os, sys, time
mode = sys.argv[1] if len(sys.argv) > 1 else "ok"
args = sys.argv[2:]
def progress(m):
    print("[fake] " + m, file=sys.stderr, flush=True)
if mode == "ok":
    progress("10% starting")
    progress("90% almost")
    print(json.dumps({"ok": True, "message": "done", "data": {"args": args, "key": os.environ.get("FAKE_KEY"), "service": os.environ.get("FAKE_SERVICE")}}))
elif mode == "error":
    print(json.dumps({"ok": False, "error": {"code": "bad_input", "message": "no captions"}}))
    sys.exit(1)
elif mode == "handoff":
    print(json.dumps({"ok": False, "error": {"code": "no_key", "message": "agent must translate"}, "data": {"lines": ["a", "b"]}}))
    sys.exit(3)
elif mode == "hang":
    progress("waiting forever")
    time.sleep(600)
elif mode == "big":
    sys.stdout.write("x" * (3 * 1024 * 1024))
    sys.stdout.write("\n" + json.dumps({"ok": True, "message": "big"}) + "\n")
elif mode == "garbage":
    print("not json at all")
elif mode == "crash":
    os.kill(os.getpid(), 9)
"#;

fn manifest(id: &str) -> Value {
    let mode =
        |m: &str| json!({"id": m, "title": m, "args": [m], "needs": "none", "params": [{"id": "word", "flag": "--word", "type": "string", "optional": true}]});
    json!({
        "schema": "bashcut-plugin/1", "id": id, "name": "Fake", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"]},
        "connect": {"bridge": ["--bridge", "{address}"]},
        "output": "bashcut-result/1",
        "settings": [
            {"id": "service", "title": "Service", "type": "enum", "choices": ["a", "b"], "default": "a", "env": ["FAKE_SERVICE"]},
            {"id": "apiKey", "title": "Key", "type": "secret", "env": ["FAKE_KEY"], "optional": true}
        ],
        "actions": [mode("ok"), mode("error"), {"id": "handoff", "title": "Hand off", "args": ["handoff"], "needs": "none", "params": [], "handoff": {"exit": 3, "then": "apply"}}, mode("hang"), mode("big"), mode("garbage"), mode("crash"),
            {"id": "edit", "title": "Edit", "args": ["ok"], "needs": "project", "params": []}]
    })
}

fn write_plugin(dir: &Path, id: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("plugin.py"), PLUGIN_PY).unwrap();
    std::fs::write(dir.join("bashcut-plugin.json"), manifest(id).to_string()).unwrap();
}

fn run_mode(dir: &Path, mode: &str, timeout: Duration) -> (Vec<Event>, bashcut_plugin_host::Outcome) {
    let mut spec = RunSpec::new(dir.to_path_buf(), vec!["python3".into(), "plugin.py".into(), mode.into()]);
    spec.timeout = timeout;
    spec.max_stdout = 64 * 1024;
    let mut r = Running::start(spec).unwrap();
    let (events, outcome) = r.wait(Duration::from_secs(60));
    (events, outcome.expect("run ended"))
}

#[test]
fn runner_outcomes() {
    if !has("python3") {
        eprintln!("skipped: no python3");
        return;
    }
    let dir = scratch("runner");
    write_plugin(&dir, "test.fake");

    let (events, o) = run_mode(&dir, "ok", Duration::from_secs(30));
    assert_eq!(o.end, End::Succeeded, "{o:?}");
    assert_eq!(o.result.as_ref().unwrap().message, "done");
    let fractions: Vec<Option<f32>> = events.iter().map(|e| if let Event::Progress { fraction, .. } = e { *fraction } else { None }).collect();
    assert_eq!(fractions, [Some(0.1), Some(0.9)]);

    let (_, o) = run_mode(&dir, "error", Duration::from_secs(30));
    assert_eq!((o.end, o.exit_code), (End::Failed, Some(1)));
    assert_eq!(o.summary(), "bad_input: no captions");

    let (_, o) = run_mode(&dir, "handoff", Duration::from_secs(30));
    assert_eq!(o.end, End::HandOff);
    assert_eq!(o.result.unwrap().data["lines"], json!(["a", "b"]));

    let start = Instant::now();
    let (_, o) = run_mode(&dir, "hang", Duration::from_millis(800));
    assert_eq!(o.end, End::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(10), "killed promptly");

    let (_, o) = run_mode(&dir, "big", Duration::from_secs(30));
    assert!(o.stdout_truncated, "stdout capped");
    assert_eq!(o.end, End::Succeeded);
    assert!(o.result.is_none(), "the result was past the cap");

    let (_, o) = run_mode(&dir, "garbage", Duration::from_secs(30));
    assert_eq!(o.end, End::Succeeded);
    assert!(o.error.unwrap().contains("bashcut-result/1"));

    #[cfg(unix)]
    {
        let (_, o) = run_mode(&dir, "crash", Duration::from_secs(30));
        assert_eq!(o.end, End::Crashed);
    }

    assert!(Running::start(RunSpec::new(dir.clone(), vec!["/no/such/program".into()])).is_err());
    assert!(Running::start(RunSpec::new(dir.clone(), vec![])).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancel_stops_a_run() {
    if !has("python3") {
        eprintln!("skipped: no python3");
        return;
    }
    let dir = scratch("cancel");
    write_plugin(&dir, "test.fake");
    let mut r = Running::start(RunSpec::new(dir.clone(), vec!["python3".into(), "plugin.py".into(), "hang".into()])).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    r.cancel();
    let (_, o) = r.wait(Duration::from_secs(10));
    assert_eq!(o.expect("ended").end, End::Cancelled);
    let _ = std::fs::remove_dir_all(&dir);
}

fn wait_job(host: &mut Host, job: u64) -> JobState {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(120) {
        host.poll();
        if let Some(j) = host.job(job)
            && !j.state.is_running()
        {
            return j.state;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("job {job} did not finish");
}

fn git(dir: &Path, args: &[&str]) {
    let ok =
        Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
}

/// A local registry repository with one plugin at `plugins/fake`.
fn registry_repo(version: &str) -> PathBuf {
    let repo = scratch("repo");
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    let mut m = manifest("test.fake");
    m["version"] = json!(version);
    write_plugin(&repo.join("plugins/fake"), "test.fake");
    std::fs::write(repo.join("plugins/fake/bashcut-plugin.json"), m.to_string()).unwrap();
    let reg = json!({"schema": "bashcut-registry/1", "name": "test", "repo": repo.to_string_lossy(), "plugins": [
        {"id": "test.fake", "name": "Fake", "version": version, "summary": "test plugin", "path": "plugins/fake",
         "source": {"type": "git", "repo": repo.to_string_lossy(), "ref": "main", "path": "plugins/fake"}}
    ]});
    std::fs::write(repo.join("registry.json"), reg.to_string()).unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "--quiet", "-m", "plugins"]);
    repo
}

#[test]
fn install_settings_run_update_remove() {
    if !has("python3") || !has("git") {
        eprintln!("skipped: needs python3 and git");
        return;
    }
    let data = scratch("data");
    let repo = registry_repo("0.1.0");
    let secrets = Arc::new(MemoryStore::default());
    let mut host = Host::new(Some(data.clone()), secrets);
    let ctx = RunContext { bridge: Some("127.0.0.1:9".into()), cli: None, known_commands: Vec::new() };

    host.command("registry", &json!({"location": format!("{}@main", repo.display())}), &ctx).unwrap();
    let job = host.jobs().last().unwrap().id;
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded, "{:?}", host.job(job));
    let list = host.command("list", &json!({}), &ctx).unwrap();
    assert_eq!(list["registry"][0]["id"], json!("test.fake"));
    assert!(host.command("run", &json!({"plugin": "test.fake", "action": "ok"}), &ctx).is_err(), "not installed yet");

    let job = host.command("install", &json!({"plugin": "test.fake"}), &ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded, "{:?}", host.job(job));
    assert!(data.join("bashcut-plugins/test.fake/bashcut-plugin.json").is_file());
    assert!(!data.join("bashcut-plugins/test.fake/.git").exists());

    // settings and secrets reach the run as env; params as flags; the bridge address is passed
    host.command("settings", &json!({"plugin": "test.fake", "values": {"service": "b"}}), &ctx).unwrap();
    assert!(host.command("settings", &json!({"plugin": "test.fake", "values": {"apiKey": "x"}}), &ctx).is_err());
    let s = host.command("setSecret", &json!({"plugin": "test.fake", "setting": "apiKey", "value": "sk-test"}), &ctx).unwrap();
    assert_eq!(s["secrets"]["apiKey"], json!(true));
    let job = host.command("run", &json!({"plugin": "test.fake", "action": "ok", "params": {"word": "hi"}}), &ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded);
    let j = host.command("job", &json!({"job": job}), &ctx).unwrap();
    let data_out = &j["outcome"]["result"]["data"];
    assert_eq!(data_out["key"], json!("sk-test"));
    assert_eq!(data_out["service"], json!("b"));
    assert_eq!(data_out["args"], json!(["--bridge", "127.0.0.1:9", "--word", "hi"]));
    assert_eq!(j["progress"], json!(1.0));
    assert!(j["log"].as_array().unwrap().len() >= 2);

    // hand-off names the follow-up action; bad params never start a process
    let job = host.command("run", &json!({"plugin": "test.fake", "action": "handoff"}), &ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, job), JobState::HandOff);
    assert_eq!(host.job(job).unwrap().then.as_deref(), Some("apply"));
    assert!(host.command("run", &json!({"plugin": "test.fake", "action": "ok", "params": {"nope": 1}}), &ctx).is_err());
    let no_bridge = RunContext::default();
    assert!(host.command("run", &json!({"plugin": "test.fake", "action": "edit"}), &no_bridge).is_err(), "needs the open app");
    let missing = RunContext { known_commands: vec!["file.save".into()], ..ctx.clone() };
    let mut m = manifest("test.fake");
    m["app"] = json!({"commands": ["captions.list"]});
    std::fs::write(data.join("bashcut-plugins/test.fake/bashcut-plugin.json"), m.to_string()).unwrap();
    host.refresh();
    assert!(host.command("run", &json!({"plugin": "test.fake", "action": "ok"}), &missing).unwrap_err().0.contains("captions.list"));

    // cancel from the command
    let job = host.command("run", &json!({"plugin": "test.fake", "action": "hang"}), &ctx).unwrap()["job"].as_u64().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    host.command("cancel", &json!({"job": job}), &ctx).unwrap();
    assert_eq!(wait_job(&mut host, job), JobState::Cancelled);

    // update keeps settings
    let repo2 = registry_repo("0.2.0");
    host.command("registry", &json!({"location": repo2.display().to_string()}), &ctx).unwrap();
    let job = host.jobs().last().unwrap().id;
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded);
    assert_eq!(host.update_for("test.fake").as_deref(), Some("0.2.0"));
    let job = host.command("update", &json!({"plugin": "test.fake"}), &ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded, "{:?}", host.job(job));
    assert_eq!(host.update_for("test.fake"), None);
    assert_eq!(host.settings("test.fake").unwrap()["service"], json!("b"), "settings survive an update");

    let runs = host.command("runs", &json!({}), &ctx).unwrap();
    assert!(runs.as_array().unwrap().len() >= 6);
    host.command("clear", &json!({}), &ctx).unwrap();
    assert!(host.jobs().is_empty());

    host.command("remove", &json!({"plugin": "test.fake"}), &ctx).unwrap();
    assert!(!data.join("bashcut-plugins/test.fake").exists());
    assert!(host.command("remove", &json!({"plugin": "test.fake"}), &ctx).is_err());
    for d in [data, repo, repo2] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn no_data_dir_means_nothing_to_install() {
    let mut host = Host::new(None, Arc::new(MemoryStore::default()));
    let ctx = RunContext::default();
    assert_eq!(host.command("list", &json!({}), &ctx).unwrap()["canInstall"], json!(false));
    assert!(host.command("registry", &json!({}), &ctx).is_err());
    assert!(host.command("install", &json!({"plugin": "x.y"}), &ctx).is_err());
    assert!(host.command("nope", &json!({}), &ctx).is_err());
    assert!(host.command("cancel", &json!({"job": 99}), &ctx).is_err());
}
