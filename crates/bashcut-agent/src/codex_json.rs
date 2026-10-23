//! Codex's JSON events (`codex exec --json`): one JSON object per line. We read `thread.started`,
//! `item.started` / `item.completed` (agent messages, MCP tool calls, shell commands, file
//! changes, errors), `turn.completed`, `turn.failed` and `error`; anything else (reasoning, todo
//! lists, usage) is ignored. The prompt goes to stdin as plain text (`-` as the prompt argument).

use std::collections::HashSet;

use serde_json::{Value, json};

use crate::clip;
use crate::stream_json::{Event, MAX_TEXT, capped_input, content_text};

/// The event format name in agent specs and plugin manifests.
pub const FORMAT: &str = bashcut_manifest::CODEX_JSON;
/// Tool calls remembered as started (a hostile stream cannot grow this without bound).
const MAX_STARTED: usize = 4096;

/// Codex's events are about items, not messages: the parser remembers the thread (the session id
/// for `resume`), the last answer (the turn's result text) and which tool calls it already showed.
#[derive(Debug, Default)]
pub struct Parser {
    thread: String,
    last_text: String,
    started: HashSet<String>,
}

impl crate::format::Decode for Parser {
    fn parse_line(&mut self, line: &str) -> Vec<Event> {
        Parser::parse_line(self, line)
    }
}

