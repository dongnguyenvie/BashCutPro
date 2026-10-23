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
//! AG12: `system/init` also lists the session's slash commands (`slash_commands`, plain names such
//! as `bcp:setup` or `compact`, no descriptions; checked with Claude Code 2.1.309) and which of them
//! are skills (`skills`); `terminal_slash_commands` (terminal-only, e.g. `doctor`) and internal
//! `__`-prefixed names are left out ([`SlashCommand`], the message box's `/` list).
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
/// Slash commands kept from `system/init`, and the longest name.
const MAX_COMMANDS: usize = 512;
const MAX_COMMAND: usize = 128;

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
        /// The slash commands and skills a message can start with (AG12), in the agent's order.
        commands: Vec<SlashCommand>,
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

/// One entry of `system/init`'s `slash_commands` (AG12): `/name` in a message runs it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SlashCommand {
    /// Without the `/`; a plugin's carry its namespace (`bcp:setup`).
    pub name: String,
    /// Listed in `skills` too (else a built-in or custom command).
    pub skill: bool,
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
                commands: slash_commands(&v),
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

/// `system/init`'s `slash_commands` without the terminal-only and internal ones, capped; `skill`
/// from `skills`.
fn slash_commands(v: &Value) -> Vec<SlashCommand> {
    let names = |k: &str| v.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).take(MAX_COMMANDS * 2).collect::<Vec<_>>();
    let (skills, terminal) = (names("skills"), names("terminal_slash_commands"));
    let mut out: Vec<SlashCommand> = Vec::new();
    for n in names("slash_commands") {
        let n = n.trim().trim_start_matches('/');
        if n.is_empty()
            || n.starts_with("__")
            || n.len() > MAX_COMMAND
            || n.chars().any(char::is_whitespace)
            || terminal.contains(&n)
            || out.iter().any(|c| c.name == n)
        {
            continue;
        }
        out.push(SlashCommand { name: n.to_string(), skill: skills.contains(&n) });
        if out.len() >= MAX_COMMANDS {
            break;
        }
    }
    out
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
        assert_eq!(init, vec![Event::Init { session: "s1".into(), model: "m".into(), capabilities: Vec::new(), plugins: Vec::new(), commands: Vec::new() }]);
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

    fn init(line: &Value) -> (Vec<String>, Vec<String>) {
        match parse_line(&line.to_string()).as_slice() {
            [Event::Init { capabilities, plugins, .. }] => (capabilities.clone(), plugins.clone()),
            other => panic!("not one Init: {other:?}"),
        }
    }

    #[test]
    fn init_capabilities_and_replay_are_parsed() {
        let line = json!({"type": "system", "subtype": "init", "session_id": "s", "model": "m",
            "capabilities": ["interrupt_receipt_v1", 7, null, {"x": 1}, "interrupt_cancel_queued_v1", "c".repeat(MAX_CAPABILITY * 2)]});
        let (caps, _) = init(&line);
        assert_eq!(caps.len(), 3, "{caps:?}");
        assert_eq!(&caps[..2], ["interrupt_receipt_v1", "interrupt_cancel_queued_v1"]);
        assert_eq!(caps[2], format!("{}…", "c".repeat(MAX_CAPABILITY)));
        let many: Vec<String> = (0..MAX_CAPABILITIES + 10).map(|i| format!("cap{i}")).collect();
        let (caps, _) = init(&json!({"type": "system", "subtype": "init", "capabilities": many}));
        assert_eq!(caps.len(), MAX_CAPABILITIES);
        assert_eq!(caps[0], "cap0");
        // not an array: none
        assert_eq!(init(&json!({"type": "system", "subtype": "init", "capabilities": "interrupt_receipt_v1"})).0, Vec::<String>::new());
        // the isReplay echo is Picked, not a tool result; a plain user line without tool_result is nothing
        let replay = parse_line(r#"{"type":"user","isReplay":true,"message":{"role":"user","content":[{"type":"text","text":"cut"}]}}"#);
        assert_eq!(replay, vec![Event::Picked { uuid: String::new() }]);
        assert_eq!(parse_line(r#"{"type":"user","isReplay":false,"message":{"content":[{"type":"text","text":"cut"}]}}"#), vec![]);
        // another system subtype is ignored
        assert_eq!(parse_line(r#"{"type":"system","subtype":"status","session_id":"s"}"#), vec![]);
    }

    #[test]
    fn init_slash_commands_are_parsed() {
        let cmds = |line: &Value| match parse_line(&line.to_string()).as_slice() {
            [Event::Init { commands, .. }] => commands.clone(),
            other => panic!("not one Init: {other:?}"),
        };
        let sc = |name: &str, skill: bool| SlashCommand { name: name.to_string(), skill };
        let line = json!({"type": "system", "subtype": "init", "session_id": "s", "model": "m",
            "slash_commands": ["bcp:setup", "/compact", "doctor", "__internal", "bcp:setup", 5, null, "has space", "", "x".repeat(MAX_COMMAND + 1), "bcp:review"],
            "skills": ["bcp:setup", "bcp:review"],
            "terminal_slash_commands": ["doctor"]});
        let got = cmds(&line);
        assert_eq!(got, vec![sc("bcp:setup", true), sc("compact", false), sc("bcp:review", true)], "{got:?}");
        // capped
        let many: Vec<String> = (0..MAX_COMMANDS + 20).map(|i| format!("c{i}")).collect();
        let got = cmds(&json!({"type": "system", "subtype": "init", "slash_commands": many}));
        assert_eq!(got.len(), MAX_COMMANDS);
        assert_eq!(got[0], sc("c0", false));
        // missing field, or not an array: empty
        assert!(cmds(&json!({"type": "system", "subtype": "init", "session_id": "s"})).is_empty());
        assert!(cmds(&json!({"type": "system", "subtype": "init", "slash_commands": "compact"})).is_empty());
    }

    #[test]
    fn init_plugin_names_are_parsed() {
        let line = json!({"type": "system", "subtype": "init", "session_id": "s", "model": "m", "plugins": [
            {"name": "superpowers", "path": "/p/s", "source": "superpowers@market"},
            {"name": "bcp", "path": "/p/bcp", "source": "bcp@bashcutpro-agent-kit", "version": "0.3.0"},
            {"path": "/p/nameless"},
            {"name": 5},
            "bcp-as-a-string",
            {"name": "n".repeat(MAX_CAPABILITY * 2)},
        ]});
        let (_, plugins) = init(&line);
        assert_eq!(plugins.len(), 3, "{plugins:?}");
        assert_eq!(&plugins[..2], ["superpowers", "bcp"]);
        assert_eq!(plugins[2], format!("{}…", "n".repeat(MAX_CAPABILITY)));
        let many: Vec<Value> = (0..MAX_CAPABILITIES + 3).map(|i| json!({"name": format!("p{i}")})).collect();
        let (_, plugins) = init(&json!({"type": "system", "subtype": "init", "plugins": many}));
        assert_eq!(plugins.len(), MAX_CAPABILITIES);
        assert_eq!(init(&json!({"type": "system", "subtype": "init", "plugins": {"name": "bcp"}})).1, Vec::<String>::new());
    }

    #[test]
    fn auth_problems_decode_to_auth_events() {
        let auth = |category: &str, message: &str| Event::Auth { category: category.into(), message: message.into() };
        // Claude Code 2.1.309 with no login: a synthetic assistant message, then a failed result
        let a = parse_line(
            r#"{"type":"assistant","error":"authentication_failed","message":{"content":[{"type":"text","text":"Not logged in · Please run /login"}]}}"#,
        );
        assert_eq!(a, vec![auth(NOT_SIGNED_IN, "Not logged in · Please run /login")]);
        for category in AUTH_ERRORS {
            let line = json!({"type": "assistant", "error": category, "message": {"content": "account problem"}});
            assert_eq!(parse_line(&line.to_string()), vec![auth(category, "account problem")]);
            let retry = json!({"type": "system", "subtype": "api_retry", "attempt": 1, "error": category});
            assert_eq!(parse_line(&retry.to_string()), vec![auth(category, "")]);
        }
        let r = parse_line(r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login","session_id":"s"}"#);
        assert_eq!(r.len(), 2, "{r:?}");
        assert_eq!(r[0], auth(NOT_SIGNED_IN, "Not logged in · Please run /login"));
        assert!(matches!(&r[1], Event::Result { ok: false, .. }), "the Auth comes first, then the result");
        let r = parse_line(r#"{"type":"result","is_error":true,"result":"Invalid API key · Please run /login"}"#);
        assert_eq!(r[0], auth(NOT_SIGNED_IN, "Invalid API key · Please run /login"));
        // a result carrying an auth error field is Auth too
        let r = parse_line(r#"{"type":"result","is_error":true,"error":"billing_error","result":"Credit balance is too low"}"#);
        assert_eq!(r[0], auth("billing_error", "Credit balance is too low"));

        // not auth: other retries, other assistant errors, successful results, failed results on other things
        assert_eq!(parse_line(r#"{"type":"system","subtype":"api_retry","attempt":2,"error":"rate_limit"}"#), vec![]);
        assert_eq!(parse_line(r#"{"type":"system","subtype":"api_retry","attempt":2}"#), vec![]);
        let other = parse_line(r#"{"type":"assistant","error":"server_error","message":{"content":[{"type":"text","text":"Overloaded"}]}}"#);
        assert_eq!(other, vec![Event::Text("Overloaded".into())]);
        let ok = parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"You are not logged in to YouTube, so I skipped the upload."}"#);
        assert!(matches!(ok.as_slice(), [Event::Result { ok: true, .. }]), "{ok:?}");
        let failed = parse_line(r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#);
        assert!(matches!(failed.as_slice(), [Event::Result { ok: false, .. }]), "{failed:?}");
    }

    #[test]
    fn control_response_and_interrupt_request() {
        let ok = parse_line(r#"{"type":"control_response","response":{"subtype":"success","request_id":"int-1","response":{}}}"#);
        assert_eq!(ok, vec![Event::Control { request: "int-1".into(), ok: true, cancelled: false }]);
        let bad = parse_line(r#"{"type":"control_response","response":{"subtype":"error","request_id":"int-2","error":"no turn running"}}"#);
        assert_eq!(bad, vec![Event::Control { request: "int-2".into(), ok: false, cancelled: false }]);
        // no response object: a Control with nothing in it, not a crash
        assert_eq!(parse_line(r#"{"type":"control_response"}"#), vec![Event::Control { request: String::new(), ok: false, cancelled: false }]);
        let long = json!({"type": "control_response", "response": {"subtype": "success", "request_id": "r".repeat(500)}});
        assert!(matches!(&parse_line(&long.to_string())[0], Event::Control { request, .. } if *request == format!("{}…", "r".repeat(128))));

        let plain: Value = serde_json::from_str(&interrupt_request("int-1", false)).unwrap();
        assert_eq!(plain, json!({"type": "control_request", "request_id": "int-1", "request": {"subtype": "interrupt"}}));
        let queued: Value = serde_json::from_str(&interrupt_request("int-2", true)).unwrap();
        assert_eq!(queued, json!({"type": "control_request", "request_id": "int-2", "request": {"subtype": "interrupt", "cancel_queued": true}}));
        assert!(!interrupt_request("x", true).ends_with('\n'));
    }

    #[test]
    fn queued_message_uuid_and_cancel_request() {
        let replay = parse_line(r#"{"type":"user","isReplay":true,"uuid":"msg-7","message":{"role":"user","content":[{"type":"text","text":"cut"}]}}"#);
        assert_eq!(replay, vec![Event::Picked { uuid: "msg-7".into() }]);
        let long = json!({"type": "user", "isReplay": true, "uuid": "u".repeat(300)});
        assert!(matches!(&parse_line(&long.to_string())[0], Event::Picked { uuid } if *uuid == format!("{}…", "u".repeat(64))));

        let cancelled = parse_line(r#"{"type":"control_response","response":{"subtype":"success","request_id":"c-1","response":{"cancelled":true}}}"#);
        assert_eq!(cancelled, vec![Event::Control { request: "c-1".into(), ok: true, cancelled: true }]);
        let taken = parse_line(r#"{"type":"control_response","response":{"subtype":"success","request_id":"c-2","response":{"cancelled":false}}}"#);
        assert_eq!(taken, vec![Event::Control { request: "c-2".into(), ok: true, cancelled: false }]);
        let odd = parse_line(r#"{"type":"control_response","response":{"subtype":"success","request_id":"c-3","response":{"cancelled":"yes"}}}"#);
        assert_eq!(odd, vec![Event::Control { request: "c-3".into(), ok: true, cancelled: false }]);

        let m: Value = serde_json::from_str(&user_message_with_uuid("cắt \"đoạn\" này", "msg-7")).unwrap();
        assert_eq!(m, json!({"type": "user", "uuid": "msg-7", "message": {"role": "user", "content": [{"type": "text", "text": "cắt \"đoạn\" này"}]}}));
        let c: Value = serde_json::from_str(&cancel_request("c-1", "msg-7")).unwrap();
        assert_eq!(c, json!({"type": "control_request", "request_id": "c-1", "request": {"subtype": "cancel_async_message", "message_uuid": "msg-7"}}));
        assert!(!user_message_with_uuid("x", "u").ends_with('\n') && !cancel_request("c", "u").ends_with('\n'));
    }

    fn permission_of(request: Value) -> Permission {
        let line = json!({"type": "control_request", "request_id": "perm-1", "request": request});
        match parse_line(&line.to_string()).as_slice() {
            [Event::Permission(p)] => p.clone(),
            other => panic!("not one Permission: {other:?}"),
        }
    }

    #[test]
    fn permission_request_and_response() {
        let rule = json!({"toolName": "Bash", "ruleContent": "npm test:*"});
        let p = permission_of(json!({
            "subtype": "can_use_tool",
            "tool_name": "Bash",
            "input": {"command": "npm test", "description": "Run the tests"},
            "tool_use_id": "toolu_1",
            "permission_suggestions": [
                {"type": "addRules", "rules": [rule, {"toolName": ""}, {"ruleContent": "x"}], "behavior": "allow", "destination": "localSettings"},
                {"type": "addRules", "rules": [{"toolName": "Bash"}], "behavior": "deny", "destination": "localSettings"},
                {"type": "setMode", "mode": "acceptEdits", "destination": "session"},
            ],
        }));
        assert_eq!((p.request.as_str(), p.tool.as_str(), p.detail.as_str()), ("perm-1", "Bash", "npm test"));
        assert_eq!(p.input, json!({"command": "npm test", "description": "Run the tests"}));
        assert_eq!(p.rules, vec![rule.clone()], "only allow rules with a tool name");
        assert!(!p.oversized);

        // the detail: a file path, else blocked_path, else the description, else the input
        let write = permission_of(json!({"subtype": "can_use_tool", "tool_name": "Write", "input": {"file_path": "/p/a.txt", "content": "x"}}));
        assert_eq!(write.detail, "/p/a.txt");
        assert!(write.rules.is_empty());
        let blocked = permission_of(json!({"subtype": "can_use_tool", "tool_name": "Glob", "input": {"x": 1}, "blocked_path": "/etc", "description": "d"}));
        assert_eq!(blocked.detail, "/etc");
        let described = permission_of(json!({"subtype": "can_use_tool", "tool_name": "mcp__x__y", "input": {"x": 1}, "description": "Do y"}));
        assert_eq!(described.detail, "Do y");
        let bare = permission_of(json!({"subtype": "can_use_tool", "tool_name": "mcp__x__y", "input": {"x": 1}}));
        assert_eq!(bare.detail, r#"{"x":1}"#);
        let many = permission_of(json!({"subtype": "can_use_tool", "tool_name": "Bash", "input": {},
            "permission_suggestions": [{"type": "addRules", "behavior": "allow", "rules": vec![rule.clone(); MAX_RULES + 4]}]}));
        assert_eq!(many.rules.len(), MAX_RULES);

        // an input over the cap is dropped: the request can only be denied
        let huge = permission_of(json!({"subtype": "can_use_tool", "tool_name": "Write",
            "input": {"file_path": "/p/big.txt", "content": "z".repeat(MAX_PERMISSION_INPUT)}}));
        assert!(huge.oversized);
        assert_eq!(huge.input, Value::Null);
        assert_eq!(huge.detail, "/p/big.txt");

        // other control requests are not permissions; a cancel withdraws one
        assert_eq!(parse_line(r#"{"type":"control_request","request_id":"x","request":{"subtype":"hook_callback"}}"#), vec![]);
        assert_eq!(parse_line(r#"{"type":"control_cancel_request","request_id":"perm-1"}"#), vec![Event::PermissionWithdrawn { request: "perm-1".into() }]);

        let answer = |a: Answer| serde_json::from_str::<Value>(&permission_response(&p, a, "The user denied this in BashCutPro.")).unwrap();
        let envelope = |response: Value| json!({"type": "control_response", "response": {"subtype": "success", "request_id": "perm-1", "response": response}});
        assert_eq!(answer(Answer::Once), envelope(json!({"behavior": "allow", "updatedInput": p.input})));
        assert_eq!(
            answer(Answer::Always),
            envelope(json!({"behavior": "allow", "updatedInput": p.input,
                "updatedPermissions": [{"type": "addRules", "rules": [rule], "behavior": "allow", "destination": "localSettings"}]}))
        );
        assert_eq!(answer(Answer::Deny), envelope(json!({"behavior": "deny", "message": "The user denied this in BashCutPro."})));
        assert!(!permission_response(&p, Answer::Once, "").ends_with('\n'));
    }

    #[test]
    fn user_message_round_trips() {
        let v: Value = serde_json::from_str(&user_message("xin chào \"quoted\"\nnext")).unwrap();
        assert_eq!(v["message"]["content"][0]["text"], "xin chào \"quoted\"\nnext");
    }
}
