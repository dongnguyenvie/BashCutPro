//! The rail: a strip on the right edge with one icon per section (`bashcut/in-app-ui.md` §1).

use egui::{Rect, Sense, Stroke, pos2, vec2};

use super::strings::s;
use super::{SECTIONS, State};
use crate::FilmcraftApp;
use crate::icons;

/// Rail width (px).
pub const WIDTH: f32 = 36.0;
/// Size and spacing of a rail button (px).
const BUTTON: f32 = 28.0;
const STEP: f32 = 34.0;

/// Draw the rail in `rect`; a click toggles that section in the side panel.
pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let t = app.tokens;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, t.header_bg);
    painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, t.separator));
    for (i, sec) in SECTIONS.iter().enumerate() {
        let top = rect.min.y + 6.0 + STEP * i as f32;
        if top + BUTTON > rect.max.y {
            break;
        }
        let r = Rect::from_min_size(pos2(rect.center().x - BUTTON / 2.0, top), vec2(BUTTON, BUTTON));
        let id = format!("bashcut.rail.{}", sec.id);
        let resp = ui.interact(r, egui::Id::new(&id), Sense::click());
        let open = st.open.as_deref() == Some(sec.id);
        let popped = st.popped.iter().any(|p| p == sec.id);
        if open {
            painter.rect_filled(r, t.radius_sm, t.pill_active_bg);
            painter.line_segment([pos2(rect.max.x - 2.0, r.min.y + 4.0), pos2(rect.max.x - 2.0, r.max.y - 4.0)], Stroke::new(2.0, t.accent));
        } else if resp.hovered() {
            painter.rect_filled(r, t.radius_sm, t.hover);
        }
        let color = if open || popped {
            t.icon_active
        } else if resp.hovered() {
            t.tab_text_active
        } else {
            t.icon
        };
        icons::paint(&painter, r.shrink(6.0), sec.icon, color);
        app.auto.add_response(&id, &resp, s(sec.title));
        if resp.on_hover_text(s(sec.title)).clicked() {
            if popped {
                // its window is already open: bring it forward instead of docking a second copy
                ui.ctx().move_to_top(egui::LayerId::new(egui::Order::Middle, super::side_panel::window_id(sec.id)));
            } else {
                st.toggle(sec.id);
            }
        }
    }
}
