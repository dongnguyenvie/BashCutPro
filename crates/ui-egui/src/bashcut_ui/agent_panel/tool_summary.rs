//! What a tool row says (pure, no egui state): the icon for the kind of call, the title, a one-line
//! summary of the input (and cheaply of the output) and the status of a group of calls.

use bashcut_agent::Item;
use filmcraft_time::{FrameRate, format_timecode_frames};
use serde_json::Value;

use crate::icons::Icon;

/// Longest summary built (the row ellipsizes it to its width; this only bounds the work).
const SUMMARY_MAX: usize = 240;
/// A shell command or value longer than this is clipped in a summary.
const VALUE_MAX: usize = 80;
/// Calls of a group whose values are listed before the "…".
const LISTED: usize = 3;
/// The rate timecodes count frames at when there is no active sequence (the input carries seconds, not the sequence).
pub const FALLBACK_RATE: FrameRate = FrameRate::FPS_25;

/// How a group of calls is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Ok,
    Failed,
    /// No output and the turn is over: it was stopped (or the app closed) before the call returned.
    Stopped,
}

/// A group is failed when any call failed; waiting when any has no output: running while the turn
/// that made it runs, stopped otherwise. `turn_running` is "this group belongs to the running turn".
pub fn status(group: &[Item], turn_running: bool) -> ToolStatus {
    if group.iter().any(|g| matches!(g, Item::Tool { is_error: true, .. })) {
        ToolStatus::Failed
    } else if group.iter().any(|g| matches!(g, Item::Tool { output: None, .. })) {
        if turn_running { ToolStatus::Running } else { ToolStatus::Stopped }
    } else {
        ToolStatus::Ok
    }
}

/// The tool's name without the MCP prefix (`mcp__bashcutpro__doc_inspect` → `doc_inspect`).
pub fn short_name(name: &str) -> &str {
    name.rsplit("__").next().unwrap_or(name)
}

fn is_bash(name: &str) -> bool {
    short_name(name) == "Bash"
}

/// The icon for a call: `key` is [`Item::tool_key`] (the engine command id, else the tool name).
pub fn icon_for(key: &str, name: &str) -> Icon {
    let tool = short_name(name);
    let key = key.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| key.contains(w) || tool.to_ascii_lowercase().contains(w));
    match tool {
        "Bash" => return Icon::Code,
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => return Icon::Pen,
        "WebFetch" | "WebSearch" => return Icon::Globe,
        _ => {}
    }
    if has(&["razor", "split"]) {
        Icon::Razor
    } else if has(&["ripple", "delete", "remove"]) {
        Icon::Extract
    } else if has(&["title", "text"]) {
        Icon::Type
    } else if has(&["render_preview", "render_frame", "ui_screenshot", "screenshot"]) {
        Icon::Image
    } else if has(&["media_import", "import"]) {
        Icon::Import
    } else if has(&["export"]) {
        Icon::Export
    } else if has(&["caption"]) {
        Icon::Captions
    } else if has(&["audio"]) {
        Icon::Audio
    } else if has(&["inspect", "list", "search"]) {
        Icon::Search
    } else if has(&["web"]) {
        Icon::Globe
    } else {
        Icon::Wrench
    }
}

/// The row title: the command id for `command_run` and the like, else the short tool name
/// (`sequence inspect`); Bash is "Bash" (its command is the summary).
pub fn title(key: &str, name: &str) -> String {
    if is_bash(name) { "Bash".to_string() } else { short_name(key).replace('_', " ") }
}

/// The arguments of a call: `params` of `command_run`, else the input itself.
fn args(input: &Value) -> &Value {
    input.get("params").filter(|p| p.is_object()).unwrap_or(input)
}

