//! Small widgets every section shares, so a button looks and registers the same everywhere
//! (`bashcut/review/ui.md` D2).

use crate::FilmcraftApp;

/// A button with an automation id; `true` when clicked.
pub fn button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let r = ui.add_enabled(enabled, egui::Button::new(label));
    app.auto.add_response(id, &r, label);
    r.clicked()
}

/// [`button`] in the small size (dense rows such as Library's results).
pub fn small_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let r = ui.add_enabled(enabled, egui::Button::new(label).small());
    app.auto.add_response(id, &r, label);
    r.clicked()
}

/// Text size of a note under a control: empty states, hints, inline errors (px).
pub const NOTE: f32 = 11.0;
/// Text size of a meta line: counts, signatures, step details (px).
pub const META: f32 = 10.5;
/// Text size of code: an agent tool call's input and output (px).
pub const CODE: f32 = 10.0;

/// A small coloured label (`t.text_dim` for notes, `t.danger` for inline errors).
pub fn note(ui: &mut egui::Ui, text: impl Into<String>, size: f32, color: egui::Color32) -> egui::Response {
    ui.label(note_text(text, size, color))
}

/// The text of a [`note`], for widgets that take `RichText` (a truncated label, a collapsing header).
pub fn note_text(text: impl Into<String>, size: f32, color: egui::Color32) -> egui::RichText {
    egui::RichText::new(text).size(size).color(color)
}

/// [`note_text`] in the monospace font: signatures, tool calls, logs.
pub fn code_text(text: impl Into<String>, size: f32, color: egui::Color32) -> egui::RichText {
    note_text(text, size, color).monospace()
}

/// A [`note`] in the monospace font.
pub fn code_note(ui: &mut egui::Ui, text: impl Into<String>, size: f32, color: egui::Color32) -> egui::Response {
    ui.label(code_text(text, size, color))
}

/// A bold [`note`]: who speaks in the Agent transcript.
pub fn strong_note(ui: &mut egui::Ui, text: impl Into<String>, size: f32, color: egui::Color32) -> egui::Response {
    ui.label(note_text(text, size, color).strong())
}

/// A [`note`] that wraps and can be selected: a run's error or message, which can be long.
pub fn wrapped_note(ui: &mut egui::Ui, text: impl Into<String>, size: f32, color: egui::Color32) -> egui::Response {
    ui.add(egui::Label::new(note_text(text, size, color)).wrap().selectable(true))
}
