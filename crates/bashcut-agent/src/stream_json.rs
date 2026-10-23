//! Claude Code's stream-json (`claude -p --output-format stream-json --input-format stream-json
//! --verbose`): one JSON object per line. We read `system/init` (session, model, `capabilities`, plugin names),
//! `assistant` (text and tool_use blocks), `user` (tool_result blocks, and the `isReplay` echo of a
//! message Claude took from stdin with `--replay-user-messages`) and `result`; anything else is
//! ignored. The same format is the default `events` of agent plugins.
//!
//! AG6: a sign-in or account problem becomes [`Event::Auth`], read only from the stream (never from
//! credential files). Claude Code 2.1.309 with no login sends a synthetic `assistant` message with
//! `"error": "authentication_failed"` and the text `Not logged in · Please run /login`, then a
//! `result` with `is_error: true` and the same text; a rejected login mid-turn arrives as
//! `system/api_retry` with the same `error` values.
//!
//! AG8: with `--permission-prompt-tool stdio` Claude Code asks the host before a tool call that is
//! not pre-allowed: a `control_request` with subtype `can_use_tool` ([`Event::Permission`]), answered
//! with [`permission_response`]; `control_cancel_request` withdraws one ([`Event::PermissionWithdrawn`]).

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
        /// Names of the Claude Code plugins it loaded (AG9: `bcp` is the BashCutPro kit).
        plugins: Vec<String>,
    },
    /// The agent took the next message from stdin (Claude Code's `isReplay` echo): a queued message
    /// is now being worked on. `uuid` is the echoed message's (ours when we wrote one, AG4; may be empty).
    Picked {
        uuid: String,
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
    /// The agent answered one of our control requests (`control_response`): `ok` is subtype
    /// `success` (AG3: the interrupt's receipt); `cancelled` is the answer's `cancelled: true` (AG4:
    /// `cancel_async_message` withdrew the message).
    Control {
        request: String,
        ok: bool,
        cancelled: bool,
    },
    /// The agent asks before running a tool that is not pre-allowed (AG8); it waits for
    /// [`permission_response`].
    Permission(Permission),
    /// The agent no longer waits for the answer to permission request `request` (it was interrupted).
    PermissionWithdrawn {
        request: String,
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

/// A `can_use_tool` request as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Permission {
    /// Its `request_id`, for the answer.
    pub request: String,
    /// The tool's name (`Bash`, `Write`, `mcp__x__y`).
    pub tool: String,
    /// The command or file it would run or touch (else the agent's description, else the input).
    pub detail: String,
    /// The tool's input as sent (up to [`MAX_PERMISSION_INPUT`]): an allowed call runs with it.
    pub input: Value,
    /// The agent's suggested allow rules (`permission_suggestions` of type `addRules`): what "Always
    /// for this project" writes. Empty: no rule to offer.
    pub rules: Vec<Value>,
    /// The input was longer than [`MAX_PERMISSION_INPUT`] (and dropped): it can only be denied.
    pub oversized: bool,
}

/// Longest tool input a permission request keeps (as JSON text); a longer one is denied at once.
pub const MAX_PERMISSION_INPUT: usize = 256 * 1024;
/// Rules kept from one request's suggestions.
const MAX_RULES: usize = 8;

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
            vec![Event::Init {
                session: text(&v, "session_id"),
                model: text(&v, "model"),
                capabilities: capabilities(v.get("capabilities")),
                plugins: plugins(v.get("plugins")),
            }]
        }
        "user" if v.get("isReplay").and_then(Value::as_bool) == Some(true) => vec![Event::Picked { uuid: clip(&text(&v, "uuid"), 64) }],
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
        "control_response" => {
            let r = v.get("response");
            let field = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_str).unwrap_or_default();
            let cancelled = r.and_then(|r| r.get("response")).and_then(|r| r.get("cancelled")).and_then(Value::as_bool) == Some(true);
            vec![Event::Control { request: clip(field("request_id"), 128), ok: field("subtype") == "success", cancelled }]
        }
        "control_request" => match v.get("request").filter(|r| r.get("subtype").and_then(Value::as_str) == Some("can_use_tool")) {
            Some(r) => vec![Event::Permission(permission(&text(&v, "request_id"), r))],
            None => Vec::new(),
        },
        "control_cancel_request" => vec![Event::PermissionWithdrawn { request: clip(&text(&v, "request_id"), 128) }],
        "error" => vec![Event::Notice(clip(&v.get("error").map(|e| content_text(Some(e))).unwrap_or_else(|| text(&v, "message")), 2048))],
        _ => Vec::new(),
    }
}

/// A `can_use_tool` request's fields (checked with Claude Code 2.1.309: `tool_name`, `input`,
/// `description`, `blocked_path`, `permission_suggestions`, `tool_use_id`).
fn permission(id: &str, r: &Value) -> Permission {
    let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or_default();
    let input = r.get("input").cloned().unwrap_or(Value::Null);
    let input_s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or_default();
    let detail = ["command", "file_path", "notebook_path", "path", "url", "pattern"]
        .iter()
        .map(|k| input_s(k))
        .chain([s("blocked_path"), s("description")])
        .find(|d| !d.trim().is_empty())
        .map_or_else(|| input.to_string(), str::to_string);
    let rules = r
        .get("permission_suggestions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|g| g.get("type").and_then(Value::as_str) == Some("addRules") && g.get("behavior").and_then(Value::as_str) == Some("allow"))
        .filter_map(|g| g.get("rules").and_then(Value::as_array))
        .flatten()
        .filter(|rule| rule.get("toolName").and_then(Value::as_str).is_some_and(|t| !t.is_empty()))
        .take(MAX_RULES)
        .cloned()
        .collect();
    let oversized = input.to_string().len() > MAX_PERMISSION_INPUT;
    let input = if oversized { Value::Null } else { input };
    Permission { request: clip(id, 128), tool: clip(s("tool_name"), 256), detail: clip(&detail, 2048), input, rules, oversized }
}

/// How the user answered a [`Permission`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Once,
    /// Allow, and add the request's rules to the project's `.claude/settings.local.json`.
    Always,
    Deny,
}

