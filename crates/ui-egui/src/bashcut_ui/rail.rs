//! The rail: a strip on the right edge with one icon per section (`bashcut/in-app-ui.md` §1).
//!
//! An icon carries a dot while its section has news the user can't see (ticket UX9): Runs while a
//! plugin job runs (accent) or after a run failed since Runs was last on screen (`danger`), Batch
//! while a batch runs. A section on screen (docked or popped out) shows no dot.

use std::sync::PoisonError;

use bashcut_plugin_host::JobState;
use egui::{Rect, Sense, Stroke, pos2, vec2};

use super::strings::{f, s};
use super::{SECTIONS, Section, State, batch, plugins};
use crate::FilmcraftApp;
use crate::icons;

/// Rail width (px).
pub const WIDTH: f32 = 36.0;
/// Size and spacing of a rail button (px).
const BUTTON: f32 = 28.0;
const STEP: f32 = 34.0;
/// Radius of an activity dot (px).
const DOT: f32 = 3.5;

/// News on a section's icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    /// Something runs there.
    Busy,
    /// A run failed that the user has not seen.
    Failed,
}

/// A plugin job that ended badly (the states Runs draws in `danger`).
fn failed(st: JobState) -> bool {
    matches!(st, JobState::Failed | JobState::Crashed | JobState::TimedOut)
}

/// The failed jobs' ids and whether any job runs; nothing before the host exists.
fn runs_activity(ctx: &egui::Context) -> (Vec<u64>, bool) {
    let Some(h) = plugins::existing(ctx) else { return (Vec::new(), false) };
    let g = h.lock().unwrap_or_else(PoisonError::into_inner);
    let failures = g.jobs().iter().filter(|j| failed(j.state)).map(|j| j.id).collect();
    (failures, g.jobs().iter().any(|j| j.state.is_running()))
}

/// The keys that open `sec` (Agent's focus command, else its show command), from the live map.
fn shortcut(app: &FilmcraftApp, sec: &Section) -> Option<String> {
    let keys = if sec.id == "agent" { app.session.shortcuts.primary("bashcut.ui.focusAgent") } else { None };
    keys.or_else(|| app.session.shortcuts.primary(&format!("bashcut.ui.show.{}", sec.id))).map(|k| crate::menus::shortcut_text(&k))
}

/// Draw the rail in `rect`; a click toggles that section in the side panel.
pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let t = app.tokens;
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, t.header_bg);
    painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, t.separator));
    let (failures, runs_busy) = runs_activity(&ctx);
    let batch_busy = batch::lock(&batch::shared(&ctx)).is_running();
    if st.on_screen("runs") {
        st.see_failures(&failures);
    }
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
        let badge = match sec.id {
            _ if open || popped => None,
            "runs" if st.unseen_failure(&failures) => Some(Badge::Failed),
            "runs" if runs_busy => Some(Badge::Busy),
            "batch" if batch_busy => Some(Badge::Busy),
            _ => None,
        };
        if let Some(b) = badge {
            let fill = if b == Badge::Failed { t.danger } else { t.accent };
            let c = pos2(r.max.x - DOT - 1.0, r.min.y + DOT + 1.0);
            painter.circle(c, DOT, fill, Stroke::new(1.5, t.header_bg));
        }
        let title = s(sec.title);
        let label = match badge {
            Some(Badge::Failed) => f("{section}: failed", &[("section", title)]),
            Some(Badge::Busy) => f("{section}: running", &[("section", title)]),
            None => title.to_string(),
        };
        app.auto.add_response(&id, &resp, &label);
        let keys = shortcut(app, sec);
        let resp = resp.on_hover_ui(|ui| {
            ui.set_max_width(260.0);
            ui.label(egui::RichText::new(title).strong());
            ui.label(egui::RichText::new(s(sec.blurb)).size(11.0));
            match badge {
                Some(Badge::Failed) => {
                    ui.label(egui::RichText::new(s("A run failed since you last looked.")).size(11.0).color(t.danger));
                }
                Some(Badge::Busy) => {
                    ui.label(egui::RichText::new(s("Running now.")).size(11.0).color(t.accent));
                }
                None => {}
            }
            if let Some(k) = &keys {
                ui.label(egui::RichText::new(f("Shortcut: {keys}", &[("keys", k)])).size(11.0).color(t.text_dim));
            }
        });
        if resp.clicked() {
            if popped {
                // its window is already open: bring it forward instead of docking a second copy
                ui.ctx().move_to_top(egui::LayerId::new(egui::Order::Middle, super::side_panel::window_id(sec.id)));
            } else {
                st.toggle(sec.id);
            }
        }
    }
}
