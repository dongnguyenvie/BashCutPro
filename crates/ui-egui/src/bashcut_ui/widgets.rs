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
