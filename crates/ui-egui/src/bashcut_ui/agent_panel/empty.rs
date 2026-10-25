//! The empty transcript: a centred block with the question, one line about the approval stops,
//! example chips that fill the message box (plain ones and kit recipes) and a hint about skills.

use super::super::strings::{f, s};
use super::super::widgets::{NOTE, note};
use crate::FilmcraftApp;
use crate::icons::{Icon, paint};

/// Kit recipes ([`super::super::palette::RECIPES`]) offered as example prompts on an empty transcript.
const EXAMPLES: &[&str] = &["vlog-plan", "vlog-hook-script", "vlog-publish"];

/// Plain examples: `(automation key, chip label, prompt)`, both translated; the chip label is short so it fits on one
/// line in a narrow panel (FX44), the prompt is what fills the message box.
const PLAIN: &[(&str, &str, &str)] =
    &[("remove-silences", "Remove the silences", "Remove the silences"), ("add-captions", "Add captions", "Add captions to the main sequence")];

/// One chip per plain example, then one per [`EXAMPLES`] recipe, wrapping to the panel width; the
/// clicked one's prompt fills the message box (not sent).
pub(super) fn examples(app: &mut FilmcraftApp, ui: &mut egui::Ui) -> Option<String> {
    let mut picked = None;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (key, label, prompt) in PLAIN {
            let r = ui.button(s(label));
            app.auto.add_response(&format!("bashcut.agent.example.{key}"), &r, s(label));
            if r.clicked() {
                picked = Some(s(prompt).to_string());
            }
        }
        for (id, name, _) in super::super::palette::RECIPES.iter().filter(|(id, ..)| EXAMPLES.contains(id)) {
            let r = ui.button(s(name)).on_hover_text(s("Needs the BashCutPro kit (bcp plugin) for Claude Code."));
            app.auto.add_response(&format!("bashcut.agent.example.{id}"), &r, s(name));
            if r.clicked() {
                picked = Some(format!("Use the bcp:{id} recipe on this project."));
            }
        }
    });
    ui.add_space(4.0);
    picked
}

/// The empty-state block; the clicked example's prompt, if any.
pub(super) fn empty(app: &mut FilmcraftApp, ui: &mut egui::Ui, agent_name: &str) -> Option<String> {
    let t = app.tokens;
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
        paint(ui.painter(), rect, Icon::Chat, t.accent);
        ui.add_space(4.0);
        ui.label(egui::RichText::new(s("What should we edit?")).size(14.0).color(t.text));
        note(ui, f("{agent} edits this project with you. It stops at each stage for your approval.", &[("agent", agent_name)]), NOTE, t.text_dim);
    });
    let picked = examples(app, ui);
    ui.vertical_centered(|ui| note(ui, s("Type / for skills"), NOTE, t.text_faint));
    picked
}