/// A time as `00:00:03:05`: seconds as a number, or a timecode as given.
fn time_text(v: &Value, rate: FrameRate, drop_frame: bool) -> Option<String> {
    match v {
        Value::Number(n) => {
            let secs = n.as_f64().filter(|s| s.is_finite())?;
            let frame = (secs * rate.as_f64()).round();
            (frame.abs() < 1.0e12).then(|| format_timecode_frames(frame as i64, rate, drop_frame))
        }
        Value::String(s) if !s.trim().is_empty() => Some(bashcut_agent::clip(s.trim(), VALUE_MAX)),
        _ => None,
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or(path)
}

/// The parts of one call's input worth a glance: time, clip, track, text, file, command.
fn parts(input: &Value, rate: FrameRate, drop_frame: bool) -> Vec<String> {
    let a = args(input);
    let mut out = Vec::new();
    if let Some(t) = ["seconds", "time", "timecode"].iter().find_map(|k| a.get(*k).and_then(|v| time_text(v, rate, drop_frame))) {
        out.push(t);
    }
    if let Some(c) = a.get("clip") {
        match c {
            Value::Number(n) => out.push(format!("clip {n}")),
            Value::String(s) if !s.is_empty() => out.push(format!("clip {}", bashcut_agent::clip(s, 24))),
            _ => {}
        }
    }
    if let Some(t) = a.get("track") {
        match t {
            Value::String(s) if !s.is_empty() => out.push(bashcut_agent::clip(s, 24)),
            Value::Number(n) => out.push(format!("track {n}")),
            _ => {}
        }
    }
    if let Some(t) = a.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()) {
        out.push(format!("“{}”", bashcut_agent::clip(&t.replace('\n', " "), 40)));
    }
    if let Some(p) = ["file_path", "path"].iter().find_map(|k| a.get(*k).and_then(Value::as_str)).filter(|p| !p.is_empty()) {
        out.push(bashcut_agent::clip(file_name(p), VALUE_MAX));
    }
    out
}

/// `{"clip":17}` as `→ clip 17`: what the call made, when the output says so.
fn output_part(output: Option<&str>) -> Option<String> {
    let v: Value = serde_json::from_str(output?).ok()?;
    match v.get("clip")? {
        Value::Number(n) => Some(format!("→ clip {n}")),
        Value::String(s) if !s.is_empty() => Some(format!("→ clip {}", bashcut_agent::clip(s, 24))),
        _ => None,
    }
}

/// One line for a group of calls (`inputs` and `outputs` side by side): a single call lists its
/// time, clip, track, text or file; several give `×4 · 00:00:03:05, 00:00:18:20, …`; a Bash call is
/// its command. `name` is the tool name (for Bash, the key is the command itself). `rate` and
/// `drop_frame` are the active sequence's ([`FALLBACK_RATE`], non-drop without one).
pub fn summary(key: &str, name: &str, inputs: &[&Value], outputs: &[Option<&str>], rate: FrameRate, drop_frame: bool) -> String {
    let _ = key;
    let text = if is_bash(name) {
        let cmd = |v: &Value| {
            v.get("command").and_then(Value::as_str).map(|c| bashcut_agent::clip(&c.split_whitespace().collect::<Vec<_>>().join(" "), VALUE_MAX * 2))
        };
        let first = inputs.first().and_then(|v| cmd(v)).unwrap_or_default();
        if inputs.len() > 1 { format!("×{} · {first}", inputs.len()) } else { first }
    } else if inputs.len() > 1 {
        let listed: Vec<String> = inputs.iter().take(LISTED).filter_map(|i| parts(i, rate, drop_frame).into_iter().next()).collect();
        let mut list = listed.join(", ");
        if inputs.len() > LISTED && !list.is_empty() {
            list.push_str(", …");
        }
        if list.is_empty() { format!("×{}", inputs.len()) } else { format!("×{} · {list}", inputs.len()) }
    } else {
        let mut p = inputs.first().map(|i| parts(i, rate, drop_frame)).unwrap_or_default();
        p.extend(output_part(outputs.first().copied().flatten()));
        p.join(" ")
    };
    bashcut_agent::clip(&text, SUMMARY_MAX)
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "test ticket: summaries for razor/ripple/title/Bash/Write/unknown, group of 4, long values clipped, Vietnamese labels"]
    fn summaries_by_kind() {}

    #[test]
    #[ignore = "test ticket: seconds 3.2 as a timecode at 25, 30, 29.97 drop-frame and the no-sequence fallback (25)"]
    fn timecode_follows_sequence_rate() {}

    #[test]
    #[ignore = "test ticket: status is Failed, Running while the turn runs, Stopped after, else Ok"]
    fn status_of_a_group() {}
}
