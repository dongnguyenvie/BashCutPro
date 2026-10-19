//! `bashcut-view/1` forms end to end: a fake Python plugin prints views on stdout while it runs,
//! the host keeps the latest, and a button press runs the action it names with the form's values.
//! Skips (with a note) when `python3` is not on PATH.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bashcut_plugin_host::host::RunContext;
use bashcut_plugin_host::settings::MemoryStore;
use bashcut_plugin_host::view::Row;
use bashcut_plugin_host::{Host, JobState};
use serde_json::{Value, json};

/// `review` prints a first view, then a second with the list filled, then its result; `accept`
/// echoes its args.
const PLUGIN_PY: &str = r#"
import json, sys, time
mode = sys.argv[1]
def view(rows):
    print(json.dumps({"schema": "bashcut-view/1", "title": "Review", "rows": rows}), flush=True)
if mode == "review":
    view([{"type": "progress", "value": 0.5, "label": "translating"}])
    time.sleep(0.2)
    print("x" * 100)
    view([
        {"type": "input", "id": "lang", "label": "Language", "value": "vi"},
        {"type": "list", "id": "lines", "items": [{"id": "c1", "text": "Xin chào", "detail": "Hello", "buttons": [{"id": "ok", "label": "Accept", "action": "accept"}]}]},
        {"type": "button", "id": "all", "label": "Accept all", "action": "accept", "params": {"all": True}},
    ])
    print(json.dumps({"ok": True, "message": "1 line to review"}))
else:
    print(json.dumps({"ok": True, "message": "accepted", "data": {"args": sys.argv[2:]}}))
"#;

fn manifest() -> Value {
    json!({
        "schema": "bashcut-plugin/1", "id": "test.views", "name": "Views", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"]},
        "output": "bashcut-result/1",
        "actions": [
            {"id": "review", "title": "Review", "args": ["review"], "needs": "none", "view": true},
            {"id": "accept", "title": "Accept", "args": ["accept"], "needs": "none", "params": [
                {"id": "lang", "flag": "--lang", "type": "string", "optional": true},
                {"id": "item", "flag": "--item", "type": "string", "optional": true},
                {"id": "all", "flag": "--all", "type": "bool", "optional": true}
            ]}
        ]
    })
}

fn wait_job(host: &mut Host, job: u64) -> JobState {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(60) {
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

#[test]
fn a_view_action_shows_a_form_and_its_buttons_run_actions() {
    if !Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("skipped: no python3");
        return;
    }
    let data: PathBuf = std::env::temp_dir().join(format!("bashcut-host-views-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let dir = bashcut_plugin_host::plugins_dir(&data).join("test.views");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.py"), PLUGIN_PY).unwrap();
    std::fs::write(dir.join("bashcut-plugin.json"), manifest().to_string()).unwrap();
    let mut host = Host::new(Some(data.clone()), Arc::new(MemoryStore::default()));
    let ctx = RunContext::default();
    assert!(host.plugin("test.views").and_then(|p| p.manifest.as_ref()).and_then(|m| m.action("review")).is_some_and(|a| a.view));
    assert_eq!(host.command("view", &json!({"plugin": "test.views"}), &ctx).unwrap(), Value::Null);

    // the first view arrives while the run is still going
    let job = host.command("run", &json!({"plugin": "test.views", "action": "review"}), &ctx).unwrap()["job"].as_u64().unwrap();
    let start = Instant::now();
    while host.view("test.views").is_none() && start.elapsed() < Duration::from_secs(30) {
        host.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(host.view("test.views").map(|(_, v)| &v.rows[..]), Some([Row::Progress { .. }])), "{:?}", host.view("test.views"));
    assert_eq!(wait_job(&mut host, job), JobState::Succeeded);
    let shown = host.command("view", &json!({"plugin": "test.views"}), &ctx).unwrap();
    assert_eq!(shown["job"], json!(job));
    assert_eq!(shown["view"]["rows"].as_array().unwrap().len(), 3, "the newest view replaced the first");
    assert_eq!(host.job(job).unwrap().message, "1 line to review", "view lines are not the result");

    // a list row's button: the item id plus the form's values the action takes
    let values = json!({"lang": "en", "unrelated": 1});
    let click = json!({"plugin": "test.views", "button": "lines", "item": "c1", "itemButton": "ok", "values": values});
    let accept = host.command("click", &click, &ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, accept), JobState::Succeeded);
    let args = &host.job(accept).unwrap().outcome.as_ref().unwrap().result.as_ref().unwrap().data["args"];
    assert_eq!(args, &json!(["--lang", "en", "--item", "c1"]));
    // a plain button with its own params
    let all = host.command("click", &json!({"plugin": "test.views", "button": "all"}), &ctx).unwrap()["job"].as_u64().unwrap();
    assert_eq!(wait_job(&mut host, all), JobState::Succeeded);
    assert_eq!(host.job(all).unwrap().outcome.as_ref().unwrap().result.as_ref().unwrap().data["args"], json!(["--all"]));
    // the form stays the review's: runs that print no view do not replace it
    assert_eq!(host.view("test.views").map(|(j, _)| j), Some(job));

    assert!(host.command("click", &json!({"plugin": "test.views", "button": "nope"}), &ctx).is_err());
    assert!(host.command("click", &json!({"plugin": "test.views", "button": "lines", "item": "c1"}), &ctx).is_err());
    assert!(host.command("click", &json!({"plugin": "test.other", "button": "all"}), &ctx).is_err());
    let _ = std::fs::remove_dir_all(&data);
}
