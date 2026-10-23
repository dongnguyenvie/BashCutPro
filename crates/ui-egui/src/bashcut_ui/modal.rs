//! `ui.click` while a modal dialog is open (FX14). An egui `Modal` (Recover Unsaved Changes,
//! Revert, Preferences) takes every click outside its own widgets, so a click by id on an element
//! behind it used to answer `ok: true` and do nothing. [`guard!`] (one hook line in the
//! `ui.click` arm of `control.rs`) turns that into an error that names the open dialog and lists
//! its automation ids, so an agent answers the dialog first (`bashcut/in-app-ui.md` §4).

use crate::FilmcraftApp;
use crate::automation::Element;

/// Upstream's modals: egui id of the `Modal`, the name in the error, and the prefix of the
/// automation ids drawn inside it (`panels::file_dialogs`, `panels::settings`).
const KNOWN: [(&str, &str, &str); 3] =
    [("recovery-modal", "Recover Unsaved Changes", "recovery."), ("revert-modal", "Revert", "revert."), ("settings-modal", "Preferences", "settings.")];

/// At most this many of the dialog's ids are listed in the error.
const LIST: usize = 8;

/// Fail a `ui.click` the open modal would swallow: `return`s the error from the control handler.
macro_rules! guard {
    ($app:expr, $ctx:expr, $req:expr, $pos:expr) => {
        if $req.method == "ui.click"
            && let Some(e) = $crate::bashcut_ui::modal::blocked($app, $ctx, $req.params.get("id").and_then(serde_json::Value::as_str), $pos)
        {
            return $crate::control::Outcome::Done(serde_json::json!({"ok": false, "error": e}));
        }
    };
}
pub(crate) use guard;

/// The error for a click at `pos` (on element `id` when given) that the top modal would swallow;
/// `None` when no modal is open, the target is inside it, or it cannot be told.
pub fn blocked(app: &FilmcraftApp, ctx: &egui::Context, id: Option<&str>, pos: egui::Pos2) -> Option<String> {
    let layer = ctx.memory(|m| m.top_modal_layer())?;
    let known = KNOWN.iter().find(|(egui_id, ..)| egui::Id::new(*egui_id) == layer.id);
    // `Some(true)`: drawn inside the modal (or above it); `None`: cannot tell
    let inside = |e: &Element| -> Option<bool> {
        if let Some(r) = e.response_id.and_then(|rid| ctx.read_response(rid)) {
            return Some(ctx.memory(|m| m.allows_interaction(r.layer_id)));
        }
        known.map(|(_, _, prefix)| e.id.starts_with(prefix))
    };
    let elements = || app.auto.previous.iter();
    let target = match id {
        Some(id) => app.auto.find(id)?,
        None => {
            let hit = |e: &&Element| egui::Rect::from_min_size(egui::pos2(e.rect[0], e.rect[1]), egui::vec2(e.rect[2], e.rect[3])).contains(pos);
            if elements().filter(hit).any(|e| inside(e) == Some(true)) {
                return None;
            }
            elements().rev().find(hit)?
        }
    };
    if inside(target) != Some(false) {
        return None;
    }
    let name = known.map_or("a modal dialog".to_string(), |(_, name, _)| format!("“{name}”"));
    let mut ids: Vec<&str> = Vec::new();
    for e in elements().filter(|e| inside(e) == Some(true)) {
        if !ids.contains(&e.id.as_str()) {
            ids.push(&e.id);
        }
    }
    let more = if ids.len() > LIST { ", …" } else { "" };
    ids.truncate(LIST);
    let answer = if ids.is_empty() { "answer it first".to_string() } else { format!("answer it first: {}{more}", ids.join(", ")) };
    Some(format!("{name} is open and takes every click: `{}` is behind it; {answer}", target.id))
}
