//! The Batch section (`bashcut/in-app-ui.md` §7): build a batch (inputs, steps, outputs), run it
//! headless, watch every item and fix the ones that failed. Every button goes through the
//! `bashcut.batch.*` commands ([`batch::invoke`]) and registers an automation id.

use std::collections::BTreeMap;

use bashcut_plugin_host::install::Installed;
use bashcut_plugin_host::manifest::Action;
use serde_json::{Map, Value, json};

use super::batch;
use super::palette::{self, Item};
use super::plugins_panel::{Spec, button, field, param_title, param_value};
use super::strings::{f, s};
use crate::FilmcraftApp;
use crate::native_dialogs::FileDialog;
use bashcut_batch::book::now_ms;
use bashcut_batch::pipeline::OnFailure;
use bashcut_batch::{Book, ItemState, Status};

/// Matches shown under the step search.
const MATCHES: usize = 6;

/// What the forms hold between frames (egui context data, not `UiState`).
#[derive(Clone, Default)]
struct View {
    search: String,
    /// The step whose parameters are open, with the JSON being edited.
    editing: Option<(usize, String)>,
    /// The open plugin step's form: its index and each param's text.
    form: Option<(usize, BTreeMap<String, String>)>,
    preset: String,
    /// The last command's error.
    status: Option<String>,
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.batch.view")
}

fn call(app: &mut FilmcraftApp, ctx: &egui::Context, v: &mut View, name: &str, params: Value) -> Option<Value> {
    match batch::invoke(app, ctx, name, &params) {
        Ok(r) => {
            v.status = None;
            Some(r)
        }
        Err(e) => {
            v.status = Some(e);
            None
        }
    }
}

fn status_label(st: Status) -> &'static str {
    match st {
        Status::Pending => "Waiting",
        Status::Running => "Running",
        Status::Done => "Done",
        Status::Failed => "Failed",
        Status::Cancelled => "Cancelled",
        Status::Tripped => "Interrupted",
        Status::NeedsAttention => "Needs attention",
        Status::NotRun => "Not run",
        Status::Stopped => "Stopped",
        Status::NeedsFix => "Needs fix",
    }
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn heading(app: &FilmcraftApp, ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(s(text)).strong().color(app.tokens.text));
}

pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut v = ctx.data_mut(|d| d.get_temp_mut_or_default::<View>(key()).clone());
    let shared = batch::shared(&ctx);
    let (draft, book, running, error, memory, recent) = {
        let g = batch::lock(&shared);
        (g.draft.clone(), g.book.clone(), g.is_running(), g.error.clone(), g.memory.clone(), g.memory.recent.clone())
    };
    let err = app.tokens.danger;
    for (id, e) in [("bashcut.batch.status", v.status.clone()), ("bashcut.batch.error", error)] {
        if let Some(e) = e {
            let r = ui.label(egui::RichText::new(&e).size(11.0).color(err));
            app.auto.add_response(id, &r, &e);
        }
    }

    inputs(app, ui, &ctx, &mut v, &shared, &draft.inputs);
    steps(app, ui, &ctx, &mut v, &draft.steps);
    outputs(app, ui, &ctx, &mut v, &draft.outputs);
    settings(app, ui, &ctx, &mut v, &draft);

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if button(app, ui, "bashcut.batch.run", s("Run batch"), !running && !draft.inputs.is_empty()) {
            call(app, &ctx, &mut v, "run", json!({}));
        }
        if button(app, ui, "bashcut.batch.cancel", s("Cancel"), running) {
            call(app, &ctx, &mut v, "cancel", json!({}));
        }
        if !running && draft.inputs.is_empty() {
            ui.label(egui::RichText::new(s("Add files to run the batch.")).size(10.5).color(app.tokens.text_dim));
        }
    });
    if let Some(book) = &book {
        queue(app, ui, &ctx, &mut v, book, &memory, running);
    }
    presets(app, ui, &ctx, &mut v);
    if !recent.is_empty() {
        heading(app, ui, "Recent batches");
        for (i, r) in recent.iter().enumerate().take(8) {
            let label = format!("{} · {}", if r.name.is_empty() { "batch" } else { r.name.as_str() }, file_name(&r.job));
            if button(app, ui, &format!("bashcut.batch.recent.{i}"), &label, !running) {
                call(app, &ctx, &mut v, "openJob", json!({"job": r.job}));
            }
        }
    }
    ctx.data_mut(|d| d.insert_temp(key(), v));
}

