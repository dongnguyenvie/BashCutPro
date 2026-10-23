//! The Runs section (`bashcut/in-app-ui.md` §2): plugin runs, installs and registry checks, newest
//! first, with progress, elapsed time, the outcome (the full error of a failed one), the log,
//! Cancel, Retry, Copy log, Reveal for the files a result names, and Send to agent for a
//! hand-off. A running batch gets one row that links to the Batch section.

use std::collections::BTreeSet;

use bashcut_plugin_host::{Job, JobKind, JobState};
use serde_json::{Value, json};

use super::batch;
use super::plugins;
use super::strings::{f, s};
use super::widgets::{META, NOTE, button, note};
use crate::FilmcraftApp;

/// `result.data` keys whose values are files a run wrote (`bashcut/plugin-host.md` §Results): a
/// path string, or a list of path strings or `{path}` objects.
pub const RESULT_PATH_KEYS: [&str; 4] = ["path", "paths", "files", "outputs"];
/// Reveal buttons shown per run.
const MAX_REVEAL: usize = 8;
/// Characters of one error part shown (plugin output is hostile: it can be megabytes).
const MAX_ERROR: usize = 4000;

/// Jobs whose log is unfolded, the last command error, and the run to scroll to.
#[derive(Clone, Default)]
struct View {
    logs: BTreeSet<u64>,
    status: Option<String>,
    focus: Option<u64>,
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.runs.view")
}

/// Scroll Runs to `job` the next time it draws (a toast's Show).
pub fn focus(ctx: &egui::Context, job: u64) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<View>(key()).focus = Some(job));
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

