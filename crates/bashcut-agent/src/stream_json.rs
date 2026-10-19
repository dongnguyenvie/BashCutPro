//! Claude Code's stream-json (`claude -p --output-format stream-json --input-format stream-json
//! --verbose`): one JSON object per line. We read `system/init`, `assistant` (text and tool_use
//! blocks), `user` (tool_result blocks) and `result`; anything else is ignored. The same format is
//! the default `events` of agent plugins.

use serde_json::{Value, json};

use crate::clip;

/// The event format name in agent specs and plugin manifests.
pub const FORMAT: &str = "claude-stream-json";
/// Longest text kept from one block.
pub const MAX_TEXT: usize = 32 * 1024;
/// Longest tool input kept (as JSON text).
const MAX_INPUT: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The agent's session started (`--resume` takes this id next turn).
    Init {
        session: String,
        model: String,
    },
    /// Assistant text.
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        id: String,
        text: String,
        is_error: bool,
    },
    /// The turn's end, as the agent reports it.
    Result {
        ok: bool,
        text: String,
        session: String,
        cost_usd: Option<f64>,
        turns: Option<u64>,
    },
    /// Something we show but do not model (an unparsable line, an agent error message).
    Notice(String),
}

/// The events of one stdout line (an assistant message can hold several blocks).
pub fn parse_line(line: &str) -> Vec<Event> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![Event::Notice(clip(line, 512))];
    };
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match v.get("type").and_then(Value::as_str).unwrap_or_default() {
        "system" if v.get("subtype").and_then(Value::as_str) == Some("init") => {
            vec![Event::Init { session: text(&v, "session_id"), model: text(&v, "model") }]
        }
        "assistant" => blocks(&v)
            .iter()
            .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                Some("text") => Some(Event::Text(clip(&text(b, "text"), MAX_TEXT))).filter(|e| !matches!(e, Event::Text(t) if t.is_empty())),
                Some("tool_use") => Some(Event::ToolUse { id: text(b, "id"), name: clip(&text(b, "name"), 256), input: capped_input(b.get("input")) }),
                _ => None,
            })
            .collect(),
        "user" => blocks(&v)
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            .map(|b| Event::ToolResult {
                id: text(b, "tool_use_id"),
                text: clip(&content_text(b.get("content")), MAX_TEXT),
                is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            })
            .collect(),
        "result" => {
            let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let ok = !is_error && v.get("subtype").and_then(Value::as_str).is_none_or(|s| s == "success");
            let mut t = text(&v, "result");
            if t.is_empty() && !ok {
                t = text(&v, "subtype");
            }
            vec![Event::Result {
                ok,
                text: clip(&t, MAX_TEXT),
                session: text(&v, "session_id"),
                cost_usd: v.get("total_cost_usd").and_then(Value::as_f64).filter(|c| c.is_finite()),
                turns: v.get("num_turns").and_then(Value::as_u64),
            }]
        }
        "error" => vec![Event::Notice(clip(&v.get("error").map(|e| content_text(Some(e))).unwrap_or_else(|| text(&v, "message")), 2048))],
        _ => Vec::new(),
    }
}

fn blocks(v: &Value) -> Vec<Value> {
    match v.get("message").and_then(|m| m.get("content")) {
        Some(Value::Array(a)) => a.iter().take(256).cloned().collect(),
        Some(Value::String(s)) => vec![json!({"type": "text", "text": s})],
        _ => Vec::new(),
    }
}

/// A tool result's content: a string, or text blocks.
pub(crate) fn content_text(c: Option<&Value>) -> String {
    match c {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        Some(Value::Object(o)) => o.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| Value::Object(o.clone()).to_string()),
        Some(other) if !other.is_null() => other.to_string(),
        _ => String::new(),
    }
}

fn capped_input(v: Option<&Value>) -> Value {
    let v = v.cloned().unwrap_or(Value::Null);
    if v.to_string().len() > MAX_INPUT { json!({"truncated": true}) } else { v }
}

/// One user message as a stream-json input line (no trailing newline).
pub fn user_message(text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_turn() {
        let init = parse_line(r#"{"type":"system","subtype":"init","session_id":"s1","model":"m","tools":[]}"#);
        assert_eq!(init, vec![Event::Init { session: "s1".into(), model: "m".into() }]);
        let a = parse_line(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Cutting."},{"type":"tool_use","id":"t1","name":"mcp__filmcraft__command_execute","input":{"command":"edit.razor"}}]}}"#,
        );
        assert_eq!(a.len(), 2);
        assert!(matches!(&a[1], Event::ToolUse { name, input, .. } if name.ends_with("command_execute") && input["command"] == "edit.razor"));
        let u = parse_line(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"ok"}],"is_error":true}]}}"#,
        );
        assert_eq!(u, vec![Event::ToolResult { id: "t1".into(), text: "ok".into(), is_error: true }]);
        let r = parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"Done.","session_id":"s1","total_cost_usd":0.01,"num_turns":3}"#);
        assert!(matches!(&r[0], Event::Result { ok: true, turns: Some(3), .. }));
        let r = parse_line(r#"{"type":"result","subtype":"error_max_turns","is_error":true,"session_id":"s1"}"#);
        assert!(matches!(&r[0], Event::Result { ok: false, text, .. } if text == "error_max_turns"));
    }

    #[test]
    fn hostile_lines_do_not_break_it() {
        assert_eq!(parse_line(""), vec![]);
        assert!(matches!(&parse_line("not json")[0], Event::Notice(_)));
        assert_eq!(parse_line(r#"{"type":"assistant","message":{"content":7}}"#), vec![]);
        assert_eq!(parse_line(r#"[1,2]"#), vec![]);
        let big = format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{}"}}]}}}}"#, "x".repeat(MAX_TEXT * 2));
        assert!(matches!(&parse_line(&big)[0], Event::Text(t) if t.len() <= MAX_TEXT + 4));
        let input = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"a","name":"n","input":{{"x":"{}"}}}}]}}}}"#,
            "y".repeat(MAX_INPUT)
        );
        assert!(matches!(&parse_line(&input)[0], Event::ToolUse { input, .. } if input["truncated"] == true));
    }

    #[test]
    fn user_message_round_trips() {
        let v: Value = serde_json::from_str(&user_message("xin chào \"quoted\"\nnext")).unwrap();
        assert_eq!(v["message"]["content"][0]["text"], "xin chào \"quoted\"\nnext");
    }
}