fn inputs(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, shared: &batch::Shared, list: &[String]) {
    let t = app.tokens;
    heading(app, ui, "Inputs");
    let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
    let frame = egui::Frame::group(ui.style()).stroke(egui::Stroke::new(1.0, if hovering { t.accent } else { t.text_dim })).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if list.is_empty() {
            ui.label(egui::RichText::new(s("Drop files or folders here, or add them below.")).size(11.0).color(t.text_dim));
        }
        for (i, path) in list.iter().enumerate() {
            ui.horizontal(|ui| {
                if button(app, ui, &format!("bashcut.batch.input.{i}.remove"), "✕", true) {
                    call(app, ctx, v, "removeInput", json!({"index": i}));
                }
                let r = ui.label(egui::RichText::new(file_name(path)).color(t.text)).on_hover_text(path);
                app.auto.add_response(&format!("bashcut.batch.input.{i}"), &r, path);
            });
        }
    });
    batch::lock(shared).drop_zone = Some(frame.response.rect);
    app.auto.add("bashcut.batch.drop", frame.response.rect, "drop files");
    ui.horizontal(|ui| {
        if button(app, ui, "bashcut.batch.addFiles", s("Add files…"), true) {
            let b = shared.clone();
            app.pick_ui(FileDialog::OpenFiles { exts: Vec::new() }, move |_, paths| {
                batch::lock(&b).add_inputs(paths);
            });
        }
        if button(app, ui, "bashcut.batch.addFolder", s("Add folder…"), true) {
            let b = shared.clone();
            app.pick_ui(FileDialog::Folder { at: None }, move |_, paths| {
                batch::lock(&b).add_inputs(paths);
            });
        }
    });
}

/// What the step search offers: engine commands, plugin actions and recipes (UI-only commands
/// cannot run in a batch).
fn step_items(app: &mut FilmcraftApp, ctx: &egui::Context) -> Vec<Item> {
    let mut items: Vec<Item> = palette::command_items(&app.session).into_iter().filter(|i| filmcraft_engine::commands::find(&i.command).is_some()).collect();
    items.extend(super::plugins::with(app, ctx, |h| palette::plugin_items(h.installed())));
    items.extend(palette::recipe_items());
    items
}

/// `addStep` params for a search match.
fn step_params(item: &Item) -> Option<Value> {
    if item.command == "bashcut.palette.recipe" {
        return Some(json!({"recipe": item.params.get("recipe")?}));
    }
    let params = if item.command == "bashcut.plugins.run" { item.params.clone() } else { palette::template(item) };
    let mut step = batch::prefill_params(&item.command, &params)?;
    // the palette's blanks are for a person to fill; a batch step leaves them to the command's defaults
    if let Some(p) = step.get_mut("params").and_then(Value::as_object_mut) {
        p.retain(|_, v| !v.is_null());
    }
    // the list shows "Add Marker", not "markers.add"
    if let Some(o) = step.as_object_mut() {
        o.entry("label").or_insert_with(|| json!(item.label));
    }
    Some(step)
}

