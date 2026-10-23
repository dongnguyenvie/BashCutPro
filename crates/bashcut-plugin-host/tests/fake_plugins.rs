//! The plugin host against fake Python plugins (stdlib only): ok, error, hand-off, hang → timeout,
//! cancel, oversized output, garbage output, and install from a local git registry.
//! Skips (with a note) when neither `python3` nor `python` runs, or `git` is not on PATH.

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

/// `python3`, else `python` (Windows installs only the latter; its `python3` is a store stub that fails).
fn python() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|p| has(p))
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
    print(json.dumps({"ok": True, "message": "done", "data": {"args": args, "key": os.environ.get("FAKE_KEY"), "service": os.environ.get("FAKE_SERVICE"), "cli": os.environ.get("BASHCUTPRO_CLI"), "dataDir": os.environ.get("BASHCUTPRO_DATA_DIR"), "token": os.environ.get("FILMCRAFT_CONTROL_TOKEN")}}))
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
elif mode == "usage":
    print("fake: error: unrecognized arguments: --nope", file=sys.stderr, flush=True)
    sys.exit(2)
elif mode == "crash":
    os.kill(os.getpid(), 9)
"#;

fn manifest(id: &str) -> Value {
    let py = python().unwrap_or("python3");
    let mode =
        |m: &str| json!({"id": m, "title": m, "args": [m], "needs": "none", "params": [{"id": "word", "flag": "--word", "type": "string", "optional": true}]});
    json!({
        "schema": "bashcut-plugin/1", "id": id, "name": "Fake", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": [py, "plugin.py"]},
        "connect": {"bridge": ["--bridge", "{address}"]},
        "output": "bashcut-result/1",
        "settings": [
            {"id": "service", "title": "Service", "type": "enum", "choices": ["a", "b"], "default": "a", "env": ["FAKE_SERVICE"]},
            {"id": "apiKey", "title": "Key", "type": "secret", "env": ["FAKE_KEY"], "optional": true}
        ],
        "actions": [mode("ok"), mode("error"), {"id": "handoff", "title": "Hand off", "args": ["handoff"], "needs": "none", "params": [], "handoff": {"exit": 3, "then": "apply"}}, mode("hang"), mode("big"), mode("garbage"), mode("crash"), mode("usage"),
            {"id": "edit", "title": "Edit", "args": ["ok"], "needs": "project", "params": []}]
    })
}

fn write_plugin(dir: &Path, id: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("plugin.py"), PLUGIN_PY).unwrap();
    std::fs::write(dir.join("bashcut-plugin.json"), manifest(id).to_string()).unwrap();
}

fn run_mode(dir: &Path, mode: &str, timeout: Duration) -> (Vec<Event>, bashcut_plugin_host::Outcome) {
    let py = python().expect("checked by the caller");
    let mut spec = RunSpec::new(dir.to_path_buf(), vec![py.into(), "plugin.py".into(), mode.into()]);
    spec.timeout = timeout;
    spec.max_stdout = 64 * 1024;
    let mut r = Running::start(spec).unwrap();
    let (events, outcome) = r.wait(Duration::from_secs(60));
    (events, outcome.expect("run ended"))
}

