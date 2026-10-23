//! BashCutPro: plugin hooks run by our code paths (#73).

use std::path::Path;

use serde_json::{Value, json};

use super::hooks::{discover, run_hook};
use crate::Session;
use crate::media_test_util::tmp_dir;

/// A shell command line per OS: `sh -c` on Unix, `cmd /C` on Windows.
fn shell(unix: &str, windows: &str) -> Value {
    json!({"command": ["sh", "-c", unix], "windows": ["cmd", "/C", windows]})
}

fn plugin(root: &Path, folder: &str, id: &str, entry: Value, hooks: Value, needs: &str) {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let m = json!({
        "schema": "bashcut-plugin/1",
        "id": id,
        "version": "0.1.0",
        "entry": entry,
        "connect": {"project": ["--project", "{project}"]},
        "actions": [{"id": "act", "args": [], "needs": needs}],
        "hooks": hooks,
    });
    std::fs::write(dir.join("bashcut-plugin.json"), serde_json::to_vec(&m).unwrap()).unwrap();
}

fn statuses(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|r| r["status"].as_str().unwrap().to_string()).collect()
}

#[test]
fn hooks_run_for_their_event_and_report() {
    let root = tmp_dir("bashcut-hooks");
    plugin(
        &root,
        "a-ok",
        "test.ok",
        shell(r#"echo '{"ok":true,"message":"hi"}'"#, "exit 0"),
        json!([{"event": "export.before", "action": "act"}, {"event": "import.after", "action": "act"}]),
        "none",
    );
    plugin(&root, "b-handoff", "test.handoff", shell("exit 3", "exit 3"), json!([{"event": "export.before", "action": "act"}]), "none");
    plugin(&root, "c-fail", "test.fail", shell("exit 1", "exit 1"), json!([{"event": "export.before", "action": "act"}]), "none");
    plugin(&root, "d-project", "test.project", shell("exit 0", "exit 0"), json!([{"event": "export.before", "action": "act"}]), "project");
    plugin(&root, "e-unknown", "test.unknown", shell("exit 0", "exit 0"), json!([{"event": "export.before", "action": "nope"}]), "none");

    assert_eq!(discover(&root, "export.before").len(), 4, "an unknown action is skipped");
    assert_eq!(discover(&root, "import.after").len(), 1);
    assert!(discover(&root, "batch.afterItem").is_empty());

    let mut s = Session::default();
    let d = root.to_string_lossy().into_owned();
    let r = s.execute("bashcut.hooks.run", json!({"event": "export.before", "context": {"path": "x.mp4"}, "pluginsDir": d})).unwrap();
    // the project hook is skipped: the session has no saved project
    assert_eq!(statuses(&r["hooks"]), ["ok", "handoff", "failed", "skipped"], "{r}");
    if cfg!(unix) {
        assert_eq!(r["hooks"][0]["message"], "hi");
    }
    let listed = s.execute("bashcut.hooks.list", json!({"pluginsDir": d})).unwrap();
    assert_eq!(listed["hooks"].as_array().unwrap().len(), 5, "{listed}");

    // hooks off: nothing runs
    let r = s.execute("bashcut.hooks.run", json!({"event": "export.before", "hooks": false, "pluginsDir": d})).unwrap();
    assert!(r["hooks"].as_array().unwrap().is_empty());
}

#[test]
fn a_failed_required_hook_stops_the_command() {
    let root = tmp_dir("bashcut-hooks-required");
    plugin(&root, "fail", "test.fail", shell("exit 1", "exit 1"), json!([{"event": "export.before", "action": "act", "required": true}]), "none");
    let mut s = Session::default();
    let err = s.execute("bashcut.hooks.run", json!({"event": "export.before", "pluginsDir": root.to_string_lossy()})).unwrap_err();
    assert!(err.to_string().contains("required"), "{err}");
}

#[test]
fn a_slow_hook_times_out() {
    let root = tmp_dir("bashcut-hooks-timeout");
    plugin(
        &root,
        "slow",
        "test.slow",
        shell("sleep 10", "ping -n 11 127.0.0.1 > NUL"),
        json!([{"event": "batch.beforeItem", "action": "act", "timeoutSeconds": 1}]),
        "none",
    );
    let hooks = discover(&root, "batch.beforeItem");
    let t0 = std::time::Instant::now();
    let row = run_hook(&hooks[0], None, &json!({}));
    assert_eq!(row["status"], "failed", "{row}");
    assert!(t0.elapsed().as_secs() < 8, "killed at the timeout");
}

#[test]
fn hook_commands_refuse_hostile_input() {
    let root = tmp_dir("bashcut-hooks-hostile");
    std::fs::create_dir_all(root.join("broken")).unwrap();
    std::fs::write(root.join("broken").join("bashcut-plugin.json"), b"{").unwrap();
    plugin(&root, "noentry", "test.noentry", json!({"command": []}), json!([{"event": "export.before", "action": "act"}]), "none");
    plugin(&root, "badhooks", "test.badhooks", shell("exit 0", "exit 0"), json!("nope"), "none");
    plugin(&root, "missing", "test.missing", json!({"command": ["bashcut-no-such-program-xyz"]}), json!([{"event": "export.before", "action": "act"}]), "none");
    let mut s = Session::default();
    let d = root.to_string_lossy().into_owned();
    for p in [json!({}), json!({"event": "nope"}), json!({"event": 7})] {
        assert!(s.execute("bashcut.hooks.run", p.clone()).is_err(), "{p}");
    }
    assert!(s.execute("bashcut.hooks.list", json!({"event": "nope", "pluginsDir": d})).is_err());
    let r = s.execute("bashcut.hooks.run", json!({"event": "export.before", "pluginsDir": d})).unwrap();
    assert_eq!(statuses(&r["hooks"]), ["failed"], "a program that does not start fails, the rest are skipped: {r}");
    assert!(s.execute("bashcut.hooks.list", json!({"pluginsDir": root.join("none").to_string_lossy()})).is_ok());
}

#[test]
fn plugins_come_from_the_sessions_data_dir() {
    let data = tmp_dir("bashcut-hooks-data-dir");
    let plugin = data.join("bashcut-plugins").join("p");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = json!({"schema": "bashcut-plugin/1", "id": "t.p", "entry": {"command": ["x"]},
        "actions": [{"id": "a", "args": ["a"]}], "hooks": [{"event": "import.after", "action": "a"}]});
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    let mut s = Session::default();
    s.export_presets.set_dir(&data);
    let v = s.execute("bashcut.hooks.list", json!({})).unwrap();
    assert_eq!(v["hooks"][0]["plugin"], "t.p", "{v}");
}

#[test]
#[ignore = "test ticket: a hook gets its plugin's settings env (settings.json value under the setting's env name, BASHCUT_PLUGIN_HOST=1) like an action run"]
fn hook_gets_the_plugins_settings_env() {}

#[test]
#[ignore = "test ticket: a timed-out hook gets TERM first (unix trap writes a marker), then KILL after the grace; a failed row carries stderr's tail"]
fn hook_timeout_is_soft_first_and_failed_rows_keep_stderr() {}
