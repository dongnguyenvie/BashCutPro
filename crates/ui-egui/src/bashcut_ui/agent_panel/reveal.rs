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
    use super::*;

    const RUN: &str = "mcp__bashcutpro__command_run";

    #[test]
    fn target_of_a_call() {
        let t = |name: &str, input: Value| target(name, &input);
        assert_eq!(t(RUN, json!({"id": "edit.razor", "params": {"seconds": 3.2}})), Some(Target::Time(json!({"seconds": 3.2}))));
        assert_eq!(t(RUN, json!({"params": {"timecode": "00:00:03:05"}})), Some(Target::Time(json!({"timecode": "00:00:03:05"}))));
        assert_eq!(t(RUN, json!({"params": {"frame": 80}})), Some(Target::Time(json!({"frame": 80}))));
        assert_eq!(t(RUN, json!({"params": {"time": 254016000000u64}})), Some(Target::Time(json!({"time": 254016000000u64}))));
        // a time wins over a clip; a blank time is ignored
        assert_eq!(t(RUN, json!({"params": {"seconds": 1, "clip": 4}})), Some(Target::Time(json!({"seconds": 1}))));
        assert_eq!(t(RUN, json!({"params": {"timecode": "  ", "clip": 4}})), Some(Target::Clip(4)));
        assert_eq!(t(RUN, json!({"params": {"clip": 17}})), Some(Target::Clip(17)));
        assert_eq!(t(RUN, json!({"params": {"clip": " 18 "}})), Some(Target::Clip(18)));
        // nothing to show
        assert_eq!(t(RUN, json!({"params": {"clip": "abc"}})), None);
        assert_eq!(t(RUN, json!({"params": {"text": "hi"}})), None);
        assert_eq!(t(RUN, json!({"id": "edit.razor"})), None);
        assert_eq!(t(RUN, json!({"params": "seconds"})), None);
        assert_eq!(t(RUN, Value::Null), None);
        // only command_run leads anywhere
        assert!(t("command_run", json!({"params": {"seconds": 1}})).is_some());
        assert_eq!(t("mcp__bashcutpro__doc_inspect", json!({"params": {"seconds": 1}})), None);
        assert_eq!(t("Bash", json!({"params": {"seconds": 1}})), None);
    }

    #[test]
    fn a_group_shows_its_first_target() {
        let call = |name: &str, input: Value| Item::Tool { id: "t".into(), name: name.into(), input, output: None, is_error: false };
        let group = [
            call("Bash", json!({"command": "ls"})),
            call(RUN, json!({"params": {"text": "x"}})),
            call(RUN, json!({"params": {"clip": 9}})),
            call(RUN, json!({"params": {"seconds": 2}})),
        ];
        assert_eq!(group_target(&group), Some(Target::Clip(9)));
        assert_eq!(group_target(&group[..2]), None);
        assert_eq!(group_target(&[Item::Notice { message: "n".into() }]), None);
    }
}
