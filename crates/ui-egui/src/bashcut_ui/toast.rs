//! Finished-run toasts and dismissable status lines (`bashcut/in-app-ui.md` §3).
//!
//! A plugin run that ends while Runs is not on screen gets one toast, "✓ <run> finished" or
//! "✕ <run> failed", with Show (opens Runs on that run) and ✕. Toasts are a foreground
//! `egui::Area` above the rail's bottom corner and go after [`TTL`] seconds. A cancelled run gets
//! none: whoever cancelled it knows. State lives in egui context data, like the rest of our UI.

use std::collections::BTreeSet;
use std::time::Duration;

use bashcut_plugin_host::{JobKind, JobState};
use egui::{Align2, vec2};

use super::strings::{f, s};
use super::widgets::button;
use super::{State, plugins, runs_panel};
use crate::FilmcraftApp;

/// Seconds a toast stays.
pub const TTL: f64 = 8.0;
/// Toasts on screen at once; the oldest goes first.
const MAX_SHOWN: usize = 3;

#[derive(Clone, Debug)]
struct Toast {
    job: u64,
    ok: bool,
    title: String,
    since: f64,
}

#[derive(Clone, Default)]
struct Toasts {
    /// Finished runs already toasted or seen in Runs (ids of jobs the host still holds).
    handled: BTreeSet<u64>,
    shown: Vec<Toast>,
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.toasts")
}

/// Toast the runs that finished since the last frame, then draw the toasts. `runs_visible`: the
/// Runs section is on screen (docked or popped out), so nothing new is toasted and toasts go.
pub fn show(app: &mut FilmcraftApp, ctx: &egui::Context, runs_visible: bool) {
    let mut ts = super::shared::temp::<Toasts>(ctx, key());
    let now = ctx.input(|i| i.time);
    // no host yet means nothing ever ran
    let finished: Vec<(u64, Option<bool>, String)> = plugins::existing(ctx).map_or_else(Vec::new, |h| {
        let g = h.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        g.jobs()
            .iter()
            .filter(|j| matches!(j.kind, JobKind::Run { .. }) && !j.state.is_running())
            .map(|j| {
                let ok = match j.state {
                    JobState::Succeeded | JobState::HandOff => Some(true),
                    JobState::Failed | JobState::Crashed | JobState::TimedOut => Some(false),
                    JobState::Cancelled | JobState::Running => None,
                };
                (j.id, ok, j.title.clone())
            })
            .collect()
    });
    ts.handled.retain(|id| finished.iter().any(|(j, ..)| j == id));
    for (job, ok, title) in finished {
        if !ts.handled.insert(job) || runs_visible {
            continue;
        }
        if let Some(ok) = ok {
            ts.shown.push(Toast { job, ok, title, since: now });
        }
    }
    if runs_visible {
        ts.shown.clear();
    }
    ts.shown.retain(|t| now - t.since < TTL && now >= t.since);
    while ts.shown.len() > MAX_SHOWN {
        ts.shown.remove(0);
    }
    if !ts.shown.is_empty() {
        draw(app, ctx, &mut ts);
        ctx.request_repaint_after(Duration::from_millis(500));
    }
    super::shared::put_temp(ctx, key(), ts);
}

/// The toast's sentence, in our language.
pub fn text(ok: bool, title: &str) -> String {
    if ok { f("✓ {action} finished", &[("action", title)]) } else { f("✕ {action} failed", &[("action", title)]) }
}

fn draw(app: &mut FilmcraftApp, ctx: &egui::Context, ts: &mut Toasts) {
    let t = app.tokens;
    let mut gone = Vec::new();
    let mut open = None;
    let offset = vec2(-(super::rail::WIDTH + 12.0), -12.0);
    egui::Area::new(key().with("area")).order(egui::Order::Foreground).anchor(Align2::RIGHT_BOTTOM, offset).show(ctx, |ui| {
        for toast in &ts.shown {
            let id = format!("bashcut.toast.{}", toast.job);
            let line = text(toast.ok, &toast.title);
            let frame = egui::Frame::popup(ui.style()).fill(t.panel_bg).show(ui, |ui| {
                ui.set_max_width(360.0);
                ui.horizontal(|ui| {
                    let color = if toast.ok { t.text } else { t.danger };
                    ui.add(egui::Label::new(egui::RichText::new(&line).color(color)).truncate()).on_hover_text(&toast.title);
                    if button(app, ui, &format!("{id}.show"), s("Show"), true) {
                        open = Some(toast.job);
                    }
                    let r = ui.small_button("✕").on_hover_text(s("Dismiss"));
                    app.auto.add_response(&format!("{id}.dismiss"), &r, s("Dismiss"));
                    if r.clicked() {
                        gone.push(toast.job);
                    }
                });
            });
            app.auto.add(&id, frame.response.rect, &line);
            ui.add_space(4.0);
        }
    });
    if let Some(job) = open {
        State::update(ctx, |st| st.show(super::RUNS));
        runs_panel::focus(ctx, job);
        gone.push(job);
    }
    ts.shown.retain(|t| !gone.contains(&t.job));
}

/// A panel-top status line with ✕: once dismissed it stays hidden until `msg` changes (or the
/// status clears and comes back). `None` draws nothing and forgets the dismissal.
pub fn status_line(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, msg: Option<&str>, color: egui::Color32) {
    let dismissed_key = egui::Id::new(("bashcut.status.dismissed", id));
    let Some(msg) = msg else {
        ui.ctx().data_mut(|d| d.remove::<String>(dismissed_key));
        return;
    };
    if ui.ctx().data(|d| d.get_temp::<String>(dismissed_key)).as_deref() == Some(msg) {
        return;
    }
    ui.horizontal(|ui| {
        let r = ui.small_button("✕").on_hover_text(s("Dismiss"));
        app.auto.add_response(&format!("{id}.dismiss"), &r, s("Dismiss"));
        if r.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(dismissed_key, msg.to_string()));
        }
        let r = ui.add(egui::Label::new(egui::RichText::new(msg).size(11.0).color(color)).wrap());
        app.auto.add_response(id, &r, msg);
    });
}