fn steps(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, list: &[bashcut_batch::pipeline::Step]) {
    let t = app.tokens;
    heading(app, ui, "Steps");
    ui.label(egui::RichText::new(s("Each input gets its own project; then these run on it in order.")).size(10.5).color(t.text_dim));
    let n = list.len();
    for (i, step) in list.iter().enumerate() {
        let label = step.label.clone().unwrap_or_else(|| step.describe());
        ui.horizontal(|ui| {
            let open = v.editing.as_ref().is_some_and(|(e, _)| *e == i);
            if button(app, ui, &format!("bashcut.batch.step.{i}"), &format!("{}. {label}", i + 1), true) {
                v.editing = if open { None } else { Some((i, serde_json::to_string_pretty(step).unwrap_or_default())) };
                v.form = None;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(app, ui, &format!("bashcut.batch.step.{i}.remove"), "✕", true) {
                    call(app, ctx, v, "removeStep", json!({"index": i}));
                    v.editing = None;
                }
                if button(app, ui, &format!("bashcut.batch.step.{i}.down"), "↓", i + 1 < n) {
                    call(app, ctx, v, "moveStep", json!({"index": i, "to": i + 1}));
                    v.editing = None;
                }
                if button(app, ui, &format!("bashcut.batch.step.{i}.up"), "↑", i > 0) {
                    call(app, ctx, v, "moveStep", json!({"index": i, "to": i - 1}));
                    v.editing = None;
                }
            });
        });
        let Some((at, text)) = v.editing.clone().filter(|(e, _)| *e == i) else { continue };
        // a plugin step is edited as its action's form, never as its command line
        let argv = step.plugin.as_ref().map(|p| p.argv.clone()).unwrap_or_default();
        if let Some((plugin, a)) = super::plugins::with(app, ctx, |h| plugin_action(h.installed(), &argv)) {
            plugin_form(app, ui, ctx, v, i, step, &plugin, &a, &argv);
            continue;
        }
        // an engine command shows what it takes, as the palette does
        if let Some(sig) = step.command.as_deref().and_then(filmcraft_engine::commands::find).map(|c| c.params).filter(|p| !p.is_empty()) {
            let r = ui.label(egui::RichText::new(sig).monospace().size(10.5).color(t.text_dim));
            app.auto.add_response(&format!("bashcut.batch.step.{i}.signature"), &r, sig);
        }
        let mut text = text;
        let r = ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_rows(4).desired_width(f32::INFINITY));
        app.auto.add_response(&format!("bashcut.batch.step.{i}.json"), &r, "step JSON");
        v.editing = Some((at, text.clone()));
        // Apply waits for JSON that parses; hovering it says where it breaks
        let parsed = serde_json::from_str::<Value>(&text);
        let mut r = ui.add_enabled(parsed.is_ok(), egui::Button::new(s("Apply")));
        if let Err(e) = &parsed {
            r = r.on_disabled_hover_text(f("Step JSON: {error}", &[("error", &e.to_string())]));
        }
        app.auto.add_response(&format!("bashcut.batch.step.{i}.apply"), &r, s("Apply"));
        if r.clicked()
            && let Ok(step) = parsed
            && call(app, ctx, v, "setStep", json!({"index": i, "step": step})).is_some()
        {
            v.editing = None;
        }
    }
    let r = ui.add(egui::TextEdit::singleline(&mut v.search).hint_text(s("＋ Add step: search commands, plugins, recipes")).desired_width(f32::INFINITY));
    app.auto.add_response("bashcut.batch.step.search", &r, "add step");
    if v.search.trim().is_empty() {
        return;
    }
    let items = step_items(app, ctx);
    let ranked = palette::rank(&items, &v.search, &[], MATCHES);
    for (n, i) in ranked.into_iter().enumerate() {
        let Some(item) = items.get(i) else { continue };
        let label = format!("{} · {}", s(item.kind.label()), item.label);
        if button(app, ui, &format!("bashcut.batch.step.add.{n}"), &label, true)
            && let Some(params) = step_params(item)
            && call(app, ctx, v, "addStep", params).is_some()
        {
            v.search.clear();
        }
    }
}

