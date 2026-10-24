//! "Show" on a project edit (Agent panel redesign §3.2): a `command_run` call that names a time
//! moves the playhead there (`playhead.set`), one that names a clip selects it (`timeline.select`).
//! Both are existing engine commands, run on the session like any UI action.

use bashcut_agent::Item;
use serde_json::{Value, json};

use super::super::strings::s;
use super::super::widgets::META;
use crate::FilmcraftApp;

/// What Show does for a call.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Target {
    /// `playhead.set` with the call's own time parameter (seconds, timecode, frame or time).
    Time(Value),
    /// `timeline.select` of a clip id.
    Clip(u64),
}

/// The params of an engine-command call: `params` of `command_run`.
fn params(input: &Value) -> Option<&Value> {
    input.get("params").filter(|p| p.is_object())
}

/// Where Show leads for `input`, if it is a `command_run` that names a time or a clip.
pub(super) fn target(name: &str, input: &Value) -> Option<Target> {
    if !name.ends_with("command_run") {
        return None;
    }
    let p = params(input)?;
    for k in ["seconds", "timecode", "frame", "time"] {
        if let Some(v) = p.get(k).filter(|v| v.is_number() || v.as_str().is_some_and(|s| !s.trim().is_empty())) {
            return Some(Target::Time(json!({ k: v })));
        }
    }
    let clip = p.get("clip")?;
    clip.as_u64().or_else(|| clip.as_str().and_then(|s| s.trim().parse().ok())).map(Target::Clip)
}

/// The Show target of a group: its first call that has one.
pub(super) fn group_target(group: &[Item]) -> Option<Target> {
    group.iter().find_map(|g| match g {
        Item::Tool { name, input, .. } => target(name, input),
        _ => None,
    })
}

/// Run the engine command for `target`; the error text when it fails (no sequence, bad time).
pub(super) fn run(app: &mut FilmcraftApp, target: &Target) -> Result<(), String> {
    let (cmd, params) = match target {
        Target::Time(p) => ("playhead.set", p.clone()),
        Target::Clip(id) => ("timeline.select", json!({ "clips": [id] })),
    };
    app.session.execute(cmd, params).map(|_| ()).map_err(|e| e.to_string())
}

/// The Show link under a row: runs `target` when clicked; a failure goes to the status line.
pub(super) fn link(app: &mut FilmcraftApp, ui: &mut egui::Ui, at: usize, target: &Target) {
    let t = app.tokens;
    let r = ui.add(egui::Button::new(egui::RichText::new(s("Show")).size(META).color(t.accent)).small().frame(false));
    app.auto.add_response(&format!("bashcut.agent.tool.{at}.show"), &r, s("Show"));
    if r.clicked()
        && let Err(e) = run(app, target)
    {
        app.ui.status = e;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "test ticket: target() maps seconds/timecode/frame to Time and clip to Clip, only for command_run"]
    fn target_of_a_call() {}
}
