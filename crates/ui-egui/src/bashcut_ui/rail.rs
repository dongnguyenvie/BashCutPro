//! The rail: a strip on the right edge with one icon per section (`bashcut/in-app-ui.md` §1).
//!
//! An icon carries a dot while its section has news the user can't see (ticket UX9): Runs while a
//! plugin job runs (accent) or after a run failed since Runs was last on screen (`danger`), Batch
//! while a batch runs. A section on screen (docked or popped out) shows no dot.

use std::sync::PoisonError;

use bashcut_plugin_host::JobState;
use egui::{Rect, Sense, Stroke, pos2, vec2};

use super::strings::{f, s};
use super::widgets::{NOTE, note};
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
/// Space above the first button (px).
const TOP: f32 = 6.0;
/// Inset of an icon inside its button (px).
const ICON_INSET: f32 = 6.0;
/// The open section's accent bar: distance from the rail's right edge, inset from the button's
/// top and bottom (px).
const BAR_X: f32 = 2.0;
const BAR_INSET: f32 = 4.0;
/// Max width of a button's tooltip (px).
const TIP_WIDTH: f32 = 260.0;

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

/// Runs' dot ([`super::Section::badge`]): a failure not yet seen, else a running job. While Runs is
/// on screen its failures count as seen.
pub fn runs_badge(ctx: &egui::Context, st: &mut State, on_screen: bool) -> Option<Badge> {
    let (failures, busy) = runs_activity(ctx);
    if on_screen {
        st.see_failures(&failures);
    }
    if st.unseen_failure(&failures) { Some(Badge::Failed) } else { busy.then_some(Badge::Busy) }
}

/// Batch's dot: a batch runs.
pub fn batch_badge(ctx: &egui::Context, _: &mut State, _: bool) -> Option<Badge> {
    batch::lock(&batch::shared(ctx)).is_running().then_some(Badge::Busy)
}

/// The keys that open `sec` (its [`Section::shortcut`] command, else its show command), from the live map.
fn shortcut(app: &FilmcraftApp, sec: &Section) -> Option<String> {
    let keys = sec.shortcut.and_then(|c| app.session.shortcuts.primary(c));
    keys.or_else(|| app.session.shortcuts.primary(&format!("bashcut.ui.show.{}", sec.id))).map(|k| crate::menus::shortcut_text(&k))
}

/// Draw the rail in `rect`; a click toggles that section in the side panel.
pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let t = app.tokens;
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, t.header_bg);
    painter.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, t.separator));
    // every section's dot first, also for buttons that don't fit (Runs marks failures seen there)
    let mut badges = Vec::with_capacity(SECTIONS.len());
    for sec in SECTIONS {
        let shown = st.on_screen(sec.id);
        badges.push(sec.badge.and_then(|b| b(&ctx, st, shown)).filter(|_| !shown));
    }
    for (i, sec) in SECTIONS.iter().enumerate() {
        let top = rect.min.y + TOP + STEP * i as f32;
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
            painter.line_segment([pos2(rect.max.x - BAR_X, r.min.y + BAR_INSET), pos2(rect.max.x - BAR_X, r.max.y - BAR_INSET)], Stroke::new(2.0, t.accent));
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
        icons::paint(&painter, r.shrink(ICON_INSET), sec.icon, color);
        let badge = badges.get(i).copied().flatten();
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
            ui.set_max_width(TIP_WIDTH);
            ui.label(egui::RichText::new(title).strong());
            ui.label(egui::RichText::new(s(sec.blurb)).size(11.0));
            match badge {
                Some(Badge::Failed) => {
                    note(ui, s("A run failed since you last looked."), NOTE, t.danger);
                }
                Some(Badge::Busy) => {
                    note(ui, s("Running now."), NOTE, t.accent);
                }
                None => {}
            }
            if let Some(k) = &keys {
                note(ui, f("Shortcut: {keys}", &[("keys", k)]), NOTE, t.text_dim);
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    use serde_json::json;

    use super::*;

    /// An app saving into a fresh data dir with plugin `test.fails`, whose `go` runs about a second,
    /// then exits 4.
    fn app_with_failing_plugin(name: &str) -> (FilmcraftApp, std::path::PathBuf) {
        let data = std::env::temp_dir().join(format!("bashcut-rail-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let plugin = data.join("bashcut-plugins").join("test.fails");
        std::fs::create_dir_all(&plugin).unwrap();
        let manifest = json!({
            "schema": "bashcut-plugin/1", "id": "test.fails", "name": "Fails", "version": "0.1.0", "summary": "test plugin",
            "entry": {"command": ["sh", "-c", "sleep 1; exit 4"], "windows": ["powershell", "-NoProfile", "-Command", "Start-Sleep -Milliseconds 1000; exit 4"]},
            "output": "bashcut-result/1",
            "actions": [{"id": "go", "title": "Go", "args": [], "needs": "none"}]
        });
        std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
        let mut session = filmcraft_engine::Session::default();
        session.start_autosave(filmcraft_engine::autosave::AutosaveConfig::new(&data)).unwrap();
        (FilmcraftApp::new(session), data)
    }

    #[test]
    fn runs_badge_failed_until_seen_and_busy_while_running() {
        let ctx = egui::Context::default();
        let mut st = State::default();
        assert_eq!(runs_badge(&ctx, &mut st, false), None, "no host yet");

        let (app, data) = app_with_failing_plugin("runs");
        let host = plugins::host(&app, &ctx);
        let job = super::super::shared::lock(&host).run("test.fails", "go", &BTreeMap::new(), &plugins::run_context(), None).unwrap();
        assert_eq!(runs_badge(&ctx, &mut st, false), Some(Badge::Busy), "running");
        let until = Instant::now() + Duration::from_secs(60);
        while super::super::shared::lock(&host).job(job).is_some_and(|j| j.state.is_running()) && Instant::now() < until {
            super::super::shared::lock(&host).poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(super::super::shared::lock(&host).job(job).map(|j| j.state), Some(JobState::Failed));

        assert_eq!(runs_badge(&ctx, &mut st, false), Some(Badge::Failed), "failed, not seen");
        assert_eq!(runs_badge(&ctx, &mut st, false), Some(Badge::Failed), "stays until seen");
        assert_eq!(runs_badge(&ctx, &mut st, true), None, "on screen: seen");
        assert_eq!(runs_badge(&ctx, &mut st, false), None, "seen stays seen");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn batch_badge_shows_while_a_batch_runs() {
        let ctx = egui::Context::default();
        let mut st = State::default();
        assert_eq!(batch_badge(&ctx, &mut st, false), None);
        batch::lock(&batch::shared(&ctx)).fake_running(true);
        assert_eq!(batch_badge(&ctx, &mut st, false), Some(Badge::Busy));
        // the rail hides it while Batch is on screen; the badge itself does not depend on that
        assert_eq!(batch_badge(&ctx, &mut st, true), Some(Badge::Busy));
        batch::lock(&batch::shared(&ctx)).fake_running(false);
        assert_eq!(batch_badge(&ctx, &mut st, false), None);
    }
}