/// The installed plugin action a plugin step was built from (`batch.rs` `plugin_step`): the one
/// whose program and args start its command line (program files made absolute), longest first.
fn plugin_action(installed: &[Installed], argv: &[String]) -> Option<(String, Action)> {
    let os = bashcut_plugin_host::os();
    let mut best: Option<(usize, String, Action)> = None;
    for p in installed {
        let Some(m) = &p.manifest else { continue };
        let entry = m.entry_for(os);
        if entry.is_empty() {
            continue;
        }
        for a in &m.actions {
            let head: Vec<&String> = entry.iter().chain(&a.args).collect();
            let fits = head.len() <= argv.len() && head.iter().zip(argv).all(|(want, got)| *want == got || p.dir.join(want).to_string_lossy() == got.as_str());
            if fits && best.as_ref().is_none_or(|(n, ..)| head.len() > *n) {
                best = Some((head.len(), p.id.clone(), a.clone()));
            }
        }
    }
    best.map(|(_, id, a)| (id, a))
}

/// Each param's text read back from its flag in the command line (a bool is its flag's presence;
/// one left out is empty, for the plugin's default).
fn flag_texts(a: &Action, argv: &[String]) -> BTreeMap<String, String> {
    a.params
        .iter()
        .map(|p| {
            let at = argv.iter().position(|x| !p.flag.is_empty() && *x == p.flag);
            let text = if p.kind == "bool" { at.is_some().to_string() } else { at.and_then(|k| argv.get(k.saturating_add(1))).cloned().unwrap_or_default() };
            (p.id.clone(), text)
        })
        .collect()
}

/// A plugin step's params as the Plugins section's form ([`field`], labels in the UI language);
/// **Apply** rebuilds the step through `setStep`, keeping its label.
#[allow(clippy::too_many_arguments)]
fn plugin_form(
    app: &mut FilmcraftApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    v: &mut View,
    i: usize,
    step: &bashcut_batch::pipeline::Step,
    plugin: &str,
    a: &Action,
    argv: &[String],
) {
    let mut texts = match v.form.take() {
        Some((at, texts)) if at == i => texts,
        _ => flag_texts(a, argv),
    };
    let mut apply = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        for p in &a.params {
            let title = param_title(p);
            let id = format!("bashcut.batch.step.{i}.param.{}", p.id);
            let text = texts.entry(p.id.clone()).or_default();
            ui.horizontal(|ui| {
                ui.label(title.as_str());
                field(app, ui, &id, &title, Spec::param(p), text);
            });
        }
        apply = button(app, ui, &format!("bashcut.batch.step.{i}.apply"), s("Apply"), true);
    });
    if apply {
        let params: Map<String, Value> = a.params.iter().filter_map(|p| Some((p.id.clone(), param_value(p, texts.get(&p.id)?)?))).collect();
        let mut req = Map::new();
        req.insert("index".into(), json!(i));
        req.insert("plugin".into(), json!(plugin));
        req.insert("action".into(), json!(a.id));
        req.insert("params".into(), Value::Object(params));
        if let Some(label) = &step.label {
            req.insert("label".into(), json!(label));
        }
        if call(app, ctx, v, "setStep", Value::Object(req)).is_some() {
            v.editing = None;
            return;
        }
    }
    v.form = Some((i, texts));
}