#[test]
fn runner_outcomes() {
    if python().is_none() {
        eprintln!("skipped: no python3 or python");
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
    if python().is_none() {
        eprintln!("skipped: no python3 or python");
        return;
    }
    let dir = scratch("cancel");
    write_plugin(&dir, "test.fake");
    let mut r = Running::start(RunSpec::new(dir.clone(), vec![python().unwrap().into(), "plugin.py".into(), "hang".into()])).unwrap();
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

/// The `ok` action's reported `token` (the run's `FILMCRAFT_CONTROL_TOKEN`) for a run with `ctx`.
fn run_token(data: &Path, ctx: &RunContext) -> Value {
    let mut host = Host::new(Some(data.to_path_buf()), Arc::new(MemoryStore::default()));
    let job = host.command("run", &json!({"plugin": "test.fake", "action": "ok"}), ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded, "{:?}", host.job(job));
    host.job(job).unwrap().outcome.as_ref().unwrap().result.as_ref().unwrap().data["token"].clone()
}

#[test]
fn control_token_reaches_the_run_env() {
    if python().is_none() {
        eprintln!("skipped: no python3 or python");
        return;
    }
    let data = scratch("token");
    write_plugin(&data.join("bashcut-plugins/test.fake"), "test.fake");
    let token = "ab".repeat(32);
    let with = |bridge: Option<&str>, token: Option<&str>| RunContext {
        bridge: bridge.map(str::to_string),
        control_token: token.map(str::to_string),
        ..RunContext::default()
    };
    if std::env::var_os("FILMCRAFT_CONTROL_TOKEN").is_some() {
        eprintln!("skipped: FILMCRAFT_CONTROL_TOKEN is set in the test environment");
        return;
    }
    assert_eq!(run_token(&data, &with(Some("127.0.0.1:9"), Some(&token))), json!(token), "token and bridge: the run gets it");
    assert_eq!(run_token(&data, &with(Some("127.0.0.1:9"), None)), Value::Null, "a bridge without a token");
    assert_eq!(run_token(&data, &with(None, Some(&token))), Value::Null, "a token without a bridge is never passed");
    assert_eq!(run_token(&data, &RunContext::default()), Value::Null);
}

#[test]
fn install_settings_run_update_remove() {
    if python().is_none() || !has("git") {
        eprintln!("skipped: needs python3 (or python) and git");
        return;
    }
    let data = scratch("data");
    let repo = registry_repo("0.1.0");
    let secrets = Arc::new(MemoryStore::default());
    let mut host = Host::new(Some(data.clone()), secrets);
    let ctx = RunContext { bridge: Some("127.0.0.1:9".into()), cli: Some("/apps/bashcutpro".into()), known_commands: Vec::new(), control_token: None };

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

    // settings and secrets reach the run as env; params as flags; an action that needs nothing gets no bridge
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
    assert_eq!(data_out["cli"], json!("/apps/bashcutpro"), "the engine reaches plugins as BASHCUTPRO_CLI (#154)");
    assert_eq!(data_out["args"], json!(["--word", "hi"]));
    assert_eq!(j["progress"], json!(1.0));
    assert_eq!(j["params"], json!({"word": "hi"}), "kept for Retry");
    assert!(j["log"].as_array().unwrap().len() >= 2);
    // one that needs the project gets the bridge address
    let job = host.command("run", &json!({"plugin": "test.fake", "action": "edit"}), &ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded);
    assert_eq!(host.job(job).unwrap().outcome.as_ref().unwrap().result.as_ref().unwrap().data["args"], json!(["--bridge", "127.0.0.1:9"]));
    // a failure with no result says why, not only the exit status
    let job = host.command("run", &json!({"plugin": "test.fake", "action": "usage"}), &ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, job), JobState::Failed);
    let msg = host.job(job).unwrap().message.clone();
    assert!(msg.starts_with("exit status 2") && msg.contains("unrecognized arguments: --nope"), "{msg}");

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

#[test]
fn agent_plugins_are_listed_for_the_agent_section() {
    let data = scratch("agents");
    let root = bashcut_plugin_host::plugins_dir(&data);
    let mut m = manifest("test.agent");
    m["kind"] = json!("agent");
    m["entry"] = json!({"command": ["./chat", "--quiet"]});
    m["agent"] = json!({"args": ["turn"], "mcpConfig": ["--mcp", "{config}"], "resume": ["--session", "{session}"]});
    std::fs::create_dir_all(root.join("test.agent")).unwrap();
    std::fs::write(root.join("test.agent").join("bashcut-plugin.json"), m.to_string()).unwrap();
    std::fs::write(root.join("test.agent").join("settings.json"), r#"{"service":"b"}"#).unwrap();
    write_plugin(&root.join("test.tool"), "test.tool");
    let host = Host::new(Some(data.clone()), Arc::new(MemoryStore::default()));
    let agents = host.agents();
    assert_eq!(agents.len(), 1, "tool plugins are not agents");
    let a = &agents[0];
    assert_eq!((a.id.as_str(), a.name.as_str(), a.events.as_str()), ("test.agent", "Fake", "claude-stream-json"));
    assert_eq!(Path::new(&a.argv[0]), root.join("test.agent").join("./chat"), "a relative program runs from the plugin folder");
    assert_eq!(&a.argv[1..], ["--quiet", "turn"]);
    assert_eq!(a.resume_args, ["--session", "{session}"]);
    assert!(a.env.iter().any(|(k, v)| k == "FAKE_SERVICE" && v == "b"), "{:?}", a.env);
    assert!(Host::new(None, Arc::new(MemoryStore::default())).agents().is_empty());
    let _ = std::fs::remove_dir_all(&data);
}

/// Set in the copies of this test binary that [`plugin_runs_get_the_data_dir`] starts: the
/// `BASHCUTPRO_DATA_DIR` the fake plugin must see.
const EXPECT_DATA_DIR: &str = "BASHCUT_TEST_EXPECT_DATA_DIR";

#[test]
fn plugin_runs_get_the_data_dir() {
    // FX9: the app's data folder (as this process resolves it) reaches an action run as BASHCUTPRO_DATA_DIR.
    // The parent copies this test binary into folders it lays out (portable or not) and runs the copy,
    // since the data folder depends on the executable's folder and the process env.
    let Some(py) = ["python3", "python"].into_iter().find(|p| has(p)) else {
        eprintln!("skipped: no python3 or python");
        return;
    };
    if let Ok(expect) = std::env::var(EXPECT_DATA_DIR) {
        let data = scratch("datadir-host");
        let dir = bashcut_plugin_host::plugins_dir(&data).join("test.fake");
        write_plugin(&dir, "test.fake");
        let mut m = manifest("test.fake");
        m["entry"] = json!({"command": [py, "plugin.py"]});
        std::fs::write(dir.join("bashcut-plugin.json"), m.to_string()).unwrap();
        let mut host = Host::new(Some(data.clone()), Arc::new(MemoryStore::default()));
        let ctx = RunContext::default();
        let job = host.command("run", &json!({"plugin": "test.fake", "action": "ok"}), &ctx).unwrap()["job"].as_u64().unwrap();
        assert_eq!(wait_job(&mut host, job), JobState::Succeeded, "{:?}", host.job(job));
        let seen = host.job(job).unwrap().outcome.as_ref().unwrap().result.as_ref().unwrap().data["dataDir"].clone();
        let _ = std::fs::remove_dir_all(&data);
        assert_eq!(seen, json!(expect));
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        eprintln!("skipped: no current_exe");
        return;
    };
    let root = scratch("datadir");
    let home = root.join("home");
    let run = |bin_dir: &Path, portable: bool, exported: Option<&Path>, expect: &Path| {
        std::fs::create_dir_all(bin_dir).unwrap();
        if portable {
            std::fs::write(bin_dir.join(bashcut_dirs::PORTABLE_MARKER), "").unwrap();
        }
        let copy = bin_dir.join(exe.file_name().unwrap());
        std::fs::copy(&exe, &copy).unwrap();
        let mut c = Command::new(&copy);
        c.args(["--exact", "plugin_runs_get_the_data_dir", "--nocapture", "--test-threads=1"]);
        c.env(EXPECT_DATA_DIR, expect).env_remove("FILMCRAFT_DATA_DIR").env_remove("BASHCUTPRO_DATA_DIR");
        c.env("HOME", &home).env("APPDATA", &home).env("XDG_DATA_HOME", &home);
        if let Some(v) = exported {
            c.env("BASHCUTPRO_DATA_DIR", v);
        }
        let out = c.output().unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "portable={portable} exported={exported:?}: {text}");
        assert!(text.contains("1 passed"), "the copy ran the check: {text}");
    };
    // portable: <exe dir>/data
    let portable = root.join("portable");
    run(&portable, true, None, &portable.join("data"));
    // normal: the per-user folder (the same rule bashcut-dirs applies)
    let normal = root.join("normal");
    let vars = |k: &str| matches!(k, "HOME" | "APPDATA" | "XDG_DATA_HOME").then(|| home.to_string_lossy().into_owned());
    let per_user = bashcut_dirs::Host { var: &vars, os: std::env::consts::OS, exe_dir: Some(normal.clone()) }.data_dir().unwrap();
    run(&normal, false, None, &per_user);
    // an exported value is kept, on a portable install too
    let exported = root.join("exported");
    run(&root.join("exported-portable"), true, Some(&exported), &exported);
    run(&root.join("exported-normal"), false, Some(&exported), &exported);
    let _ = std::fs::remove_dir_all(&root);
}

/// Set in the copies of this test binary that [`plugin_runs_find_the_engine_beside_the_app`] starts:
/// the `BASHCUTPRO_CLI` the fake plugin must see.
const EXPECT_CLI: &str = "BASHCUT_TEST_EXPECT_CLI";

#[test]
fn plugin_runs_find_the_engine_beside_the_app() {
    // NM4: a packaged app's engine (`Contents/MacOS/bashcutpro-cli`, `<install>ashcutpro-cli.exe`,
    // `/usr/bin/bashcutpro-cli`) sits next to the app, not always on PATH: every plugin child gets it as
    // BASHCUTPRO_CLI, also without a `--cli` in the run context (agent plugins, hooks). The parent copies
    // this test binary next to a fake engine and runs the copy, since the lookup starts at the executable's folder.
    let Some(py) = python() else {
        eprintln!("skipped: no python3 or python");
        return;
    };
    if let Ok(expect) = std::env::var(EXPECT_CLI) {
        let data = scratch("cli-host");
        let dir = bashcut_plugin_host::plugins_dir(&data).join("test.fake");
        write_plugin(&dir, "test.fake");
        let mut m = manifest("test.fake");
        m["entry"] = json!({"command": [py, "plugin.py"]});
        m["settings"].as_array_mut().unwrap().push(json!({"id": "engine", "title": "Engine", "type": "string", "optional": true, "env": ["BASHCUTPRO_CLI"]}));
        // FX11: an action that takes connection arguments gets the same engine as `--cli`
        m["connect"]["cli"] = json!(["--cli", "{cli}"]);
        m["actions"].as_array_mut().unwrap().push(json!({"id": "engine", "title": "Engine", "args": ["ok"], "params": []}));
        std::fs::write(dir.join("bashcut-plugin.json"), m.to_string()).unwrap();
        let mut host = Host::new(Some(data.clone()), Arc::new(MemoryStore::default()));
        let cli_of = |host: &mut Host, ctx: &RunContext| {
            let job = host.command("run", &json!({"plugin": "test.fake", "action": "engine"}), ctx).unwrap()["job"].as_u64().unwrap();
            assert_eq!(wait_job(host, job), JobState::Succeeded, "{:?}", host.job(job));
            let data = &host.job(job).unwrap().outcome.as_ref().unwrap().result.as_ref().unwrap().data;
            assert_eq!(data["args"], json!(["--cli", data["cli"].clone()]), "argv --cli is the env's engine (FX11)");
            data["cli"].clone()
        };
        let seen = cli_of(&mut host, &RunContext::default());
        // FX10: a run's own engine (RunContext.cli) comes after an exported one and a manifest setting
        let ctx = RunContext { cli: Some("/ctx/bashcutpro-cli".into()), ..RunContext::default() };
        let with_ctx = cli_of(&mut host, &ctx);
        host.command("settings", &json!({"plugin": "test.fake", "values": {"engine": "/setting/bashcutpro-cli"}}), &ctx).unwrap();
        let with_setting = cli_of(&mut host, &ctx);
        let parsed = bashcut_plugin_host::Manifest::parse(&m.to_string()).unwrap();
        let env = bashcut_plugin_host::settings::env_for(&parsed, &Default::default(), &MemoryStore::default());
        let _ = std::fs::remove_dir_all(&data);
        assert_eq!(seen, json!(expect), "action run");
        assert!(env.contains(&("BASHCUTPRO_CLI".to_string(), expect.clone())), "agent plugins and hooks (env_for): {env:?}");
        let exported = std::env::var("BASHCUTPRO_CLI").unwrap_or_else(|_| "/ctx/bashcutpro-cli".into());
        assert_eq!(with_ctx, json!(exported), "an export wins over the run's engine, which wins over the found one");
        assert_eq!(with_setting, json!("/setting/bashcutpro-cli"), "a manifest setting wins over both");
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        eprintln!("skipped: no current_exe");
        return;
    };
    let root = scratch("cli");
    let run = |name: &str, engine: &str, exported: Option<&str>, expect: &str| {
        let bin_dir = root.join(name);
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join(engine), "").unwrap();
        let copy = bin_dir.join(exe.file_name().unwrap());
        std::fs::copy(&exe, &copy).unwrap();
        let mut c = Command::new(&copy);
        c.args(["--exact", "plugin_runs_find_the_engine_beside_the_app", "--nocapture", "--test-threads=1"]);
        c.env(EXPECT_CLI, expect.replace("{dir}", &bin_dir.to_string_lossy()));
        for k in bashcut_dirs::engine::ENV {
            c.env_remove(k);
        }
        if let Some(v) = exported {
            c.env("BASHCUTPRO_CLI", v);
        }
        let out = c.output().unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "{name}: {text}");
        assert!(text.contains("1 passed"), "the copy ran the check: {text}");
    };
    let exe_name = |n: &str| format!("{n}{}", std::env::consts::EXE_SUFFIX);
    let sep = std::path::MAIN_SEPARATOR;
    // the engine next to the app
    run("packaged", &exe_name("bashcutpro-cli"), None, &format!("{{dir}}{sep}{}", exe_name("bashcutpro-cli")));
    // an older build's engine name
    run("older", &exe_name("filmcraft-cli"), None, &format!("{{dir}}{sep}{}", exe_name("filmcraft-cli")));
    // an exported value is kept
    run("exported", &exe_name("bashcutpro-cli"), Some("/my/bashcutpro-cli"), "/my/bashcutpro-cli");
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn install_into_a_deep_data_dir() {
    // a portable install in a deep folder: the clone's .git/objects/pack/*.keep goes past MAX_PATH (260)
    if !has("git") {
        eprintln!("skipped: needs git");
        return;
    }
    let mut data = scratch("deep");
    while data.as_os_str().len() < 190 {
        data = data.join("deeper-folder");
    }
    std::fs::create_dir_all(&data).unwrap();
    let repo = registry_repo("0.1.0");
    let mut host = Host::new(Some(data.clone()), Arc::new(MemoryStore::default()));
    let ctx = RunContext::default();
    host.command("registry", &json!({"location": format!("{}@main", repo.display())}), &ctx).unwrap();
    let job = host.jobs().last().unwrap().id;
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded, "{:?}", host.job(job));
    let job = host.command("install", &json!({"plugin": "test.fake"}), &ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded, "{:?}", host.job(job));
    assert!(bashcut_plugin_host::plugins_dir(&data).join("test.fake/bashcut-plugin.json").is_file());
    for d in [scratch_root(&data), repo] {
        let _ = std::fs::remove_dir_all(d);
    }
}

/// The scratch folder a deep path was made in.
#[cfg(windows)]
fn scratch_root(deep: &Path) -> PathBuf {
    deep.ancestors().find(|a| a.file_name().is_some_and(|n| n.to_string_lossy().starts_with("bashcut-host-deep-"))).unwrap_or(deep).to_path_buf()
}