impl Parser {
    /// The events of one stdout line.
    pub fn parse_line(&mut self, line: &str) -> Vec<Event> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return vec![Event::Notice(clip(line, 512))];
        };
        let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "thread.started" => {
                self.thread = clip(&text(&v, "thread_id"), 128);
                vec![Event::Init { session: self.thread.clone(), model: String::new() }]
            }
            "item.started" => self.item(v.get("item").unwrap_or(&Value::Null), false),
            "item.completed" => self.item(v.get("item").unwrap_or(&Value::Null), true),
            "turn.completed" => vec![Event::Result { ok: true, text: self.last_text.clone(), session: self.thread.clone(), cost_usd: None, turns: None }],
            "turn.failed" => {
                let message = v.get("error").map(|e| text(e, "message")).filter(|m| !m.is_empty()).unwrap_or_else(|| "the turn failed".into());
                vec![Event::Result { ok: false, text: clip(&message, MAX_TEXT), session: self.thread.clone(), cost_usd: None, turns: None }]
            }
            "error" => vec![Event::Notice(clip(&text(&v, "message"), 2048))],
            _ => Vec::new(),
        }
    }

    fn item(&mut self, item: &Value, done: bool) -> Vec<Event> {
        let text = |k: &str| item.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let id = clip(&text("id"), 128);
        let (name, input) = match item.get("type").and_then(Value::as_str).unwrap_or_default() {
            "agent_message" if done => {
                let t = clip(&text("text"), MAX_TEXT);
                if t.is_empty() {
                    return Vec::new();
                }
                self.last_text = t.clone();
                return vec![Event::Text(t)];
            }
            "error" if done => return vec![Event::Notice(clip(&text("message"), 2048))],
            // named like Claude Code's MCP tools, so the panel groups them the same way
            "mcp_tool_call" => (format!("mcp__{}__{}", clip(&text("server"), 64), clip(&text("tool"), 128)), capped_input(item.get("arguments"))),
            "command_execution" => ("shell".to_string(), json!({"cmd": clip(&text("command"), 2048)})),
            "file_change" => ("file_change".to_string(), capped_input(item.get("changes"))),
            "web_search" => ("web_search".to_string(), json!({"query": clip(&text("query"), 512)})),
            _ => return Vec::new(),
        };
        let mut out = Vec::new();
        // a call seen only when completed still shows as a call
        if !self.started.contains(&id) {
            if self.started.len() >= MAX_STARTED {
                self.started.clear();
            }
            self.started.insert(id.clone());
            out.push(Event::ToolUse { id: id.clone(), name, input });
        }
        if done {
            let status = text("status");
            let (output, failed) = match item.get("type").and_then(Value::as_str) {
                Some("mcp_tool_call") => match item.get("error").filter(|e| !e.is_null()) {
                    Some(e) => (e.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| e.to_string()), true),
                    None => (content_text(item.get("result").and_then(|r| r.get("content"))), status == "failed"),
                },
                Some("command_execution") => {
                    (text("aggregated_output"), status == "failed" || item.get("exit_code").and_then(Value::as_i64).is_some_and(|c| c != 0))
                }
                _ => (status.clone(), status == "failed"),
            };
            self.started.remove(&id);
            out.push(Event::ToolResult { id, text: clip(&output, MAX_TEXT), is_error: failed });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_turn() {
        let mut p = Parser::default();
        assert_eq!(p.parse_line(r#"{"type":"thread.started","thread_id":"th-1"}"#), vec![Event::Init { session: "th-1".into(), model: String::new() }]);
        assert_eq!(p.parse_line(r#"{"type":"turn.started"}"#), vec![]);
        assert_eq!(p.parse_line(r#"{"type":"item.completed","item":{"id":"i0","type":"reasoning","text":"hmm"}}"#), vec![]);
        let started = p.parse_line(
            r#"{"type":"item.started","item":{"id":"i1","type":"mcp_tool_call","server":"bashcutpro","tool":"command_execute","arguments":{"command":"edit.razor"},"status":"in_progress"}}"#,
        );
        assert!(
            matches!(&started[..], [Event::ToolUse { id, name, input }] if id == "i1" && name == "mcp__bashcutpro__command_execute" && input["command"] == "edit.razor")
        );
        let done = p.parse_line(
            r#"{"type":"item.completed","item":{"id":"i1","type":"mcp_tool_call","server":"bashcutpro","tool":"command_execute","arguments":{"command":"edit.razor"},"result":{"content":[{"type":"text","text":"ok"}]},"status":"completed"}}"#,
        );
        assert_eq!(done, vec![Event::ToolResult { id: "i1".into(), text: "ok".into(), is_error: false }]);
        // a shell command seen only when completed: the call, then its result
        let sh = p.parse_line(r#"{"type":"item.completed","item":{"id":"i2","type":"command_execution","command":"ls","aggregated_output":"x","exit_code":1,"status":"completed"}}"#);
        assert!(matches!(&sh[..], [Event::ToolUse { name, .. }, Event::ToolResult { is_error: true, .. }] if name == "shell"));
        assert_eq!(p.parse_line(r#"{"type":"item.completed","item":{"id":"i3","type":"agent_message","text":"Cut."}}"#), vec![Event::Text("Cut.".into())]);
        let r = p.parse_line(r#"{"type":"turn.completed","usage":{"input_tokens":1}}"#);
        assert!(matches!(&r[0], Event::Result { ok: true, text, session, .. } if text == "Cut." && session == "th-1"));
        let f = p.parse_line(r#"{"type":"turn.failed","error":{"message":"quota"}}"#);
        assert!(matches!(&f[0], Event::Result { ok: false, text, .. } if text == "quota"));
        let e = p.parse_line(
            r#"{"type":"item.completed","item":{"id":"i4","type":"mcp_tool_call","server":"bashcutpro","tool":"t","error":{"message":"no such command"},"status":"failed"}}"#,
        );
        assert!(matches!(&e[..], [Event::ToolUse { .. }, Event::ToolResult { is_error: true, text, .. }] if text == "no such command"));
    }

    #[test]
    fn hostile_lines_do_not_break_it() {
        let mut p = Parser::default();
        assert_eq!(p.parse_line(""), vec![]);
        assert!(matches!(&p.parse_line("not json")[0], Event::Notice(_)));
        assert_eq!(p.parse_line(r#"{"type":"item.completed","item":7}"#), vec![]);
        assert_eq!(p.parse_line(r#"[1,2]"#), vec![]);
        let big = format!(r#"{{"type":"item.completed","item":{{"id":"a","type":"agent_message","text":"{}"}}}}"#, "x".repeat(MAX_TEXT * 2));
        assert!(matches!(&p.parse_line(&big)[0], Event::Text(t) if t.len() <= MAX_TEXT + 4));
        for i in 0..(MAX_STARTED + 10) {
            p.parse_line(&format!(r#"{{"type":"item.started","item":{{"id":"c{i}","type":"command_execution","command":"x"}}}}"#));
        }
        assert!(p.started.len() <= MAX_STARTED);
    }

    #[test]
    fn a_large_tool_call_input_is_truncated() {
        let mut p = Parser::default();
        let call = |id: &str, args: &str| {
            format!(
                r#"{{"type":"item.started","item":{{"id":"{id}","type":"mcp_tool_call","server":"bashcutpro","tool":"command_execute","arguments":{{"text":"{args}"}}}}}}"#
            )
        };
        let big = p.parse_line(&call("big", &"y".repeat(9 * 1024)));
        assert!(matches!(&big[..], [Event::ToolUse { input, .. }] if *input == json!({"truncated": true})), "{big:?}");
        let small = p.parse_line(&call("small", "short"));
        assert!(matches!(&small[..], [Event::ToolUse { input, .. }] if input["text"] == "short"), "{small:?}");
        // file changes are capped the same way
        let changes =
            format!(r#"{{"type":"item.completed","item":{{"id":"f","type":"file_change","changes":["{}"],"status":"completed"}}}}"#, "z".repeat(9 * 1024));
        assert!(matches!(&p.parse_line(&changes)[0], Event::ToolUse { input, .. } if input["truncated"] == true));
    }
}
