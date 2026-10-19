//! The Runs section (`bashcut/in-app-ui.md` §2): plugin runs, installs and registry checks, newest
//! first, with progress, the outcome, the log, Cancel, and Send to agent for a hand-off.

use std::collections::BTreeSet;

use bashcut_plugin_host::{Job, JobState};
use serde_json::json;

use super::plugins;
use super::plugins_panel::button;
use super::strings::{f, s};
use crate::FilmcraftApp;

/// Jobs whose log is unfolded, and the last command error.
#[derive(Clone, Default)]
struct View {
    logs: BTreeSet<u64>,
    status: Option<String>,
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.runs.view")
}

fn state_label(st: JobState) -> &'static str {
    match st {
        JobState::Running => "Running",
        JobState::Succeeded => "Done",
        JobState::Failed => "Failed",
        JobState::HandOff => "Needs the agent",
        JobState::Cancelled => "Cancelled",
        JobState::TimedOut => "Timed out",
        JobState::Crashed => "Crashed",
    }
}

pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let t = app.tokens;
    let mut v = ctx.data_mut(|d| d.get_temp_mut_or_default::<View>(key()).clone());
    let jobs: Vec<Job> = plugins::with(app, &ctx, |h| h.jobs().iter().rev().cloned().collect());
    let call = |app: &mut FilmcraftApp, v: &mut View, name: &str, params: serde_json::Value| match plugins::invoke(app, &ctx, name, &params) {
        Ok(_) => v.status = None,
        Err(e) => v.status = Some(e),
    };

    if let Some(e) = &v.status {
        let r = ui.label(egui::RichText::new(e).size(11.0).color(egui::Color32::from_rgb(0xe0, 0x6c, 0x5f)));
        app.auto.add_response("bashcut.runs.status", &r, e);
    }
    if jobs.is_empty() {
        ui.label(egui::RichText::new(s("Nothing has run yet.")).size(11.0).color(t.text_dim));
    } else if button(app, ui, "bashcut.runs.clear", s("Clear finished"), jobs.iter().any(|j| !j.state.is_running())) {
        call(app, &mut v, "clear", json!({}));
    }

    for j in &jobs {
        ui.add_space(4.0);
        let frame = egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&j.title).color(t.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(s(state_label(j.state))).size(11.0).color(if j.state == JobState::Succeeded { t.accent } else { t.text_dim }));
                });
            });
            if j.state.is_running() {
                let bar = match j.progress {
                    Some(p) => egui::ProgressBar::new(p.clamp(0.0, 1.0)).show_percentage(),
                    None => egui::ProgressBar::new(0.0).animate(true),
                };
                let r = ui.add(bar.desired_height(6.0));
                app.auto.add_response(&format!("bashcut.runs.{}.progress", j.id), &r, &j.message);
            }
            if !j.message.is_empty() {
                ui.label(egui::RichText::new(&j.message).size(11.0).color(t.text_dim));
            }
            super::view::job_view(app, ui, j);
            if !j.state.is_running() {
                let secs = format!("{:.1}", j.elapsed_ms as f64 / 1000.0);
                ui.label(egui::RichText::new(f("{seconds} s", &[("seconds", &secs)])).size(10.5).color(t.text_dim));
            }
            ui.horizontal(|ui| {
                if j.state.is_running() && button(app, ui, &format!("bashcut.runs.{}.cancel", j.id), s("Cancel"), true) {
                    call(app, &mut v, "cancel", json!({"job": j.id}));
                }
                let open = v.logs.contains(&j.id);
                if button(app, ui, &format!("bashcut.runs.{}.log", j.id), if open { s("Hide log") } else { s("Log") }, !j.log.is_empty()) {
                    if open {
                        v.logs.remove(&j.id);
                    } else {
                        v.logs.insert(j.id);
                    }
                }
                if j.state == JobState::HandOff && button(app, ui, &format!("bashcut.runs.{}.agent", j.id), s("Send to agent"), true) {
                    call(app, &mut v, "sendToAgent", json!({"job": j.id}));
                    super::State::update(&ctx, |st| st.show("agent"));
                }
            });
            if v.logs.contains(&j.id) {
                let text: String = j.log.iter().map(String::as_str).collect::<Vec<_>>().join("\n");
                egui::ScrollArea::vertical().id_salt(("bashcut.runs.log", j.id)).max_height(160.0).show(ui, |ui| {
                    ui.label(egui::RichText::new(text).monospace().size(10.5).color(t.text_dim));
                });
            }
        });
        app.auto.add(&format!("bashcut.runs.{}", j.id), frame.response.rect, &j.title);
    }
    v.logs.retain(|id| jobs.iter().any(|j| j.id == *id));
    super::view::retain(&ctx, &jobs.iter().map(|j| j.id).collect::<Vec<_>>());
    ctx.data_mut(|d| d.insert_temp(key(), v));
}