fn outputs(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, list: &[bashcut_batch::pipeline::Output]) {
    let t = app.tokens;
    heading(app, ui, "Outputs");
    if list.is_empty() {
        ui.label(egui::RichText::new(s("No exports: the projects are the result.")).size(10.5).color(t.text_dim));
    }
    for (i, o) in list.iter().enumerate() {
        ui.horizontal(|ui| {
            if button(app, ui, &format!("bashcut.batch.output.{i}.remove"), "✕", true) {
                call(app, ctx, v, "removeOutput", json!({"index": i}));
            }
            let what = o.preset.clone().or_else(|| o.format.clone()).unwrap_or_default();
            ui.label(egui::RichText::new(format!("{} · {what}", o.name)).color(t.text));
        });
    }
    let r = ui.menu_button(s("＋ Add output"), |ui| {
        let entries = |app: &mut FilmcraftApp, cmd: &str, array: &str, field: &str| -> Vec<(String, String)> {
            let v = app.session.execute(cmd, json!({})).unwrap_or(Value::Null);
            v.get(array)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|e| {
                    let id = e.get(field).and_then(Value::as_str)?.to_string();
                    let name = e.get("name").and_then(Value::as_str).unwrap_or(id.as_str()).to_string();
                    Some((id, name))
                })
                .collect()
        };
        ui.label(egui::RichText::new(s("Output profiles")).size(10.5).color(t.text_dim));
        for (id, name) in entries(app, "bashcut.profiles.list", "profiles", "id") {
            if button(app, ui, &format!("bashcut.batch.output.profile.{id}"), &name, true) {
                call(app, ctx, v, "addOutput", json!({"profile": id}));
                ui.close();
            }
        }
        ui.separator();
        ui.label(egui::RichText::new(s("Export presets")).size(10.5).color(t.text_dim));
        for (id, name) in entries(app, "export.presets.list", "presets", "name") {
            if button(app, ui, &format!("bashcut.batch.output.preset.{id}"), &name, true) {
                call(app, ctx, v, "addOutput", json!({"preset": id}));
                ui.close();
            }
        }
    });
    app.auto.add_response("bashcut.batch.output.add", &r.response, "add output");
}

fn settings(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, draft: &bashcut_batch::Pipeline) {
    let t = app.tokens;
    heading(app, ui, "Settings");
    ui.horizontal(|ui| {
        ui.label(s("Name"));
        let mut name = draft.name.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text("batch").desired_width(160.0));
        app.auto.add_response("bashcut.batch.name", &r, "name");
        if r.changed() {
            call(app, ctx, v, "set", json!({"name": name}));
        }
    });
    ui.horizontal(|ui| {
        ui.label(s("At once"));
        let mut n = draft.concurrency;
        let r = ui.add(egui::DragValue::new(&mut n).range(1..=bashcut_batch::pipeline::MAX_CONCURRENCY));
        app.auto.add_response("bashcut.batch.concurrency", &r, "concurrency");
        if r.changed() {
            call(app, ctx, v, "set", json!({"concurrency": n}));
        }
    });
    ui.horizontal(|ui| {
        ui.label(s("When an item fails"));
        let skip = draft.on_failure == OnFailure::Skip;
        let r = ui.radio(skip, s("go on"));
        app.auto.add_response("bashcut.batch.onFailure.skip", &r, "go on");
        if r.clicked() {
            call(app, ctx, v, "set", json!({"onFailure": "skip"}));
        }
        let r = ui.radio(!skip, s("stop"));
        app.auto.add_response("bashcut.batch.onFailure.stop", &r, "stop");
        if r.clicked() {
            call(app, ctx, v, "set", json!({"onFailure": "stop"}));
        }
    });
    ui.horizontal(|ui| {
        let shown = if draft.projects_folder.is_empty() { s("next to the first input, in batch/").to_string() } else { draft.projects_folder.clone() };
        let r = ui.label(egui::RichText::new(f("Projects: {folder}", &[("folder", &shown)])).size(10.5).color(t.text_dim));
        app.auto.add_response("bashcut.batch.projects", &r, &shown);
        if button(app, ui, "bashcut.batch.projects.choose", s("Choose…"), true) {
            let b = batch::shared(ctx);
            app.pick_ui(FileDialog::Folder { at: None }, move |_, paths| {
                if let Some(p) = paths.into_iter().next() {
                    batch::lock(&b).draft.projects_folder = p;
                }
            });
        }
    });
}

