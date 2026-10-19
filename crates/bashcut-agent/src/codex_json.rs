//! Codex's JSON events (`codex exec --json`): one JSON object per line, mapped onto the same [`Event`]s as Claude
//! Code's stream-json so the transcript, the Agent section and conversations need nothing new.
//!
//! Read: `thread.started` (the session to resume), `item.completed` with an `agent_message` (assistant text),
//! `item.started` / `item.completed` with an `mcp_tool_call` or a `command_execution` (tool use and result),
//! `turn.completed` / `turn.failed` (the turn's end) and `error`. Reasoning items and anything unknown are ignored.

use serde_json::{Value, json};

use crate::clip;
use crate::stream_json::{Event, MAX_TEXT};

/// The event format name in agent specs and plugin manifests.
pub const FORMAT: &str = "codex-exec-json";
/// Longest tool input kept (as JSON text).
const MAX_INPUT: usize = 8 * 1024;

pub fn parse_line(line: &str) -> Vec<Event> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![Event::Notice(clip(line, 512))];
    };
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match v.get("type").and_then(Value::as_str).unwrap_or_default() {
        "thread.started" => vec![Event::Init { session: s(&v, "thread_id"), model: s(&v, "model") }],
        "item.started" => item(v.get("item"), false),
        "item.completed" => item(v.get("item"), true),
        "turn.completed" => vec![Event::Result { ok: true, text: String::new(), session: String::new(), cost_usd: None, turns: None }],
        "turn.failed" => {
            let msg = v.get("error").map(|e| s(e, "message")).filter(|m| !m.is_empty()).unwrap_or_else(|| "the turn failed".into());
            vec![Event::Result { ok: false, text: clip(&msg, MAX_TEXT), session: String::new(), cost_usd: None, turns: None }]
        }
        "error" => vec![Event::Notice(clip(&s(&v, "message"), 2048))],
        _ => Vec::new(),
    }
}

fn item(it: Option<&Value>, done: bool) -> Vec<Event> {
    let Some(it) = it else { return Vec::new() };
    let s = |k: &str| it.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let id = s("id");
    match (s("type").as_str(), done) {
        ("agent_message", true) => {
            let text = clip(&s("text"), MAX_TEXT);
            if text.is_empty() { Vec::new() } else { vec![Event::Text(text)] }
        }
        ("mcp_tool_call", false) => {
            let name = clip(&format!("mcp__{}__{}", s("server"), s("tool")), 256);
            vec![Event::ToolUse { id, name, input: capped(it.get("arguments")) }]
        }
        ("mcp_tool_call", true) => {
            let failed = s("status") == "failed" || it.get("error").is_some_and(|e| !e.is_null());
            let text = match it.get("error").filter(|e| !e.is_null()) {
                Some(e) => e.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| e.to_string()),
                None => result_text(it.get("result")),
            };
            vec![Event::ToolResult { id, text: clip(&text, MAX_TEXT), is_error: failed }]
        }
        ("command_execution", false) => vec![Event::ToolUse { id, name: "shell".into(), input: json!({"command": clip(&s("command"), 4096)}) }],
        ("command_execution", true) => {
            let code = it.get("exit_code").and_then(Value::as_i64);
            let failed = s("status") == "failed" || code.is_some_and(|c| c != 0);
            vec![Event::ToolResult { id, text: clip(&s("aggregated_output"), MAX_TEXT), is_error: failed }]
        }
        ("error", true) => vec![Event::Notice(clip(&s("message"), 2048))],
        _ => Vec::new(),
    }
}

/// An MCP result's text: its content blocks' text, else the JSON.
fn result_text(r: Option<&Value>) -> String {
    let Some(r) = r.filter(|r| !r.is_null()) else { return String::new() };
    match r.get("content") {
        Some(Value::Array(a)) => a.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => r.to_string(),
    }
}

fn capped(v: Option<&Value>) -> Value {
    let v = v.cloned().unwrap_or(Value::Null);
    // Codex may send the arguments as a JSON string
    let v = match v {
        Value::String(text) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        other => other,
    };
    if v.to_string().len() > MAX_INPUT { json!({"truncated": true}) } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_turn() {
        assert_eq!(parse_line(r#"{"type":"thread.started","thread_id":"th-1"}"#), vec![Event::Init { session: "th-1".into(), model: String::new() }]);
        let u = parse_line(
            r#"{"type":"item.started","item":{"id":"i1","type":"mcp_tool_call","server":"filmcraft","tool":"command_run","arguments":"{\"id\":\"edit.razor\"}"}}"#,
        );
        assert!(matches!(&u[0], Event::ToolUse { name, input, .. } if name == "mcp__filmcraft__command_run" && input["id"] == "edit.razor"));
        let r = parse_line(
            r#"{"type":"item.completed","item":{"id":"i1","type":"mcp_tool_call","server":"filmcraft","tool":"command_run","status":"completed","result":{"content":[{"type":"text","text":"ok"}]}}}"#,
        );
        assert_eq!(r, vec![Event::ToolResult { id: "i1".into(), text: "ok".into(), is_error: false }]);
        let c = parse_line(
            r#"{"type":"item.completed","item":{"id":"i2","type":"command_execution","command":"ls","aggregated_output":"nope","exit_code":2,"status":"failed"}}"#,
        );
        assert!(matches!(&c[0], Event::ToolResult { is_error: true, .. }));
        assert_eq!(parse_line(r#"{"type":"item.completed","item":{"id":"i3","type":"agent_message","text":"Done."}}"#), vec![Event::Text("Done.".into())]);
        assert!(matches!(&parse_line(r#"{"type":"turn.completed","usage":{"input_tokens":1}}"#)[0], Event::Result { ok: true, .. }));
        assert!(matches!(&parse_line(r#"{"type":"turn.failed","error":{"message":"quota"}}"#)[0], Event::Result { ok: false, text, .. } if text == "quota"));
    }

    #[test]
    fn hostile_lines_do_not_break_it() {
        assert_eq!(parse_line(""), vec![]);
        assert!(matches!(&parse_line("garbage")[0], Event::Notice(_)));
        assert_eq!(parse_line(r#"{"type":"item.completed"}"#), vec![]);
        assert_eq!(parse_line(r#"{"type":"item.completed","item":{"type":"reasoning","text":"…"}}"#), vec![]);
        assert_eq!(parse_line("[1]"), vec![]);
        let big = format!(r#"{{"type":"item.started","item":{{"id":"a","type":"mcp_tool_call","arguments":{{"x":"{}"}}}}}}"#, "y".repeat(MAX_INPUT));
        assert!(matches!(&parse_line(&big)[0], Event::ToolUse { input, .. } if input["truncated"] == true));
    }
}