/// Done and a hand-off are good news (accent), a run that broke is `danger`, the rest is quiet.
fn state_color(st: JobState, t: &crate::theme::Tokens) -> egui::Color32 {
    match st {
        JobState::Succeeded | JobState::HandOff => t.accent,
        JobState::Failed | JobState::Crashed | JobState::TimedOut => t.danger,
        JobState::Running | JobState::Cancelled => t.text_dim,
    }
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(MAX_ERROR) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

/// The full error of a run that broke, one part per line: the plugin's `error.code: message`,
/// else the job's message (the host's error plus the plugin's last line), then the host's error
/// when it says more. `None` for a run that did not break.
pub fn error_text(j: &Job) -> Option<String> {
    if !matches!(j.state, JobState::Failed | JobState::Crashed | JobState::TimedOut) {
        return None;
    }
    let outcome = j.outcome.as_ref();
    let host = outcome.and_then(|o| o.error.as_deref()).map(str::trim).filter(|e| !e.is_empty());
    let mut lines: Vec<String> = Vec::new();
    match outcome.and_then(|o| o.result.as_ref()).and_then(|r| r.error.as_ref()) {
        Some(e) => lines.push(if e.code.is_empty() { e.message.clone() } else { format!("{}: {}", e.code, e.message) }),
        None if !j.message.trim().is_empty() => lines.push(j.message.trim().to_string()),
        None => {}
    }
    if let Some(h) = host.filter(|h| !lines.iter().any(|l| l.contains(h))) {
        lines.push(h.to_string());
    }
    if lines.is_empty() {
        lines.push(match outcome.and_then(|o| o.exit_code) {
            Some(c) => f("exit code {code}", &[("code", &c.to_string())]),
            None => s("No error was reported.").to_string(),
        });
    }
    Some(lines.iter().map(|l| clip(l)).collect::<Vec<_>>().join("\n"))
}

/// The `bashcut.plugins.*` command and params that run a finished, unsuccessful job again: the
/// same plugin, action and params; the same install; a new registry check.
pub fn retry(j: &Job) -> Option<(&'static str, Value)> {
    if !matches!(j.state, JobState::Failed | JobState::Crashed | JobState::TimedOut | JobState::Cancelled) {
        return None;
    }
    Some(match &j.kind {
        JobKind::Run { plugin, action } => ("run", json!({"plugin": plugin, "action": action, "params": j.params})),
        JobKind::Install { plugin } => ("install", json!({"plugin": plugin})),
        JobKind::Registry => ("registry", json!({})),
    })
}

/// The files a finished run's result names under [`RESULT_PATH_KEYS`], in that order, without
/// repeats, at most [`MAX_REVEAL`].
pub fn result_paths(j: &Job) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if j.state.is_running() {
        return out;
    }
    let Some(data) = j.outcome.as_ref().and_then(|o| o.result.as_ref()).map(|r| &r.data) else { return out };
    let as_path = |v: &Value| v.as_str().or_else(|| v.get("path").and_then(Value::as_str)).map(str::trim).filter(|p| !p.is_empty()).map(str::to_string);
    for k in RESULT_PATH_KEYS {
        let found: Vec<String> = match data.get(k) {
            Some(Value::Array(a)) => a.iter().take(MAX_REVEAL).filter_map(as_path).collect(),
            Some(v) => as_path(v).into_iter().collect(),
            None => Vec::new(),
        };
        for p in found {
            if out.len() < MAX_REVEAL && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// What a row's buttons asked for (run after the row is drawn, with the app free again).
enum Act {
    Command(&'static str, Value),
    Reveal(String),
    ToggleLog,
    SendToAgent,
}

pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let t = app.tokens;
    let mut v = super::shared::temp::<View>(&ctx, key());
    let jobs: Vec<Job> = plugins::with(app, &ctx, |h| h.jobs().iter().rev().cloned().collect());
    let call = |app: &mut FilmcraftApp, v: &mut View, name: &str, params: serde_json::Value| match plugins::invoke(app, &ctx, name, &params) {
        Ok(_) => v.status = None,
        Err(e) => v.status = Some(e),
    };

    super::toast::status_line(app, ui, "bashcut.runs.status", v.status.as_deref(), t.danger);
    let batch_row = batch_row(app, ui);
    if jobs.is_empty() {
        if !batch_row {
            note(ui, s("Nothing has run yet."), NOTE, t.text_dim);
            note(ui, s("Run a plugin action from Plugins and it shows up here."), NOTE, t.text_dim);
        }
        if button(app, ui, "bashcut.runs.openPlugins", s("Open Plugins"), true) {
            super::State::update(&ctx, |st| st.show("plugins"));
        }
    } else if button(app, ui, "bashcut.runs.clear", s("Clear finished"), jobs.iter().any(|j| !j.state.is_running())) {
        call(app, &mut v, "clear", json!({}));
    }

    for j in &jobs {
        ui.add_space(4.0);
        let focus = v.focus == Some(j.id);
        for act in job_row(app, ui, j, v.logs.contains(&j.id), focus) {
            match act {
                Act::Command(name, params) => call(app, &mut v, name, params),
                Act::Reveal(path) => {
                    v.status = crate::panels::menu_dialogs::open_path(app, &ctx, &path, true).err();
                }
                Act::ToggleLog => {
                    if !v.logs.remove(&j.id) {
                        v.logs.insert(j.id);
                    }
                }
                Act::SendToAgent => {
                    call(app, &mut v, "sendToAgent", json!({"job": j.id}));
                    super::State::update(&ctx, |st| st.show("agent"));
                }
            }
        }
    }
    v.logs.retain(|id| jobs.iter().any(|j| j.id == *id));
    // scrolled to once (or gone from the list)
    v.focus = None;
    super::shared::put_temp(&ctx, key(), v);
}

/// "Batch: n of m · Open Batch" while a batch runs (its rows live in the Batch section); returns
/// whether it drew.
fn batch_row(app: &mut FilmcraftApp, ui: &mut egui::Ui) -> bool {
    let shared = batch::shared(ui.ctx());
    let counts = {
        let g = batch::lock(&shared);
        g.is_running().then(|| g.book.as_ref().map(|b| (b.items.iter().filter(|i| i.status.is_final()).count(), b.items.len())))
    };
    let Some(counts) = counts else { return false };
    let text = match counts {
        Some((done, total)) => f("Batch: {done} of {total}", &[("done", &done.to_string()), ("total", &total.to_string())]),
        None => s("Batch: starting").to_string(),
    };
    let t = app.tokens;
    let frame = egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&text).color(t.text));
            if button(app, ui, "bashcut.runs.batch.open", s("Open Batch"), true) {
                super::State::update(ui.ctx(), |st| st.show("batch"));
            }
        });
    });
    app.auto.add("bashcut.runs.batch", frame.response.rect, &text);
    true
}

