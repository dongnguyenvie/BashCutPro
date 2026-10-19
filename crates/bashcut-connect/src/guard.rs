//! The MCP write guard. `bashcut-mcp` sits between the agent and `filmcraft-cli mcp`, and every
//! JSON-RPC line from the agent passes [`check`] first. A `tools/call` whose command writes (its
//! id says save, export, render, open, move, delete… and it names a target path) to a place outside
//! the allowed [`Roots`] gets a tool error back instead of reaching the engine. Reads (importing
//! media from anywhere, inspecting) pass. This is a guard against mistakes, not a sandbox: the
//! agent's own file tools are limited by the agent's settings.

use crate::roots::Roots;
use serde_json::{Value, json};
use std::path::Path;

/// Words in a command id that mean it can write (or open something to write later).
const WRITE_WORDS: &[&str] = &[
    "save",
    "export",
    "write",
    "render",
    "screenshot",
    "open",
    "new",
    "delete",
    "remove",
    "trash",
    "move",
    "rename",
    "install",
    "consolidate",
    "transcode",
    "proxy",
    "collect",
    "copy",
    "extract",
    "record",
    "download",
    "generate",
    "bounce",
    "template",
];
/// Parameter names that hold a target path.
const PATH_KEYS: &[&str] = &["path", "out", "output", "outpath", "dest", "destination", "target", "file", "dir", "folder", "directory"];
const MAX_DEPTH: usize = 16;
static NULL: Value = Value::Null;

/// `None`: forward the line. `Some(reply)`: send this JSON-RPC reply to the agent instead.
pub fn check(line: &str, roots: &Roots, cwd: &Path) -> Option<String> {
    if roots.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    match &v {
        Value::Array(batch) => {
            let refused: Vec<String> = batch.iter().filter_map(|m| violation(m, roots, cwd)).collect();
            if refused.is_empty() {
                return None;
            }
            let why = refused.join("; ");
            let replies: Vec<Value> = batch.iter().filter_map(|m| m.get("id").cloned()).map(|id| reply(id, &why)).collect();
            Some(Value::Array(replies).to_string())
        }
        _ => violation(&v, roots, cwd).map(|why| reply(v.get("id").cloned().unwrap_or(Value::Null), &why).to_string()),
    }
}

fn reply(id: Value, why: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": why}], "isError": true}})
}

/// Why this message must not reach the engine, if it must not.
fn violation(msg: &Value, roots: &Roots, cwd: &Path) -> Option<String> {
    if msg.get("method").and_then(Value::as_str) != Some("tools/call") {
        return None;
    }
    let params = msg.get("params")?;
    let tool = params.get("name").and_then(Value::as_str).unwrap_or_default();
    let args = params.get("arguments").unwrap_or(&NULL);
    for (id, p) in commands(tool, args) {
        if !writes(&id) {
            continue;
        }
        let mut paths = Vec::new();
        collect_paths(p, 0, &mut paths);
        if let Some(bad) = paths.iter().find(|p| !roots.allows(p, cwd)) {
            let allowed = roots.list().iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ");
            return Some(format!(
                "refused by bashcut-mcp: {id} would write {bad}, outside the folders agents may write to ({allowed}). Write inside the project folder, or add the folder to BASHCUTPRO_ROOTS."
            ));
        }
    }
    None
}

/// The (command id, params) pairs a tool call runs.
fn commands<'a>(tool: &str, args: &'a Value) -> Vec<(String, &'a Value)> {
    let id_of = |v: &'a Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let empty: &'static Value = &NULL;
    match tool {
        "command_run" => vec![(id_of(args, "id"), args.get("params").unwrap_or(empty))],
        "command_batch" => args
            .get("steps")
            .and_then(Value::as_array)
            .map(|steps| steps.iter().map(|s| (id_of(s, "id"), s.get("params").unwrap_or(empty))).collect())
            .unwrap_or_default(),
        "ui_control" => {
            let method = id_of(args, "method");
            let p = args.get("params").unwrap_or(empty);
            if method == "engine.execute" { vec![(id_of(p, "command"), p.get("params").unwrap_or(empty))] } else { vec![(method, p)] }
        }
        "ui_screenshot" => vec![("ui.screenshot".to_string(), args)],
        other => vec![(other.to_string(), args)],
    }
}