/// The `control_response` answering permission request `p` (checked with Claude Code 2.1.309):
/// allow runs the tool with its input as sent; [`Answer::Always`] also passes the rules as
/// `updatedPermissions` with destination `localSettings`, which Claude Code writes into
/// `<cwd>/.claude/settings.local.json` (never the user's own settings); deny tells the agent
/// `message`. No trailing newline.
pub fn permission_response(p: &Permission, answer: Answer, message: &str) -> String {
    let response = match answer {
        Answer::Deny => json!({"behavior": "deny", "message": message}),
        Answer::Once => json!({"behavior": "allow", "updatedInput": p.input}),
        Answer::Always => json!({
            "behavior": "allow",
            "updatedInput": p.input,
            "updatedPermissions": [{"type": "addRules", "rules": p.rules, "behavior": "allow", "destination": "localSettings"}],
        }),
    };
    json!({"type": "control_response", "response": {"subtype": "success", "request_id": p.request, "response": response}}).to_string()
}

/// The strings of `system/init`'s `capabilities`, capped.
fn capabilities(v: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(a)) = v else { return Vec::new() };
    a.iter().filter_map(Value::as_str).take(MAX_CAPABILITIES).map(|c| clip(c, MAX_CAPABILITY)).collect()
}

/// The `name`s of `system/init`'s `plugins` (`[{"name", "path", "source", "version"}]`), capped.
fn plugins(v: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(a)) = v else { return Vec::new() };
    a.iter().filter_map(|p| p.get("name").and_then(Value::as_str)).take(MAX_CAPABILITIES).map(|n| clip(n, MAX_CAPABILITY)).collect()
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

/// [`user_message`] carrying `uuid` (AG4): Claude Code echoes it in the `isReplay` line, and
/// [`cancel_request`] withdraws the message by it while it waits. No trailing newline.
pub fn user_message_with_uuid(text: &str, uuid: &str) -> String {
    json!({"type": "user", "uuid": uuid, "message": {"role": "user", "content": [{"type": "text", "text": text}]}}).to_string()
}

/// The stream-json `cancel_async_message` control request (AG4, checked with Claude Code 2.1.309):
/// drops the message written with `uuid` if the agent has not taken it yet; the answer's
/// `{"cancelled": true}` says it never runs, `false` that it was taken already (or never seen). No
/// trailing newline.
pub fn cancel_request(id: &str, uuid: &str) -> String {
    json!({"type": "control_request", "request_id": id, "request": {"subtype": "cancel_async_message", "message_uuid": uuid}}).to_string()
}

/// The stream-json interrupt control request (AG3, checked with Claude Code 2.1.309): the running
/// turn ends with an `error_during_execution` result, the process stays. `cancel_queued` (capability
/// `interrupt_cancel_queued_v1`) also drops messages written but not taken yet. No trailing newline.
pub fn interrupt_request(id: &str, cancel_queued: bool) -> String {
    let mut request = json!({"subtype": "interrupt"});
    if cancel_queued {
        request["cancel_queued"] = json!(true);
    }
    json!({"type": "control_request", "request_id": id, "request": request}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_turn() {
        let init = parse_line(r#"{"type":"system","subtype":"init","session_id":"s1","model":"m","tools":[]}"#);
        assert_eq!(init, vec![Event::Init { session: "s1".into(), model: "m".into(), capabilities: Vec::new(), plugins: Vec::new() }]);
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
    #[ignore = "test ticket: system/init plugins keep each name (bcp among others), entries without a name skipped, capped"]
    fn init_plugin_names_are_parsed() {}

    #[test]
    #[ignore = "test ticket: authentication_failed / oauth_org_not_allowed / billing_error from assistant, system/api_retry and a failed 'Not logged in' result decode to Event::Auth; other api_retry errors and successful results do not"]
    fn auth_problems_decode_to_auth_events() {}

    #[test]
    #[ignore = "test ticket: control_response success/error parsed to Event::Control with its request_id; interrupt_request shape with and without cancel_queued"]
    fn control_response_and_interrupt_request() {}

    #[test]
    #[ignore = "test ticket: isReplay echo keeps its uuid in Event::Picked; control_response {cancelled:true} sets Control.cancelled; user_message_with_uuid and cancel_request shapes"]
    fn queued_message_uuid_and_cancel_request() {}

    #[test]
    #[ignore = "test ticket: can_use_tool control_request decodes to Event::Permission (tool, detail from command/file_path/blocked_path, addRules allow suggestions kept, input over the cap → truncated); control_cancel_request → PermissionWithdrawn; permission_response shapes for Once / Always (localSettings) / Deny"]
    fn permission_request_and_response() {}

    #[test]
    fn user_message_round_trips() {
        let v: Value = serde_json::from_str(&user_message("xin chào \"quoted\"\nnext")).unwrap();
        assert_eq!(v["message"]["content"][0]["text"], "xin chào \"quoted\"\nnext");
    }
}
