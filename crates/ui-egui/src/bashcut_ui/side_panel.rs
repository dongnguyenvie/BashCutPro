//! The side panel left of the rail, and popped-out sections as floating windows
//! (`bashcut/in-app-ui.md` §1). The panel takes its width from the dock area, so it behaves like a
//! docked panel without being in FilmCraft's dock tree.

use egui::{Align2, Rect, Sense, Stroke, pos2, vec2};

use super::state::{MAX_WIDTH, MIN_WIDTH};
use super::strings::s;
use super::{SECTIONS, Section, State};
use crate::FilmcraftApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// The dock keeps at least this much width (px); the panel gives way below it.
const MIN_DOCK: f32 = 320.0;
const HEADER: f32 = 30.0;
/// Width of the resize grip on the panel's left edge.
const GRIP: f32 = 6.0;

pub fn window_id(section: &str) -> egui::Id {
    egui::Id::new(("bashcut.window", section))
}

/// Draw `sec` in a panel at the right of `area`; returns what is left of `area` for the dock.
pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, area: Rect, sec: &Section, st: &mut State) -> Rect {
    let t = app.tokens;
    let room = (area.width() - MIN_DOCK).min(MAX_WIDTH);
    if room < MIN_WIDTH {
        // too narrow a window: keep the panel closed rather than squeeze the dock
        return area;
    }
    let width = st.width.clamp(MIN_WIDTH, room);
    let rect = Rect::from_min_max(pos2(area.max.x - width, area.min.y), area.max);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, t.radius, t.panel_bg);

    // resize grip on the left edge
    let grip = Rect::from_min_max(pos2(rect.min.x - GRIP / 2.0, rect.min.y), pos2(rect.min.x + GRIP / 2.0, rect.max.y));
    let g = ui.interact(grip, egui::Id::new("bashcut.panel.resize"), Sense::drag());
    if g.hovered() || g.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        painter.line_segment([pos2(rect.min.x, rect.min.y), pos2(rect.min.x, rect.max.y)], Stroke::new(2.0, t.accent));
    }
    if g.dragged() {
        st.width = (width - g.drag_delta().x).clamp(MIN_WIDTH, room.max(MIN_WIDTH));
    }
    app.auto.add_response("bashcut.panel.resize", &g, &format!("{width:.0}"));

    // header: title, pop out, close
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER));
    painter.text(pos2(head.min.x + 10.0, head.center().y), Align2::LEFT_CENTER, s(sec.title), Tokens::semibold(13.0), t.text);
    painter.line_segment([pos2(head.min.x, head.max.y), pos2(head.max.x, head.max.y)], Stroke::new(1.0, t.separator));
    app.auto.add("bashcut.panel.title", head, s(sec.title));
    let close = Rect::from_center_size(pos2(head.max.x - 16.0, head.center().y), vec2(20.0, 20.0));
    let pop = Rect::from_center_size(pos2(close.min.x - 14.0, head.center().y), vec2(20.0, 20.0));
    if header_button(app, ui, pop, Icon::Fullscreen, "bashcut.panel.popout", s("Pop out")) {
        st.pop_out(sec.id);
    }
    if header_button(app, ui, close, Icon::Close, "bashcut.panel.close", s("Close")) {
        st.open = None;
    }

    // body
    let inner = Rect::from_min_max(pos2(rect.min.x + 10.0, head.max.y + 6.0), pos2(rect.max.x - 8.0, rect.max.y - 6.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(("bashcut.panel", sec.id)));
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    body(app, &mut child, sec);
    app.auto.add(&format!("bashcut.panel.{}", sec.id), rect, s(sec.title));

    Rect::from_min_max(area.min, pos2(rect.min.x - 1.0, area.max.y))
}

fn header_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, r: Rect, icon: Icon, id: &str, tip: &str) -> bool {
    let t = app.tokens;
    let resp = ui.interact(r, egui::Id::new(id), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    icons::paint(ui.painter(), r.shrink(4.0), icon, if resp.hovered() { t.tab_text_active } else { t.icon });
    app.auto.add_response(id, &resp, tip);
    resp.on_hover_text(tip).clicked()
}

/// A section's content: its blurb, then what its `draw` shows, scrolling.
fn body(app: &mut FilmcraftApp, ui: &mut egui::Ui, sec: &Section) {
    ui.label(egui::RichText::new(s(sec.blurb)).size(11.0).color(app.tokens.text_dim));
    ui.add_space(6.0);
    egui::ScrollArea::vertical().id_salt(("bashcut.scroll", sec.id)).auto_shrink([false, false]).show(ui, |ui| (sec.draw)(app, ui));
}

/// The monitor the app's window is on, as `"<w>x<h>"` (pop-out positions are kept per monitor).
pub fn monitor_key(ctx: &egui::Context) -> String {
    ctx.input(|i| i.viewport().monitor_size)
        .filter(|s| s.x.is_finite() && s.y.is_finite())
        .map_or_else(|| "default".into(), |s| format!("{:.0}x{:.0}", s.x, s.y))
}

/// Popped-out sections, each in its own window with a Dock back button. Each window comes back where it was on this
/// monitor; moving to another monitor puts it where it was there.
pub fn floating(app: &mut FilmcraftApp, ctx: &egui::Context, st: &mut State) {
    let mut dock_back = None;
    let mut closed = Vec::new();
    let monitor = monitor_key(ctx);
    let last_id = egui::Id::new("bashcut.window.monitor");
    let moved = ctx.data(|d| d.get_temp::<String>(last_id)).is_some_and(|m| m != monitor);
    ctx.data_mut(|d| d.insert_temp(last_id, monitor.clone()));
    for sec in SECTIONS.iter().filter(|sec| st.popped.iter().any(|p| p == sec.id)) {
        let mut open = true;
        let mut win =
            egui::Window::new(s(sec.title)).id(window_id(sec.id)).open(&mut open).default_size(vec2(st.width, 520.0)).min_width(MIN_WIDTH).collapsible(false);
        if let Some([x, y, w, h]) = st.window_rect(&monitor, sec.id) {
            let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
            win = if moved { win.current_pos(r.min).default_size(r.size()) } else { win.default_rect(r) };
        }
        let shown = win.show(ctx, |ui| {
            let r = ui.button(s("Dock back"));
            app.auto.add_response(&format!("bashcut.window.{}.dock", sec.id), &r, s("Dock back"));
            if r.clicked() {
                dock_back = Some(sec.id);
            }
            body(app, ui, sec);
        });
        if let Some(w) = shown {
            let r = w.response.rect;
            app.auto.add(&format!("bashcut.window.{}", sec.id), r, s(sec.title));
            st.remember_window(&monitor, sec.id, [r.min.x, r.min.y, r.width(), r.height()]);
        }
        if !open {
            closed.push(sec.id);
        }
    }
    for id in closed {
        st.popped.retain(|p| p != id);
    }
    if let Some(id) = dock_back {
        st.dock_back(id);
    }
}