fn writes(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    WRITE_WORDS.iter().any(|w| id.contains(w))
}

fn collect_paths(v: &Value, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH {
        return;
    }
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if PATH_KEYS.contains(&k.to_ascii_lowercase().as_str()) {
                    match x {
                        Value::String(s) => out.push(s.clone()),
                        Value::Array(a) => out.extend(a.iter().filter_map(Value::as_str).map(str::to_string)),
                        other => collect_paths(other, depth + 1, out),
                    }
                } else {
                    collect_paths(x, depth + 1, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_paths(x, depth + 1, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn roots() -> (Roots, PathBuf) {
        let cwd = std::env::temp_dir().join(format!("bashcut-connect-guard-{}", std::process::id()));
        let _ = std::fs::create_dir_all(cwd.join("proj"));
        (Roots::new(Some(&cwd.join("proj/a.fcproj")), None, &[], &cwd), cwd)
    }

    fn call(name: &str, arguments: Value) -> String {
        json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name, "arguments": arguments}}).to_string()
    }

    #[test]
    fn writes_outside_the_roots_are_refused() {
        let (r, cwd) = roots();
        let inside = cwd.join("proj/exports/a.mp4").to_string_lossy().into_owned();
        let outside = cwd.join("other/a.mp4").to_string_lossy().into_owned();
        assert!(check(&call("command_run", json!({"id": "file.exportMedia", "params": {"path": inside}})), &r, &cwd).is_none());
        let refused = check(&call("command_run", json!({"id": "file.exportMedia", "params": {"path": outside}})), &r, &cwd).unwrap();
        let v: Value = serde_json::from_str(&refused).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["result"]["isError"], true);
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("file.exportMedia would write"));
        // the same through a batch, the control channel and a relative path
        assert!(
            check(&call("command_batch", json!({"steps": [{"id": "timeline.razor"}, {"id": "file.saveAs", "params": {"path": outside}}]})), &r, &cwd).is_some()
        );
        assert!(
            check(&call("ui_control", json!({"method": "engine.execute", "params": {"command": "file.saveAs", "params": {"path": outside}}})), &r, &cwd)
                .is_some()
        );
        assert!(check(&call("ui_screenshot", json!({"path": outside})), &r, &cwd).is_some());
        assert!(check(&call("command_run", json!({"id": "file.saveAs", "params": {"path": "../x.fcproj"}})), &r, &cwd.join("proj")).is_some());
        // opening another project to edit it is refused too
        assert!(check(&call("command_run", json!({"id": "file.open", "params": {"path": outside}})), &r, &cwd).is_some());
    }

    #[test]
    fn reads_and_other_messages_pass() {
        let (r, cwd) = roots();
        let outside = cwd.join("other/clip.mp4").to_string_lossy().into_owned();
        assert!(check(&call("command_run", json!({"id": "file.import", "params": {"paths": [outside]}})), &r, &cwd).is_none(), "import reads");
        assert!(check(&call("media_import", json!({"paths": [outside]})), &r, &cwd).is_none());
        assert!(check(&call("command_run", json!({"id": "file.save"})), &r, &cwd).is_none(), "save in place");
        assert!(check(&call("doc_inspect", json!({})), &r, &cwd).is_none());
        assert!(check(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#, &r, &cwd).is_none());
        assert!(check("not json", &r, &cwd).is_none());
        assert!(check(&call("command_run", json!({"id": "file.exportMedia", "params": {"path": outside}})), &Roots::default(), &cwd).is_none(), "no roots");
    }

    #[test]
    fn a_batch_array_with_a_bad_call_is_refused_whole() {
        let (r, cwd) = roots();
        let outside = cwd.join("other/a.mp4").to_string_lossy().into_owned();
        let good: Value = serde_json::from_str(&call("doc_inspect", json!({}))).unwrap();
        let mut bad: Value = serde_json::from_str(&call("command_run", json!({"id": "file.saveAs", "params": {"path": outside}}))).unwrap();
        bad["id"] = json!(8);
        let replies: Value = serde_json::from_str(&check(&json!([good, bad]).to_string(), &r, &cwd).unwrap()).unwrap();
        assert_eq!(replies.as_array().map(Vec::len), Some(2));
    }
}