fn queue(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, book: &Book, memory: &batch::Memory, running: bool) {
    let t = app.tokens;
    heading(app, ui, "Queue");
    let now = now_ms();
    let c = book.counts();
    let total = book.items.len().max(1) as f32;
    let overall = book.items.iter().map(|i| batch::item_progress(i, memory, now)).sum::<f32>() / total;
    let summary = f(
        "{done} done · {failed} failed · {attention} need attention · {left} left",
        &[
            ("done", &c.done.to_string()),
            ("failed", &c.failed.to_string()),
            ("attention", &c.needs_attention.to_string()),
            ("left", &(c.pending + c.running).to_string()),
        ],
    );
    let r = ui.add(egui::ProgressBar::new(overall).desired_height(8.0).animate(running && book.status == Status::Running));
    app.auto.add_response("bashcut.batch.progress", &r, &summary);
    ui.label(egui::RichText::new(format!("{} · {summary}", s(status_label(book.status)))).size(10.5).color(t.text_dim));
    // the runner waits (low disk) without failing anything: say so, or the queue looks stuck
    if let Some(why) = book.paused.as_ref().filter(|_| running) {
        let r = ui.label(egui::RichText::new(f("Waiting: {why}", &[("why", why)])).size(10.5).color(t.text_dim));
        app.auto.add_response("bashcut.batch.paused", &r, why);
    }
    ui.horizontal(|ui| {
        let failed =
            book.items.iter().any(|i| matches!(i.status, Status::Failed | Status::NeedsAttention | Status::NeedsFix | Status::Cancelled | Status::NotRun));
        if button(app, ui, "bashcut.batch.retryAll", s("Retry failed"), !running && failed) {
            call(app, ctx, v, "retry", json!({}));
        }
        if button(app, ui, "bashcut.batch.reveal", s("Reveal folder"), true) {
            call(app, ctx, v, "reveal", json!({}));
        }
    });
    for item in &book.items {
        item_row(app, ui, ctx, v, item, memory, now, running);
    }
}

#[allow(clippy::too_many_arguments)]
fn item_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, item: &ItemState, memory: &batch::Memory, now: u64, running: bool) {
    let t = app.tokens;
    let id = format!("bashcut.batch.item.{}", item.name);
    ui.add_space(4.0);
    let frame = egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&item.name).color(t.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let color = match item.status {
                    Status::Done => t.accent,
                    Status::Failed | Status::NeedsAttention | Status::NeedsFix => t.danger,
                    _ => t.text_dim,
                };
                ui.label(egui::RichText::new(s(status_label(item.status))).size(11.0).color(color));
            });
        });
        if item.status == Status::Running {
            let current = item.steps.iter().find(|s| s.status == Status::Running).map(|s| s.label.as_str()).unwrap_or_default();
            let r = ui.add(egui::ProgressBar::new(batch::item_progress(item, memory, now)).desired_height(6.0));
            app.auto.add_response(&format!("{id}.progress"), &r, current);
            ui.label(egui::RichText::new(current).size(10.5).color(t.text_dim));
        }
        // a step marked done by hand keeps its old error in the book; the item is fine now
        let error = item.error.as_ref().or_else(|| item.steps.iter().rev().find_map(|s| s.error.as_ref())).filter(|_| item.status != Status::Done);
        if let Some(e) = error {
            let r = ui.label(egui::RichText::new(&e.message).size(10.5).color(t.danger));
            app.auto.add_response(&format!("{id}.error"), &r, &e.message);
        }
        for (n, out) in item.outputs().iter().enumerate() {
            if button(app, ui, &format!("{id}.output.{n}"), &file_name(out), true) {
                call(app, ctx, v, "reveal", json!({"path": out}));
            }
        }
        if item.status == Status::NeedsAttention {
            ui.label(egui::RichText::new(s("This step needs you: open the project, finish it by hand, then click Mark done.")).size(10.5).color(t.text_dim));
        }
        if item.status.is_final() {
            ui.horizontal(|ui| {
                let made = std::path::Path::new(&item.project).is_file();
                let open = if item.status == Status::Done { s("Open") } else { s("Open to fix") };
                if button(app, ui, &format!("{id}.open"), open, made) {
                    call(app, ctx, v, "open", json!({"item": item.name}));
                }
                if item.status == Status::NeedsAttention && button(app, ui, &format!("{id}.markDone"), s("Mark done"), !running) {
                    call(app, ctx, v, "markDone", json!({"items": [item.name]}));
                }
                if item.status != Status::Done && button(app, ui, &format!("{id}.retry"), s("Retry"), !running) {
                    call(app, ctx, v, "retry", json!({"items": [item.name]}));
                }
                if button(app, ui, &format!("{id}.reveal"), s("Reveal"), true) {
                    call(app, ctx, v, "reveal", json!({"item": item.name}));
                }
            });
        }
    });
    app.auto.add(&id, frame.response.rect, &item.name);
}

