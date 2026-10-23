//! Claude Code's stream-json (`claude -p --output-format stream-json --input-format stream-json
//! --verbose`): one JSON object per line. We read `system/init` (session, model, `capabilities`),
//! `assistant` (text and tool_use blocks), `user` (tool_result blocks, and the `isReplay` echo of a
//! message Claude took from stdin with `--replay-user-messages`) and `result`; anything else is
//! ignored. The same format is the default `events` of agent plugins.
//!
//! AG6: a sign-in or account problem becomes [`Event::Auth`], read only from the stream (never from
//! credential files). Claude Code 2.1.309 with no login sends a synthetic `assistant` message with
//! `"error": "authentication_failed"` and the text `Not logged in · Please run /login`, then a
//! `result` with `is_error: true` and the same text; a rejected login mid-turn arrives as
//! `system/api_retry` with the same `error` values.

use serde_json::{Value, json};

use crate::clip;

/// The event format name in agent specs and plugin manifests.
pub const FORMAT: &str = bashcut_manifest::CLAUDE_STREAM_JSON;
/// Longest text kept from one block.
pub const MAX_TEXT: usize = 32 * 1024;
/// Longest tool input kept (as JSON text).
const MAX_INPUT: usize = 8 * 1024;
/// `capabilities` kept from `system/init`, and the longest one.
const MAX_CAPABILITIES: usize = 64;
const MAX_CAPABILITY: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The agent's session started (`--resume` takes this id next turn). Claude Code sends one per
    /// turn of a live session.
    Init {
        session: String,
        model: String,
        /// What the agent supports (`interrupt_receipt_v1`, …): feature-detect from it, unknown
        /// values are kept and ignored.
        capabilities: Vec<String>,
    },
    /// The agent took the next message from stdin (Claude Code's `isReplay` echo): a queued message
    /// is now being worked on.
    Picked,
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
    /// Claude Code is not signed in, or its account cannot be used: `category` is one of
    /// [`AUTH_ERRORS`] (`authentication_failed` when the user has to sign in), `message` the
    /// agent's own words (may be empty).
    Auth {
        category: String,
        message: String,
    },
}

/// The `error` values of `assistant` and `system/api_retry` events that mean "sign in" or "account
/// problem" rather than a failed turn.
pub const AUTH_ERRORS: [&str; 3] = [NOT_SIGNED_IN, "oauth_org_not_allowed", "billing_error"];
/// The [`Event::Auth`] category of a missing or expired login.
pub const NOT_SIGNED_IN: &str = "authentication_failed";

/// The auth category in `v`'s `error` field, if it is one of [`AUTH_ERRORS`].
fn auth_error(v: &Value) -> Option<&'static str> {
    let e = v.get("error").and_then(Value::as_str)?;
    AUTH_ERRORS.iter().copied().find(|c| *c == e)
}

/// A failed result's text that says the user is not logged in (`Not logged in · Please run /login`,
/// `Invalid API key · Please run /login`).
fn says_not_signed_in(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("not logged in") || t.contains("please run /login") || t.contains("invalid api key")
}

/// The stateless [`crate::format::Decode`]r of this format.
pub struct Parser;

impl crate::format::Decode for Parser {
    fn parse_line(&mut self, line: &str) -> Vec<Event> {
        parse_line(line)
    }
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
            vec![Event::Init { session: text(&v, "session_id"), model: text(&v, "model"), capabilities: capabilities(v.get("capabilities")) }]
        }
        "user" if v.get("isReplay").and_then(Value::as_bool) == Some(true) => vec![Event::Picked],
        "system" if v.get("subtype").and_then(Value::as_str) == Some("api_retry") => match auth_error(&v) {
            Some(category) => vec![Event::Auth { category: category.into(), message: String::new() }],
            None => Vec::new(),
        },
        "assistant" if auth_error(&v).is_some() => {
            let message = blocks(&v).iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n");
            let category = auth_error(&v).unwrap_or(NOT_SIGNED_IN);
            vec![Event::Auth { category: category.into(), message: clip(&message, 2048) }]
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
            let result = Event::Result {
                ok,
                text: clip(&t, MAX_TEXT),
                session: text(&v, "session_id"),
                cost_usd: v.get("total_cost_usd").and_then(Value::as_f64).filter(|c| c.is_finite()),
                turns: v.get("num_turns").and_then(Value::as_u64),
            };
            // the Auth event first, so the transcript shows the sign-in line instead of a generic error
            let category = auth_error(&v).or_else(|| (!ok && says_not_signed_in(&t)).then_some(NOT_SIGNED_IN));
            match category {
                Some(category) => vec![Event::Auth { category: category.into(), message: clip(&t, 2048) }, result],
                None => vec![result],
            }
        }
        "error" => vec![Event::Notice(clip(&v.get("error").map(|e| content_text(Some(e))).unwrap_or_else(|| text(&v, "message")), 2048))],
        _ => Vec::new(),
    }
}

/// The strings of `system/init`'s `capabilities`, capped.
fn capabilities(v: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(a)) = v else { return Vec::new() };
    a.iter().filter_map(Value::as_str).take(MAX_CAPABILITIES).map(|c| clip(c, MAX_CAPABILITY)).collect()
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

/// A tool's input, or `{"truncated": true}` when its JSON is longer than [`MAX_INPUT`] (Codex uses it too).
pub(crate) fn capped_input(v: Option<&Value>) -> Value {
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
        assert_eq!(init, vec![Event::Init { session: "s1".into(), model: "m".into(), capabilities: Vec::new() }]);
        let a = parse_line(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Cutting."},{"type":"tool_use","id":"t1","name":"mcp__bashcutpro__command_execute","input":{"command":"edit.razor"}}]}}"#,
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
    #[ignore = "test ticket: system/init capabilities parsed (strings kept, non-strings skipped, capped at 64), isReplay user line is Picked"]
    fn init_capabilities_and_replay_are_parsed() {}

    #[test]
    #[ignore = "test ticket: authentication_failed / oauth_org_not_allowed / billing_error from assistant, system/api_retry and a failed 'Not logged in' result decode to Event::Auth; other api_retry errors and successful results do not"]
    fn auth_problems_decode_to_auth_events() {}

    #[test]
    fn user_message_round_trips() {
        let v: Value = serde_json::from_str(&user_message("xin chào \"quoted\"\nnext")).unwrap();
        assert_eq!(v["message"]["content"][0]["text"], "xin chào \"quoted\"\nnext");
    }
}
