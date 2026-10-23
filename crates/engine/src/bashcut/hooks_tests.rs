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
    let manifest = json!({"schema": "bashcut-plugin/1", "id": "t.p", "version": "0.1.0", "entry": {"command": ["x"]},
        "actions": [{"id": "a", "args": ["a"]}], "hooks": [{"event": "import.after", "action": "a"}]});
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    let mut s = Session::default();
    s.export_presets.set_dir(&data);
    let v = s.execute("bashcut.hooks.list", json!({})).unwrap();
    assert_eq!(v["hooks"][0]["plugin"], "t.p", "{v}");
}

#[test]
fn plugins_dir_defaults_to_the_sessions_data_dir() {
    let data = tmp_dir("bashcut-hooks-default-dir");
    let mut s = Session::default();
    s.export_presets.set_dir(&data);
    let v = s.execute("bashcut.hooks.list", json!({})).unwrap();
    assert_eq!(v["dir"].as_str().map(std::path::PathBuf::from), Some(data.join("bashcut-plugins")), "{v}");
    assert_eq!(v["hooks"], json!([]), "no plugins folder yet: no hooks, no error");
    // an empty pluginsDir is the default too
    let v = s.execute("bashcut.hooks.list", json!({"pluginsDir": ""})).unwrap();
    assert_eq!(v["dir"].as_str().map(std::path::PathBuf::from), Some(data.join("bashcut-plugins")));
}

#[test]
fn discover_skips_an_invalid_manifest_and_keeps_the_connect_project_default() {
    let root = tmp_dir("bashcut-hooks-discover");
    // refused by the host's reader: no schema
    std::fs::create_dir_all(root.join("a-noschema")).unwrap();
    let noschema =
        json!({"id": "t.noschema", "entry": {"command": ["x"]}, "actions": [{"id": "a", "args": []}], "hooks": [{"event": "export.before", "action": "a"}]});
    std::fs::write(root.join("a-noschema").join("bashcut-plugin.json"), noschema.to_string()).unwrap();
    // no connect section: a project action gets `--project {project}`
    std::fs::create_dir_all(root.join("b-plain")).unwrap();
    let plain = json!({"schema": "bashcut-plugin/1", "id": "t.plain", "version": "0.1.0", "entry": {"command": ["tool", "--flag"]},
        "actions": [{"id": "a", "args": ["run"], "needs": "project"}], "hooks": [{"event": "export.before", "action": "a"}]});
    std::fs::write(root.join("b-plain").join("bashcut-plugin.json"), plain.to_string()).unwrap();
    // a folder without a manifest is no plugin
    std::fs::create_dir_all(root.join("c-empty")).unwrap();
    plugin(&root, "d-custom", "t.custom", json!({"command": ["x"]}), json!([{"event": "export.before", "action": "act"}]), "project");

    let hooks = discover(&root, "export.before");
    let ids: Vec<&str> = hooks.iter().map(|h| h.plugin.as_str()).collect();
    assert_eq!(ids, ["t.plain", "t.custom"], "the invalid manifest declares no hooks");
    assert_eq!(hooks[0].argv, ["tool", "--flag", "run"]);
    assert_eq!(hooks[0].project_args.as_deref(), Some(&["--project".to_string(), "{project}".to_string()][..]));
    assert_eq!(hooks[0].timeout.as_secs(), 300, "the default timeout");
    assert_eq!(hooks[1].project_args.as_deref(), Some(&["--project".to_string(), "{project}".to_string()][..]), "the manifest's own connect.project");
    // the project path fills {project}; without one the hook is skipped
    assert_eq!(run_hook(&hooks[0], None, &json!({}))["status"], "skipped");
}

#[test]
fn hook_gets_the_plugins_settings_env() {
    if !cfg!(unix) {
        return;
    }
    let root = tmp_dir("bashcut-hooks-settings");
    let dir = root.join("t.env");
    std::fs::create_dir_all(&dir).unwrap();
    let m = json!({"schema": "bashcut-plugin/1", "id": "t.env", "version": "0.1.0",
        "entry": {"command": ["sh", "-c", "printf '%s|%s|%s|%s' \"$S_SERVICE\" \"$S_N\" \"$BASHCUT_PLUGIN_HOST\" \"$BASHCUT_HOOK_EVENT\" > env.out"]},
        "settings": [
            {"id": "service", "type": "enum", "choices": ["a", "b"], "default": "a", "env": ["S_SERVICE"]},
            {"id": "n", "type": "integer", "env": ["S_N"]},
            {"id": "plain", "type": "string"}
        ],
        "actions": [{"id": "act", "args": []}], "hooks": [{"event": "import.after", "action": "act"}]});
    std::fs::write(dir.join("bashcut-plugin.json"), m.to_string()).unwrap();
    std::fs::write(dir.join("settings.json"), r#"{"service":"b","n":7,"plain":"x"}"#).unwrap();
    let hooks = discover(&root, "import.after");
    let row = run_hook(&hooks[0], None, &json!({}));
    assert_eq!(row["status"], "ok", "{row}");
    let out = std::fs::read_to_string(dir.join("env.out")).unwrap();
    assert_eq!(out, "b|7|1|import.after", "settings under their env names, like an action run");
}

#[test]
fn hook_timeout_is_soft_first_and_failed_rows_keep_stderr() {
    if !cfg!(unix) {
        return;
    }
    let root = tmp_dir("bashcut-hooks-soft");
    // TERM is trapped (a marker), so only the KILL after the grace ends it
    plugin(
        &root,
        "stubborn",
        "test.stubborn",
        shell("trap 'echo term > term.marker' TERM; echo starting >&2; while :; do sleep 0.1; done", "exit 1"),
        json!([{"event": "batch.afterItem", "action": "act", "timeoutSeconds": 1}]),
        "none",
    );
    let hooks = discover(&root, "batch.afterItem");
    let t0 = std::time::Instant::now();
    let row = run_hook(&hooks[0], None, &json!({}));
    let took = t0.elapsed();
    assert_eq!(row["status"], "failed", "{row}");
    assert!(row["message"].as_str().unwrap().contains("timed out after 1 s"), "{row}");
    assert!(root.join("stubborn").join("term.marker").is_file(), "TERM came first");
    assert!(took >= std::time::Duration::from_secs(1) + bashcut_dirs::child::GRACE / 2, "KILL waited for the grace: {took:?}");
    assert!(took.as_secs() < 10, "{took:?}");

    // a failed row carries the tail of stderr (at most 2 KiB)
    plugin(
        &root,
        "noisy",
        "test.noisy",
        shell("i=0; while [ $i -lt 400 ]; do echo \"line $i of the noise\" >&2; i=$((i+1)); done; echo last words >&2; exit 2", "exit 2"),
        json!([{"event": "batch.beforeItem", "action": "act"}]),
        "none",
    );
    let hooks = discover(&root, "batch.beforeItem");
    let row = run_hook(&hooks[0], None, &json!({}));
    assert_eq!((row["status"].as_str(), row["exit"].as_i64()), (Some("failed"), Some(2)), "{row}");
    let stderr = row["stderr"].as_str().unwrap();
    assert!(stderr.ends_with("last words"), "the tail is kept: {stderr}");
    assert!(stderr.len() <= 2048 && !stderr.contains("line 0 of"), "capped to the tail: {} bytes", stderr.len());
}
