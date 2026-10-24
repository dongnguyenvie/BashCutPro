//! The empty transcript: a line, the example prompts and the gate line.

use super::super::strings::{f, s};
use super::super::widgets::{NOTE, note};
use crate::FilmcraftApp;

/// Kit recipes ([`super::super::palette::RECIPES`]) offered as example prompts on an empty transcript.
const EXAMPLES: &[&str] = &["vlog-plan", "vlog-hook-script", "vlog-publish"];

/// One button per [`EXAMPLES`] recipe; the clicked one's prompt fills the message box (not sent).
pub(super) fn examples(app: &mut FilmcraftApp, ui: &mut egui::Ui) -> Option<String> {
    let mut picked = None;
    ui.add_space(4.0);
    for (id, name, _) in super::super::palette::RECIPES.iter().filter(|(id, ..)| EXAMPLES.contains(id)) {
        let r = ui.button(s(name)).on_hover_text(s("Needs the BashCutPro kit (bcp plugin) for Claude Code."));
        app.auto.add_response(&format!("bashcut.agent.example.{id}"), &r, s(name));
        if r.clicked() {
            picked = Some(format!("Use the bcp:{id} recipe on this project."));
        }
    }
    ui.add_space(4.0);
    picked
}

/// The empty-state block; the clicked example's prompt, if any.
pub(super) fn empty(app: &mut FilmcraftApp, ui: &mut egui::Ui, agent_name: &str) -> Option<String> {
    let t = app.tokens;
    note(ui, f("Ask {agent} to edit this project.", &[("agent", agent_name)]), NOTE, t.text_dim);
    let picked = examples(app, ui);
    note(ui, s("The agent stops at each stage for your approval."), NOTE, t.text_dim);
    picked
}
