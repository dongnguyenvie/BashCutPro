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

/// The width a floating vertical scroll bar covers at the right of a scroll area's content when
/// hovered (none for a solid one: the area already keeps it out of the content).
pub fn scroll_bar_overlap(ui: &egui::Ui) -> f32 {
    let s = &ui.spacing().scroll;
    if s.floating { (s.bar_inner_margin + s.bar_width + s.bar_outer_margin - s.allocated_width()).max(0.0) } else { 0.0 }
}

/// Show `area` with its content clear of its floating vertical scroll bar: while the content is
/// taller than the area (last frame), it ends [`scroll_bar_overlap`] short of the right edge, so a
/// right-aligned button or a full-width card is not drawn under the bar (FX16, FX18). `key` holds
/// that flag; growing content only narrows, so the width never flips back and forth.
pub fn scroll_clear_of_bar<R>(
    ui: &mut egui::Ui,
    area: egui::ScrollArea,
    key: egui::Id,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    let scrolls = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let out = area.show(ui, |ui| {
        let gap = if scrolls { scroll_bar_overlap(ui) } else { 0.0 };
        if gap <= 0.0 {
            return add(ui);
        }
        let mut rect = ui.available_rect_before_wrap();
        rect.max.x = (rect.max.x - gap).max(rect.min.x);
        let layout = *ui.layout();
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), add).inner
    });
    let taller = out.content_size.y > out.inner_rect.height() + 0.5;
    ui.ctx().data_mut(|d| d.insert_temp(key, taller));
    out
}