fn presets(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View) {
    let Some(dir) = super::data_dir(app) else { return };
    heading(app, ui, "Presets");
    let names = bashcut_batch::presets::list(&dir);
    for name in &names {
        ui.horizontal(|ui| {
            if button(app, ui, &format!("bashcut.batch.preset.{name}"), name, true) {
                call(app, ctx, v, "loadPreset", json!({"name": name}));
            }
            if button(app, ui, &format!("bashcut.batch.preset.{name}.delete"), "✕", true) {
                call(app, ctx, v, "deletePreset", json!({"name": name}));
            }
        });
    }
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut v.preset).hint_text(s("preset-name")).desired_width(160.0));
        app.auto.add_response("bashcut.batch.preset.name", &r, "preset name");
        let name = v.preset.trim().to_string();
        if button(app, ui, "bashcut.batch.preset.save", s("Save as preset"), !name.is_empty())
            && call(app, ctx, v, "savePreset", json!({"name": name})).is_some()
        {
            v.preset.clear();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::palette::Kind;
    use super::*;

    #[test]
    fn searched_steps_drop_the_blanks_and_keep_the_label() {
        let item = Item {
            key: "command:markers.add".into(),
            kind: Kind::Command,
            label: "Add Marker".into(),
            detail: String::new(),
            shortcut: None,
            enabled: true,
            command: "markers.add".into(),
            params: json!({"name": "Hook"}),
            signature: r#"{"target":"program"|"source","time":ticks?,"name":str?}"#.into(),
            needs_params: true,
        };
        let step = step_params(&item).unwrap();
        assert_eq!(step, json!({"command": "markers.add", "params": {"name": "Hook"}, "label": "Add Marker"}));
    }

    #[test]
    fn flags_read_back_as_form_texts() {
        let m = bashcut_plugin_host::manifest::Manifest::parse(
            &json!({"schema": "bashcut-plugin/1", "id": "t", "name": "T", "version": "0.1.0", "entry": {"command": ["t"]},
                "actions": [{"id": "go", "args": ["go"], "needs": "none", "params": [
                    {"id": "to", "flag": "--to", "type": "string"}, {"id": "on", "flag": "--on", "type": "bool", "optional": true},
                    {"id": "n", "flag": "--n", "type": "integer", "optional": true}]}]})
            .to_string(),
        )
        .unwrap();
        let installed = [Installed { id: "t".into(), dir: "/p".into(), manifest: Some(m), origin: None, error: None }];
        let argv: Vec<String> = ["t", "go", "--to", "vi", "--on"].map(String::from).into();
        let (id, a) = plugin_action(&installed, &argv).unwrap();
        assert_eq!((id.as_str(), a.id.as_str()), ("t", "go"));
        let texts = flag_texts(&a, &argv);
        assert_eq!((texts["to"].as_str(), texts["on"].as_str(), texts["n"].as_str()), ("vi", "true", ""));
        // a trailing flag with no value, or another program, reads nothing
        assert!(plugin_action(&installed, &["other".to_string()]).is_none());
        assert_eq!(flag_texts(&a, &["--to".to_string()])["to"], "");
    }
}