/// One job's row; returns what its buttons asked for.
fn job_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, j: &Job, log_open: bool, focus: bool) -> Vec<Act> {
    let t = app.tokens;
    let id = format!("bashcut.runs.{}", j.id);
    let error = error_text(j);
    let paths = result_paths(j);
    let mut acts = Vec::new();
    let mut group = egui::Frame::group(ui.style());
    if focus {
        group = group.stroke(egui::Stroke::new(1.5, t.accent));
    }
    let frame = group.show(ui, |ui| {
        // State first from the right, so a long title truncates instead of running under it.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(s(state_label(j.state))).size(11.0).color(state_color(j.state, &t)));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&j.title).color(t.text)).truncate()).on_hover_text(&j.title);
                });
            });
        });
        if j.state.is_running() {
            let bar = match j.progress {
                Some(p) => egui::ProgressBar::new(p.clamp(0.0, 1.0)).show_percentage(),
                None => egui::ProgressBar::new(0.0).animate(true),
            };
            let r = ui.add(bar.desired_height(6.0));
            app.auto.add_response(&format!("{id}.progress"), &r, &j.message);
        }
        match &error {
            Some(e) => {
                let r = ui.add(egui::Label::new(egui::RichText::new(e).size(11.0).color(t.danger)).wrap().selectable(true));
                app.auto.add_response(&format!("{id}.error"), &r, e);
            }
            None if !j.message.is_empty() => {
                ui.add(egui::Label::new(egui::RichText::new(&j.message).size(11.0).color(t.text_dim)).wrap().selectable(true));
            }
            None => {}
        }
        // elapsed_ms ticks while it runs (`Host::poll`), so a long run shows its time too
        let secs = format!("{:.1}", j.elapsed_ms as f64 / 1000.0);
        note(ui, f("{seconds} s", &[("seconds", &secs)]), META, t.text_dim);
        ui.horizontal_wrapped(|ui| {
            if j.state.is_running() && button(app, ui, &format!("{id}.cancel"), s("Cancel"), true) {
                acts.push(Act::Command("cancel", json!({"job": j.id})));
            }
            if let Some((name, params)) = retry(j)
                && button(app, ui, &format!("{id}.retry"), s("Retry"), true)
            {
                acts.push(Act::Command(name, params));
            }
            if button(app, ui, &format!("{id}.log"), if log_open { s("Hide log") } else { s("Log") }, !j.log.is_empty()) {
                acts.push(Act::ToggleLog);
            }
            if button(app, ui, &format!("{id}.copyLog"), s("Copy log"), !j.log.is_empty() || error.is_some()) {
                let mut text: Vec<&str> = j.log.iter().map(String::as_str).collect();
                if let Some(e) = &error {
                    text.push(e);
                }
                ui.ctx().copy_text(text.join("\n"));
            }
            if j.state == JobState::HandOff && button(app, ui, &format!("{id}.agent"), s("Send to agent"), true) {
                acts.push(Act::SendToAgent);
            }
        });
        for (k, p) in paths.iter().enumerate() {
            ui.horizontal(|ui| {
                if button(app, ui, &format!("{id}.reveal.{k}"), s("Reveal"), true) {
                    acts.push(Act::Reveal(p.clone()));
                }
                let name = std::path::Path::new(p).file_name().map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned());
                ui.add(egui::Label::new(egui::RichText::new(name).size(10.5).color(t.text_dim)).truncate()).on_hover_text(p);
            });
        }
        if log_open {
            let text: String = j.log.iter().map(String::as_str).collect::<Vec<_>>().join("\n");
            egui::ScrollArea::vertical().id_salt(("bashcut.runs.log", j.id)).max_height(160.0).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(text).monospace().size(10.5).color(t.text_dim)).selectable(true));
            });
        }
    });
    if focus {
        ui.scroll_to_rect(frame.response.rect, Some(egui::Align::Center));
    }
    app.auto.add(&id, frame.response.rect, &j.title);
    acts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ThemeKind, Tokens};
    use bashcut_plugin_host::runner::{End, Outcome, PluginResult, ResultError};

    #[test]
    fn run_states_use_the_theme_colours() {
        for kind in [ThemeKind::Dark, ThemeKind::Light] {
            let t = Tokens::for_kind(kind);
            for st in [JobState::Failed, JobState::Crashed, JobState::TimedOut] {
                assert_eq!(state_color(st, &t), t.danger);
            }
            assert_eq!(state_color(JobState::Succeeded, &t), t.accent);
            assert_eq!(state_color(JobState::HandOff, &t), t.accent);
            assert_eq!(state_color(JobState::Cancelled, &t), t.text_dim);
            assert_eq!(state_color(JobState::Running, &t), t.text_dim);
        }
    }

    fn finished(state: JobState, result: Option<PluginResult>, error: Option<&str>) -> Job {
        let mut j = Job::new(7, JobKind::Run { plugin: "test.fake".into(), action: "speak".into() }, "Fake › Speak".into());
        j.state = state;
        j.params.insert("text".into(), json!("xin chào"));
        let end = if state == JobState::Succeeded { End::Succeeded } else { End::Failed };
        let o = Outcome { end, exit_code: Some(1), result, stdout_truncated: false, error: error.map(str::to_string) };
        j.message = o.summary();
        j.outcome = Some(o);
        j
    }

    fn failed_with_code() -> Job {
        let e = ResultError { code: "no_key".into(), message: "set the API key in the plugin's settings".into() };
        finished(JobState::Failed, Some(PluginResult { ok: false, error: Some(e), ..PluginResult::default() }), None)
    }

    #[test]
    fn a_failed_run_has_its_full_error_and_a_retry_of_the_same_run() {
        let j = failed_with_code();
        assert_eq!(error_text(&j).as_deref(), Some("no_key: set the API key in the plugin's settings"));
        let (name, params) = retry(&j).unwrap();
        assert_eq!(name, "run");
        assert_eq!(params, json!({"plugin": "test.fake", "action": "speak", "params": {"text": "xin chào"}}));

        // no result: the host's error with the plugin's last line, then nothing repeated
        let mut j = finished(JobState::Failed, None, Some("exit status 2"));
        j.message = "exit status 2: usage: plugin.py [--word W]".into();
        assert_eq!(error_text(&j).as_deref(), Some("exit status 2: usage: plugin.py [--word W]"));
        let j = finished(JobState::Crashed, None, None);
        assert_eq!(error_text(&j).as_deref(), Some("exit code 1"));

        // an install or registry check retries as itself; a good or running job has nothing to retry
        let mut j = Job::new(8, JobKind::Install { plugin: "a.b".into() }, "Install".into());
        assert!(retry(&j).is_none() && error_text(&j).is_none());
        j.state = JobState::Failed;
        assert_eq!(retry(&j), Some(("install", json!({"plugin": "a.b"}))));
        let ok = finished(JobState::Succeeded, Some(PluginResult { ok: true, ..PluginResult::default() }), None);
        assert!(retry(&ok).is_none() && error_text(&ok).is_none());
    }

    #[test]
    fn result_paths_come_from_the_documented_keys() {
        let data = json!({"path": "/out/a.wav", "paths": ["/out/b.wav", "/out/a.wav", 3], "files": [{"path": "/out/c.srt"}], "outputs": "  ", "other": "/x"});
        let j = finished(JobState::Succeeded, Some(PluginResult { ok: true, data, ..PluginResult::default() }), None);
        assert_eq!(result_paths(&j), vec!["/out/a.wav", "/out/b.wav", "/out/c.srt"]);
        let many: Vec<String> = (0..50).map(|i| format!("/o/{i}")).collect();
        let j = finished(JobState::Succeeded, Some(PluginResult { ok: true, data: json!({"files": many}), ..PluginResult::default() }), None);
        assert_eq!(result_paths(&j).len(), MAX_REVEAL);
        assert!(result_paths(&failed_with_code()).is_empty());
    }

    #[test]
    fn a_fake_failed_job_renders_its_error_and_a_retry_id() {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let j = failed_with_code();
        let done = finished(JobState::Succeeded, Some(PluginResult { ok: true, data: json!({"path": "/out/a.wav"}), ..PluginResult::default() }), None);
        let ctx = egui::Context::default();
        let mut done = done;
        done.id = 9;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(job_row(&mut app, ui, &j, false, false).is_empty());
            assert!(job_row(&mut app, ui, &done, false, false).is_empty());
        });
        // no renderer here: drop the font texture on purpose (egui asserts it was handled)
        out.textures_delta.clear();
        let error = app.auto.find("bashcut.runs.7.error").expect("the error is drawn");
        assert!(error.label.contains("no_key: set the API key"), "{}", error.label);
        for id in ["bashcut.runs.7.retry", "bashcut.runs.7.copyLog", "bashcut.runs.9.reveal.0"] {
            assert!(app.auto.find(id).is_some(), "{id}");
        }
        for id in ["bashcut.runs.9.retry", "bashcut.runs.9.error", "bashcut.runs.7.reveal.0", "bashcut.runs.7.cancel"] {
            assert!(app.auto.find(id).is_none(), "{id}");
        }
    }
}
