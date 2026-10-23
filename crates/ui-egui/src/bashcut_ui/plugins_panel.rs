//! The Plugins section (`bashcut/in-app-ui.md` §2): installed plugins with their settings and
//! actions, and what the registry offers. Every button goes through the `bashcut.plugins.*`
//! commands ([`plugins::invoke`]) and registers an automation id.

use std::collections::{BTreeMap, BTreeSet};

use bashcut_plugin_host::install::Installed;
use bashcut_plugin_host::manifest::{self, Action, I18n, Manifest, Param, Setting};
use bashcut_plugin_host::registry::Entry;
use bashcut_plugin_host::{Job, JobKind, JobState};
use serde_json::{Map, Value, json};

use super::hints::{self, CONTROL_OFF, HOW_TO_TURN_ON};
use super::plugins;
use super::strings::{self, f, s};
use super::widgets::{META, NOTE, button, close_button, note, note_text};
use crate::FilmcraftApp;
use crate::native_dialogs::FileDialog;

/// What the forms hold between frames (egui context data, not `UiState`).
#[derive(Clone, Default)]
struct Drafts {
    /// `<plugin>/<setting>` → text being edited.
    settings: BTreeMap<String, String>,
    /// `<plugin>/<setting>` → secret being typed (cleared once stored).
    secrets: BTreeMap<String, String>,
    /// Which secrets are stored, per plugin (asked once: the OS store is a process call).
    stored: BTreeMap<String, BTreeMap<String, bool>>,
    /// `<plugin>/<setting>` → a save (`true`) or clear being written on a side thread (RC13).
    writing: BTreeMap<String, (bool, plugins::Pending)>,
    /// The action whose form is open: `(plugin, action)`.
    open: Option<(String, String)>,
    /// `<plugin>/<action>/<param>` → value being edited.
    params: BTreeMap<String, String>,
    /// The last command's outcome: `(ok, message)`.
    status: Option<(bool, String)>,
    /// Per plugin, the run whose form last opened its card (a new form opens it once).
    shown_view: BTreeMap<String, u64>,
    /// Per plugin, the last run its card shows (until dismissed).
    last: BTreeMap<String, Last>,
    /// Per plugin, the newest run seen in the host: a newer one, started anywhere (palette, agent,
    /// control channel), becomes the card's last run.
    seen: BTreeMap<String, u64>,
    /// The registry was checked on the first open (once a session, never repeated).
    auto_checked: bool,
}

/// A plugin's last run as its card shows it.
#[derive(Clone, Debug, PartialEq)]
enum Last {
    /// A run the host holds.
    Job(u64),
    /// A run that did not start, with the host's error.
    Error(String),
}

/// What the panel reads of a job each frame.
#[derive(Clone, Debug, PartialEq)]
struct Brief {
    state: JobState,
    progress: Option<f32>,
    message: String,
}

impl Brief {
    fn of(j: &Job) -> Self {
        Brief { state: j.state, progress: j.progress, message: j.message.clone() }
    }
}

/// What the host says this frame, read under one lock.
#[derive(Default)]
struct Snapshot {
    /// Every job, by id.
    jobs: BTreeMap<u64, Brief>,
    /// Per plugin, its newest run.
    newest_run: BTreeMap<String, u64>,
    /// Per plugin, its newest install or update.
    newest_install: BTreeMap<String, u64>,
    /// Plugins being installed or updated now.
    installing: BTreeSet<String>,
    /// The newest registry check.
    registry_check: Option<Brief>,
}

impl Snapshot {
    fn read(jobs: &[Job]) -> Self {
        let mut s = Snapshot::default();
        // jobs come oldest first: the last one seen per plugin is its newest
        for j in jobs {
            s.jobs.insert(j.id, Brief::of(j));
            match &j.kind {
                JobKind::Run { plugin, .. } => {
                    s.newest_run.insert(plugin.clone(), j.id);
                }
                JobKind::Install { plugin } => {
                    s.newest_install.insert(plugin.clone(), j.id);
                    if j.state.is_running() {
                        s.installing.insert(plugin.clone());
                    }
                }
                JobKind::Registry => s.registry_check = Some(Brief::of(j)),
            }
        }
        s
    }
}

/// The line a card shows for its last run.
#[derive(Clone, Debug, PartialEq)]
struct LastLine {
    text: String,
    failed: bool,
    /// The run is in Runs (its log, Retry, Send to agent): offer "Open in Runs".
    in_runs: bool,
}

/// What a card says about its last run (`None` once the host forgot it, e.g. after Clear).
/// Failures read through [`hints::friendly`].
fn last_line(last: &Last, job: Option<&Brief>) -> Option<LastLine> {
    let failed = |msg: &str| if msg.trim().is_empty() { s("Failed").to_string() } else { f("Failed: {error}", &[("error", &hints::friendly(msg))]) };
    let line = |text: String, failed: bool| Some(LastLine { text, failed, in_runs: true });
    match last {
        Last::Error(e) => Some(LastLine { text: failed(e), failed: true, in_runs: false }),
        Last::Job(_) => {
            let j = job?;
            match j.state {
                JobState::Running => match j.progress.filter(|p| p.is_finite()) {
                    Some(p) => line(f("Running {percent} %", &[("percent", &format!("{}", (p.clamp(0.0, 1.0) * 100.0).round() as u32))]), false),
                    None => line(s("Running…").to_string(), false),
                },
                JobState::Succeeded => line(s("Done").to_string(), false),
                JobState::HandOff => line(s("Needs the agent").to_string(), false),
                JobState::Cancelled => line(s("Cancelled").to_string(), false),
                JobState::Failed | JobState::TimedOut | JobState::Crashed => line(failed(&j.message), true),
            }
        }
    }
}

/// The registry line: "N plugins available" (`listed` = the ones not installed yet).
fn available_text(listed: usize, total: usize) -> String {
    match listed {
        0 if total > 0 => s("All plugins are installed").to_string(),
        1 => s("1 plugin available").to_string(),
        n => f("{n} plugins available", &[("n", &n.to_string())]),
    }
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.plugins.drafts")
}

fn drafts(ctx: &egui::Context) -> Drafts {
    super::shared::temp::<Drafts>(ctx, key())
}

/// Manifest prose as plain text: the panel draws no markdown, so code spans lose their backticks.
fn plain(text: &str) -> String {
    text.replace('`', "")
}

/// Run `bashcut.plugins.<name>` and keep its outcome for the status line (a known setup error as
/// its sentence, [`hints::friendly`]).
fn run(app: &mut FilmcraftApp, ctx: &egui::Context, d: &mut Drafts, name: &str, params: Value) -> Option<Value> {
    match plugins::invoke(app, ctx, name, &params) {
        Ok(v) => {
            d.status = None;
            Some(v)
        }
        Err(e) => {
            d.status = Some((false, hints::friendly(&e)));
            None
        }
    }
}

/// Run `bashcut.plugins.run` or `click` for `plugin`: the run, or why it did not start, becomes
/// the card's last run (not the panel's status line).
fn start_run(app: &mut FilmcraftApp, ctx: &egui::Context, d: &mut Drafts, plugin: &str, name: &str, params: Value) -> bool {
    match plugins::invoke(app, ctx, name, &params) {
        Ok(v) => {
            d.status = None;
            if let Some(job) = v.get("job").and_then(Value::as_u64) {
                d.last.insert(plugin.to_string(), Last::Job(job));
            }
            true
        }
        Err(e) => {
            d.last.insert(plugin.to_string(), Last::Error(e));
            false
        }
    }
}

pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut d = drafts(&ctx);
    let t = app.tokens;
    let (installed, registry, location, running, snap) = plugins::with(app, &ctx, |h| {
        (
            h.installed().to_vec(),
            h.registry().cloned(),
            format!("{}@{}", h.registry_location.0, h.registry_location.1),
            h.any_running(),
            Snapshot::read(h.jobs()),
        )
    });
    // a run started anywhere since the last look becomes its card's last run
    for (plugin, id) in &snap.newest_run {
        if d.seen.get(plugin).is_none_or(|seen| seen < id) {
            d.seen.insert(plugin.clone(), *id);
            d.last.insert(plugin.clone(), Last::Job(*id));
        }
    }
    let can_install = super::data_dir(app).is_some();
    // the first open with no registry checks it once, so the list shows without a click
    if !d.auto_checked && can_install {
        d.auto_checked = true;
        if registry.is_none() && snap.registry_check.is_none() {
            run(app, &ctx, &mut d, "registry", json!({}));
        }
    }

    // before any click: without the control channel, project actions cannot run
    if !hints::control_on() {
        let r = note(ui, s(CONTROL_OFF), NOTE, t.text_dim).on_hover_text(s(HOW_TO_TURN_ON));
        app.auto.add_response("bashcut.plugins.setup", &r, s(CONTROL_OFF));
        super::control::turn_on_button(app, ui, "bashcut.plugins.turnOn");
        ui.add_space(4.0);
    }
    let color = if d.status.as_ref().is_some_and(|(ok, _)| *ok) { t.text_dim } else { t.danger };
    super::toast::status_line(app, ui, "bashcut.plugins.status", d.status.as_ref().map(|(_, m)| m.as_str()), color);

    // installed
    ui.label(egui::RichText::new(s("Installed")).strong().color(t.text));
    if installed.is_empty() {
        note(ui, s("No plugins installed yet."), NOTE, t.text_dim);
    }
    for p in &installed {
        plugin_card(app, ui, &ctx, &mut d, &snap, p);
    }

    // registry
    ui.add_space(8.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(s("Available")).strong().color(t.text));
        let label = if registry.is_some() { s("Refresh") } else { s("Check registry") };
        let checking = snap.registry_check.as_ref().is_some_and(|j| j.state.is_running());
        if button(app, ui, "bashcut.plugins.registry.refresh", label, can_install && !checking) {
            run(app, &ctx, &mut d, "registry", json!({}));
        }
    });
    if !can_install {
        note(ui, s("No data folder in this session: plugins cannot be installed."), NOTE, t.text_dim);
    }
    let listed: Vec<&Entry> = registry.iter().flat_map(|r| &r.plugins).filter(|e| !installed.iter().any(|p| p.id == e.id)).collect();
    // what the registry check says; where the registry lives is in the tooltip
    let line = match &snap.registry_check {
        Some(j) if j.state.is_running() => Some((s("Checking the registry…").to_string(), false)),
        Some(j) if j.state != JobState::Succeeded => Some((f("Could not check {location}: {error}", &[("location", &location), ("error", &j.message)]), true)),
        _ => registry.as_ref().map(|r| (available_text(listed.len(), r.plugins.len()), false)),
    };
    if let Some((text, failed)) = line {
        let r =
            ui.label(note_text(&text, NOTE, if failed { t.danger } else { t.text_dim })).on_hover_text(f("Registry: {location}", &[("location", &location)]));
        app.auto.add_response("bashcut.plugins.registry.status", &r, &text);
    }
    for e in listed {
        registry_row(app, ui, &ctx, &mut d, &snap, e, can_install);
    }
    if running {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    super::shared::put_temp(&ctx, key(), d);
}

fn registry_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, snap: &Snapshot, e: &Entry, can_install: bool) {
    let t = app.tokens;
    let supported = e.supports(bashcut_plugin_host::os());
    // one install at a time: the button waits while the host installs this id
    let installing = snap.installing.contains(&e.id);
    let lang = strings::current();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{} {}", e.name_in(lang), e.version)).color(t.text));
        let label = if installing { s("Installing…") } else { s("Install") };
        if button(app, ui, &format!("bashcut.plugins.{}.install", e.id), label, can_install && supported && !installing) {
            run(app, ctx, d, "install", json!({"plugin": e.id}));
        }
        if !supported {
            note(ui, s("Not available on this system"), META, t.text_dim);
        }
    });
    if !e.summary_in(lang).is_empty() {
        note(ui, plain(e.summary_in(lang)), NOTE, t.text_dim);
    }
    // a failed install says why next to its button
    if let Some(j) = snap.newest_install.get(&e.id).and_then(|id| snap.jobs.get(id)).filter(|j| !j.state.is_running() && j.state != JobState::Succeeded) {
        let text = f("Failed: {error}", &[("error", &hints::friendly(&j.message))]);
        let r = note(ui, &text, NOTE, t.danger);
        app.auto.add_response(&format!("bashcut.plugins.{}.install.status", e.id), &r, &text);
    }
}

/// The card's last-run line: "Running 40 %", "Failed: …", "Done · Open in Runs", with ✕ to hide it.
fn last_run(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, snap: &Snapshot, plugin: &str) {
    let Some(last) = d.last.get(plugin) else { return };
    let job = match last {
        Last::Job(id) => snap.jobs.get(id),
        Last::Error(_) => None,
    };
    let Some(line) = last_line(last, job) else {
        // Runs was cleared: nothing left to show
        d.last.remove(plugin);
        return;
    };
    let t = app.tokens;
    let id = format!("bashcut.plugins.{plugin}.last");
    let text = note_text(&line.text, NOTE, if line.failed { t.danger } else { t.text_dim });
    // a failure can be a long sentence: it wraps on its own line, the links go below it
    if line.failed {
        let r = ui.add(egui::Label::new(text.clone()).wrap());
        app.auto.add_response(&id, &r, &line.text);
    }
    ui.horizontal(|ui| {
        if !line.failed {
            let r = ui.label(text);
            app.auto.add_response(&id, &r, &line.text);
        }
        if line.in_runs {
            if !line.failed {
                note(ui, "·", NOTE, t.text_dim);
            }
            let r = ui.link(egui::RichText::new(s("Open in Runs")).size(11.0));
            app.auto.add_response(&format!("{id}.runs"), &r, s("Open in Runs"));
            if r.clicked() {
                super::State::update(ctx, |st| st.show(super::RUNS));
            }
        }
        let running = job.is_some_and(|j| j.state.is_running());
        if !running && close_button(app, ui, &format!("{id}.dismiss"), s("Dismiss")) {
            d.last.remove(plugin);
        }
    });
}

fn plugin_card(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, snap: &Snapshot, p: &Installed) {
    let t = app.tokens;
    let Some(m) = &p.manifest else {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&p.id).color(t.text));
            if button(app, ui, &format!("bashcut.plugins.{}.remove", p.id), s("Remove"), true) {
                run(app, ctx, d, "remove", json!({"plugin": p.id}));
            }
        });
        let error = p.error.clone().unwrap_or_default();
        note(ui, error, NOTE, t.text_dim);
        return;
    };
    let (update, view) = plugins::with(app, ctx, |h| (h.update_for(&p.id), h.view(&p.id).map(|(job, v)| (job, v.clone()))));
    let lang = strings::current();
    let head = egui::CollapsingHeader::new(egui::RichText::new(format!("{} {}", m.name_in(lang), m.version)).color(t.text))
        .id_salt(("bashcut.plugins.card", &p.id))
        .open(view.as_ref().and_then(|(job, _)| (d.shown_view.insert(p.id.clone(), *job) != Some(*job)).then_some(true)))
        .show(ui, |ui| {
            last_run(app, ui, ctx, d, snap, &p.id);
            if !m.summary_in(lang).is_empty() {
                note(ui, plain(m.summary_in(lang)), NOTE, t.text_dim);
            }
            ui.horizontal(|ui| {
                let updating = snap.installing.contains(&p.id);
                let label = if updating {
                    s("Updating…").to_string()
                } else {
                    update.as_ref().map(|v| f("Update to {version}", &[("version", v)])).unwrap_or_default()
                };
                if (update.is_some() || updating) && button(app, ui, &format!("bashcut.plugins.{}.update", p.id), &label, !updating) {
                    run(app, ctx, d, "update", json!({"plugin": p.id}));
                }
                if button(app, ui, &format!("bashcut.plugins.{}.remove", p.id), s("Remove"), true) {
                    run(app, ctx, d, "remove", json!({"plugin": p.id}));
                }
            });
            if !m.settings.is_empty() {
                settings_form(app, ui, ctx, d, m);
            }
            for a in &m.actions {
                action_row(app, ui, ctx, d, m, a);
            }
            // the newest form a `view` action showed; its buttons run the actions they name
            if let Some((job, v)) = &view
                && let Some(c) = super::view::draw(app, ui, &p.id, &p.dir, *job, v)
            {
                let item = c.item.as_ref();
                let params = json!({"plugin": p.id, "button": c.button, "item": item.map(|i| &i.0), "itemButton": item.map(|i| &i.1), "values": c.values});
                start_run(app, ctx, d, &p.id, "click", params);
            }
        });
    app.auto.add_response(&format!("bashcut.plugins.{}", p.id), &head.header_response, &m.name);
    // a closed card still says how its last run went
    if head.body_returned.is_none() && d.last.contains_key(&p.id) {
        ui.indent(("bashcut.plugins.last", &p.id), |ui| last_run(app, ui, ctx, d, snap, &p.id));
    }
}

fn settings_form(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, m: &Manifest) {
    let t = app.tokens;
    ui.add_space(4.0);
    ui.label(egui::RichText::new(s("Settings")).strong().color(t.text));
    let saved = if m.settings.iter().any(|st| !st.is_secret() && !d.settings.contains_key(&format!("{}/{}", m.id, st.id))) {
        plugins::with(app, ctx, |h| h.settings(&m.id)).unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    if !d.stored.contains_key(&m.id) && m.settings.iter().any(Setting::is_secret) {
        let known = plugins::with(app, ctx, |h| h.secrets_set(&m.id)).unwrap_or_default();
        d.stored.insert(m.id.clone(), known);
    }
    let lang = strings::current();
    for st in &m.settings {
        let k = format!("{}/{}", m.id, st.id);
        let title = if st.title_in(lang).is_empty() { st.id.clone() } else { st.title_in(lang).to_string() };
        if st.is_secret() {
            let stored = d.stored.get(&m.id).and_then(|x| x.get(&st.id)).copied().unwrap_or(false);
            settle_secret(d, &m.id, &st.id, &k);
            let busy = d.writing.get(&k).map(|(set, _)| *set);
            if busy.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            ui.horizontal(|ui| {
                ui.label(title.as_str());
                let text = d.secrets.entry(k.clone()).or_default();
                let hint = if stored { s("stored") } else { s("not set") };
                let r = ui.add_enabled(busy.is_none(), egui::TextEdit::singleline(text).password(true).hint_text(hint).desired_width(140.0));
                app.auto.add_response(&format!("bashcut.plugins.{}.secret.{}", m.id, st.id), &r, &title);
                let value = text.clone();
                if let Some(set) = busy {
                    ui.spinner();
                    ui.label(egui::RichText::new(if set { s("Saving…") } else { s("Clearing…") }).color(t.text_dim));
                    return;
                }
                if button(app, ui, &format!("bashcut.plugins.{}.secret.{}.save", m.id, st.id), s("Save"), !value.is_empty()) {
                    write_secret(app, ctx, d, &m.id, &st.id, Some(value));
                }
                if stored && button(app, ui, &format!("bashcut.plugins.{}.secret.{}.clear", m.id, st.id), s("Clear"), true) {
                    write_secret(app, ctx, d, &m.id, &st.id, None);
                }
            });
        } else {
            let initial = saved.get(&st.id).or(st.default.as_ref()).map(value_text).unwrap_or_default();
            let text = d.settings.entry(k).or_insert(initial);
            let id = format!("bashcut.plugins.{}.settings.{}", m.id, st.id);
            ui.horizontal(|ui| {
                ui.label(title.as_str());
                field(app, ui, &id, &title, Spec::setting(st), text);
            });
        }
        if !st.help_in(lang).is_empty() {
            note(ui, plain(st.help_in(lang)), META, t.text_dim);
        }
    }
    if m.settings.iter().any(|st| !st.is_secret()) && button(app, ui, &format!("bashcut.plugins.{}.settings.save", m.id), s("Save settings"), true) {
        let mut values = Map::new();
        for st in m.settings.iter().filter(|st| !st.is_secret()) {
            let text = d.settings.get(&format!("{}/{}", m.id, st.id)).cloned().unwrap_or_default();
            values.insert(st.id.clone(), if text.is_empty() { Value::Null } else { Value::String(text) });
        }
        if run(app, ctx, d, "settings", json!({"plugin": m.id, "values": values})).is_some() {
            d.status = Some((true, f("Saved settings of {plugin}", &[("plugin", m.name_in(lang))])));
        }
    }
}

/// Start storing (`Some`) or clearing a secret off the UI thread; the row shows it busy meanwhile.
fn write_secret(app: &FilmcraftApp, ctx: &egui::Context, d: &mut Drafts, plugin: &str, setting: &str, value: Option<String>) {
    let set = value.is_some();
    match plugins::set_secret_async(app, ctx, plugin, setting, value) {
        Ok(slot) => {
            d.status = None;
            d.writing.insert(format!("{plugin}/{setting}"), (set, slot));
        }
        Err(e) => d.status = Some((false, hints::friendly(&e))),
    }
}

/// A finished background write: the row's stored flag follows it, a failure goes to the status line.
fn settle_secret(d: &mut Drafts, plugin: &str, setting: &str, k: &str) {
    let Some((set, slot)) = d.writing.get(k) else { return };
    let set = *set;
    let Some(r) = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() else { return };
    d.writing.remove(k);
    match r {
        Ok(()) => {
            if set {
                d.secrets.remove(k);
            }
            d.stored.entry(plugin.to_string()).or_default().insert(setting.to_string(), set);
        }
        Err(e) => d.status = Some((false, hints::friendly(&e))),
    }
}

fn action_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, m: &Manifest, a: &Action) {
    let t = app.tokens;
    let lang = strings::current();
    let title = if a.title_in(lang).is_empty() { a.id.clone() } else { a.title_in(lang).to_string() };
    let is_open = d.open.as_ref().is_some_and(|(p, x)| *p == m.id && *x == a.id);
    let control = hints::control_on();
    // wrapped, so a long action title stays inside the panel
    ui.horizontal_wrapped(|ui| {
        let label = if a.params.is_empty() { format!("▶ {title}") } else { title.clone() };
        // ▶ runs at once, so it waits for the channel; a form still opens (its batch needs none)
        let now = a.params.is_empty().then(|| run_blocker(control, a, &[])).flatten();
        let mut r = ui.add_enabled(now.is_none(), egui::Button::new(label.as_str()).wrap());
        if let Some(why) = &now {
            r = r.on_disabled_hover_text(why);
        }
        app.auto.add_response(&format!("bashcut.plugins.{}.action.{}", m.id, a.id), &r, &label);
        if r.clicked() {
            if a.params.is_empty() {
                start(app, ctx, d, m, a);
            } else {
                d.open = if is_open { None } else { Some((m.id.clone(), a.id.clone())) };
            }
        }
        if a.needs == "project" && control {
            note(ui, s("uses the open project"), META, t.text_dim);
        } else if a.needs == "project" {
            let r = note(ui, s("needs the control channel"), META, t.text_dim).on_hover_text(s(HOW_TO_TURN_ON));
            app.auto.add_response(&format!("bashcut.plugins.{}.action.{}.blocked", m.id, a.id), &r, s("needs the control channel"));
        }
        // with parameters, the button sits in the form so the batch takes their values
        if a.needs == "project" && a.params.is_empty() {
            batch_button(app, ui, ctx, d, m, a, None);
        }
    });
    if !is_open {
        return;
    }
    egui::Frame::group(ui.style()).show(ui, |ui| {
        for p in &a.params {
            let k = format!("{}/{}/{}", m.id, a.id, p.id);
            let ptitle = param_title(p);
            let text = d.params.entry(k).or_insert_with(|| p.default.as_ref().map(value_text).unwrap_or_default());
            let id = format!("bashcut.plugins.{}.action.{}.param.{}", m.id, a.id, p.id);
            ui.horizontal(|ui| {
                ui.label(if required(p) { format!("{ptitle} *") } else { ptitle.clone() });
                field(app, ui, &id, &ptitle, Spec::param(p), text);
            });
        }
        let missing = missing(d, m, a);
        let why = run_blocker(control, a, &missing);
        ui.horizontal(|ui| {
            if gated_button(app, ui, &format!("bashcut.plugins.{}.{}.run", m.id, a.id), s("Run"), why.as_deref()) {
                start(app, ctx, d, m, a);
            }
            // a batch opens the project files itself: only the empty fields hold it back
            if a.needs == "project" {
                batch_button(app, ui, ctx, d, m, a, run_blocker(true, a, &missing).as_deref());
            }
        });
    });
}

/// Why Run can't start `a` yet, one reason per line: a project action without the control channel
/// ([`hints::control_on`]), then the empty required fields; `None` when it can.
fn run_blocker(control: bool, a: &Action, missing: &[String]) -> Option<String> {
    let mut reasons = Vec::new();
    if a.needs == "project" && !control {
        reasons.push(s(HOW_TO_TURN_ON).to_string());
    }
    if !missing.is_empty() {
        reasons.push(f("Fill in: {fields}", &[("fields", &missing.join(", "))]));
    }
    (!reasons.is_empty()).then(|| reasons.join("\n"))
}

/// A button disabled while `why` says what is missing (shown when hovered); `true` when clicked.
pub(super) fn gated_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, why: Option<&str>) -> bool {
    let mut r = ui.add_enabled(why.is_none(), egui::Button::new(label));
    if let Some(why) = why {
        r = r.on_disabled_hover_text(why);
    }
    app.auto.add_response(id, &r, label);
    r.clicked()
}

/// "Run on many files…": the action, with the form's params, becomes a step of the Batch section's
/// draft (#46), which opens. Disabled while `why` names empty required params.
fn batch_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, m: &Manifest, a: &Action, why: Option<&str>) {
    if !gated_button(app, ui, &format!("bashcut.plugins.{}.{}.batch", m.id, a.id), s("Run on many files…"), why) {
        return;
    }
    let params = form_params(d, m, a);
    match super::batch::invoke(app, ctx, "prefill", &json!({"plugin": m.id, "action": a.id, "params": params})) {
        Ok(_) => d.open = None,
        Err(e) => d.status = Some((false, hints::friendly(&e))),
    }
}

/// Start `a` with the form's params (empty ones are left to the plugin's defaults).
fn start(app: &mut FilmcraftApp, ctx: &egui::Context, d: &mut Drafts, m: &Manifest, a: &Action) {
    let params = form_params(d, m, a);
    if start_run(app, ctx, d, &m.id, "run", json!({"plugin": m.id, "action": a.id, "params": params})) {
        d.open = None;
    }
}

/// The action form's params (empty ones are left to the plugin's defaults).
fn form_params(d: &Drafts, m: &Manifest, a: &Action) -> Map<String, Value> {
    let mut params = Map::new();
    for p in &a.params {
        let text = d.params.get(&format!("{}/{}/{}", m.id, a.id, p.id)).cloned().unwrap_or_default();
        if let Some(v) = param_value(p, &text) {
            params.insert(p.id.clone(), v);
        }
    }
    params
}

/// The titles of `a`'s required params whose field is empty.
fn missing(d: &Drafts, m: &Manifest, a: &Action) -> Vec<String> {
    let empty = |p: &Param| d.params.get(&format!("{}/{}/{}", m.id, a.id, p.id)).is_none_or(|t| t.trim().is_empty());
    a.params.iter().filter(|p| required(p) && empty(p)).map(param_title).collect()
}

/// A param the plugin can't run without: not optional and with no default (a checkbox always has a value).
pub(super) fn required(p: &Param) -> bool {
    !p.optional && p.default.is_none() && p.kind != "bool"
}

/// The param's label in the UI language: its `title` (or the manifest's translation of it), else its id in words.
pub(super) fn param_title(p: &Param) -> String {
    p.title_in(strings::current()).map_or_else(|| humanise(&p.id), str::to_string)
}

/// An id as a label: camelCase, snake_case and kebab-case become words, first one capitalised
/// (`maxCps` → "Max cps", `out_dir` → "Out dir").
pub(super) fn humanise(id: &str) -> String {
    let mut words = String::new();
    let mut prev_lower = false;
    for c in id.chars() {
        if c == '_' || c == '-' || c == ' ' {
            prev_lower = false;
            if !words.is_empty() && !words.ends_with(' ') {
                words.push(' ');
            }
            continue;
        }
        if c.is_uppercase() && prev_lower {
            words.push(' ');
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        words.extend(c.to_lowercase());
    }
    let words = words.trim_end();
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => id.to_string(),
    }
}

/// A form's text as the param's JSON value; `None` when left empty. Numbers go out as JSON
/// numbers, clamped to the manifest's bounds (text that isn't one is sent as is, for the plugin to
/// refuse).
pub(super) fn param_value(p: &Param, text: &str) -> Option<Value> {
    let text_value = || Some(Value::String(text.to_string()));
    match p.kind.as_str() {
        "bool" => Some(Value::Bool(text == "true")),
        _ if text.trim().is_empty() => None,
        "integer" => match text.trim().parse::<f64>() {
            // `as` saturates: a huge value can't wrap
            Ok(n) if n.is_finite() => Some(Value::from(clamp(n.round(), p.minimum, p.maximum) as i64)),
            _ => text_value(),
        },
        "number" => match text.trim().parse::<f64>().ok().and_then(|n| serde_json::Number::from_f64(clamp(n, p.minimum, p.maximum))) {
            Some(n) => Some(Value::Number(n)),
            None => text_value(),
        },
        _ => text_value(),
    }
}

/// `n` within the optional bounds (a reversed pair leaves it as is rather than panic).
fn clamp(n: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let n = min.filter(|m| m.is_finite()).map_or(n, |m| n.max(m));
    max.filter(|m| m.is_finite() && min.is_none_or(|lo| lo <= *m)).map_or(n, |m| n.min(m))
}

pub(super) fn value_text(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// What a form field edits: the manifest's `type`, its choices and its bounds.
#[derive(Clone, Copy, Default)]
pub(super) struct Spec<'a> {
    pub kind: &'a str,
    pub choices: Option<&'a [String]>,
    /// The manifest's translations, for the choice labels.
    pub i18n: Option<&'a I18n>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

impl<'a> Spec<'a> {
    pub fn param(p: &'a Param) -> Self {
        Spec { kind: &p.kind, choices: p.choices.as_deref(), i18n: Some(&p.i18n), minimum: p.minimum, maximum: p.maximum }
    }

    /// The label of choice `value` in the UI language (`i18n.<lang>.choices`), else the value.
    pub fn choice_label(&self, value: &str) -> String {
        self.i18n.map_or(value, |i| manifest::choice_label(i, strings::current(), value)).to_string()
    }

    pub fn setting(st: &'a Setting) -> Self {
        Spec { kind: &st.kind, choices: st.choices.as_deref(), i18n: Some(&st.i18n), ..Spec::default() }
    }
}

/// One form field over its text: a checkbox for `bool`, a combo box for choices, a drag value for
/// `integer` / `number` (it takes no letters), a text field with **Browse…** for `path`, else a
/// text field.
pub(super) fn field(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, spec: Spec, text: &mut String) {
    let r = if spec.kind == "bool" {
        let mut on = text == "true";
        let r = ui.checkbox(&mut on, "");
        *text = on.to_string();
        r
    } else if let Some(choices) = spec.choices {
        egui::ComboBox::from_id_salt(id)
            .selected_text(spec.choice_label(text))
            .show_ui(ui, |ui| {
                for c in choices {
                    ui.selectable_value(text, c.clone(), spec.choice_label(c));
                }
            })
            .response
    } else if spec.kind == "integer" || spec.kind == "number" {
        number(ui, spec, text)
    } else if spec.kind == "path" {
        // the picker answers in a later frame: its path waits in this slot for the field
        let slot = egui::Id::new((id, "bashcut.plugins.picked"));
        if let Some(path) = ui.ctx().data_mut(|d| d.remove_temp::<String>(slot)) {
            *text = path;
        }
        // leave room for Browse… in a narrow panel
        let width = (ui.available_width() - 90.0).clamp(60.0, 160.0);
        let r = ui.add(egui::TextEdit::singleline(text).desired_width(width));
        if button(app, ui, &format!("{id}.browse"), s("Browse…"), true) {
            let ctx = ui.ctx().clone();
            app.pick_ui(FileDialog::OpenFiles { exts: Vec::new() }, move |_, paths| {
                if let Some(path) = paths.into_iter().next() {
                    ctx.data_mut(|d| d.insert_temp(slot, path));
                }
            });
        }
        r
    } else {
        ui.add(egui::TextEdit::singleline(text).desired_width(160.0))
    };
    app.auto.add_response(id, &r, label);
}

/// A drag value over a number's text, within the bounds. Empty text shows "–" (the plugin's
/// default) until it is dragged or typed.
fn number(ui: &mut egui::Ui, spec: Spec, text: &mut String) -> egui::Response {
    let integer = spec.kind == "integer";
    let lo = spec.minimum.filter(|n| n.is_finite()).unwrap_or(f64::MIN);
    let hi = spec.maximum.filter(|n| n.is_finite() && *n >= lo).unwrap_or(f64::MAX);
    let parsed = text.trim().parse::<f64>().ok().filter(|n| n.is_finite());
    let mut n = parsed.unwrap_or_else(|| clamp(0.0, spec.minimum, spec.maximum));
    let mut drag = egui::DragValue::new(&mut n).range(lo..=hi).speed(if integer { 0.2 } else { 0.1 });
    if integer {
        drag = drag.max_decimals(0);
    }
    if parsed.is_none() {
        drag = drag.custom_formatter(|_, _| "–".to_string());
    }
    let r = ui.add(drag);
    if r.changed() {
        *text = if integer { format!("{}", n.round() as i64) } else { n.to_string() };
    }
    if parsed.is_none() { r.on_hover_text(s("Not set: the plugin's default is used")) } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_prose_drops_code_backticks() {
        assert_eq!(plain("empty = the one `setup` installed"), "empty = the one setup installed");
        assert_eq!(plain("no code here"), "no code here");
    }

    #[test]
    fn form_strings_have_vietnamese() {
        super::super::strings::set_language(Some("vi"));
        assert_eq!(s("Browse…"), "Chọn tệp…");
        assert_eq!(f("Fill in: {fields}", &[("fields", "Answer")]), "Cần điền: Answer");
        assert_ne!(s("Not set: the plugin's default is used"), "Not set: the plugin's default is used");
        super::super::strings::set_language(None);
    }

    fn brief(state: JobState, progress: Option<f32>, message: &str) -> Brief {
        Brief { state, progress, message: message.into() }
    }

    #[test]
    fn the_card_says_how_its_last_run_went() {
        super::super::strings::set_language(Some("en"));
        let job = Last::Job(3);
        let line = |b: Brief| last_line(&job, Some(&b));
        let running = line(brief(JobState::Running, Some(0.4), "")).unwrap();
        assert_eq!((running.text.as_str(), running.failed, running.in_runs), ("Running 40 %", false, true));
        assert_eq!(line(brief(JobState::Running, None, "step 1")).unwrap().text, "Running…");
        assert_eq!(line(brief(JobState::Running, Some(f32::NAN), "")).unwrap().text, "Running…");
        assert_eq!(line(brief(JobState::Running, Some(7.0), "")).unwrap().text, "Running 100 %");
        assert_eq!(line(brief(JobState::Succeeded, Some(1.0), "3 captions")).unwrap().text, "Done");
        assert_eq!(line(brief(JobState::HandOff, None, "")).unwrap().text, "Needs the agent");
        let failed = line(brief(JobState::Failed, None, "bad_params: no captions")).unwrap();
        assert_eq!((failed.text.as_str(), failed.failed, failed.in_runs), ("Failed: bad_params: no captions", true, true));
        let python = line(brief(JobState::Crashed, None, "cannot start `python3`: No such file (os error 2)")).unwrap();
        assert!(python.text.starts_with("Failed: Python 3 was not found") && !python.text.contains("os error"), "{}", python.text);
        assert_eq!(line(brief(JobState::TimedOut, None, " ")).unwrap().text, "Failed");
        // a run that never started has no job in Runs to open
        let not_started = last_line(&Last::Error("no_cli: not found".into()), None).unwrap();
        assert!(not_started.failed && !not_started.in_runs && not_started.text.starts_with("Failed: The BashCutPro engine"), "{not_started:?}");
        // Runs was cleared: the line goes
        assert_eq!(last_line(&job, None), None);
        super::super::strings::set_language(None);
    }

    #[test]
    fn the_registry_line_counts_what_can_be_installed() {
        super::super::strings::set_language(Some("en"));
        assert_eq!(available_text(8, 8), "8 plugins available");
        assert_eq!(available_text(1, 3), "1 plugin available");
        assert_eq!(available_text(0, 3), "All plugins are installed");
        assert_eq!(available_text(0, 0), "0 plugins available");
        super::super::strings::set_language(None);
    }

    #[test]
    fn card_status_strings_have_vietnamese() {
        super::super::strings::set_language(Some("vi"));
        for en in [
            "Running…",
            "Open in Runs",
            "Dismiss",
            "Installing…",
            "Updating…",
            "Checking the registry…",
            "1 plugin available",
            "All plugins are installed",
            "Done",
            "Needs the agent",
            "Cancelled",
            "Failed",
        ] {
            assert_ne!(s(en), en, "no Vietnamese for {en}");
        }
        assert_eq!(f("Running {percent} %", &[("percent", "40")]), "Đang chạy 40 %");
        assert_eq!(available_text(8, 8), "Có 8 plugin");
        assert_eq!(last_line(&Last::Error("bad_params: x".into()), None).map(|l| l.text).as_deref(), Some("Lỗi: bad_params: x"));
        super::super::strings::set_language(None);
    }

    fn param(id: &str, kind: &str) -> Param {
        Param { id: id.into(), kind: kind.into(), ..Param::default() }
    }

    #[test]
    fn labels_are_the_title_or_the_id_in_words() {
        assert_eq!(humanise("maxCps"), "Max cps");
        assert_eq!(humanise("out_dir"), "Out dir");
        assert_eq!(humanise("out-dir"), "Out dir");
        assert_eq!(humanise("target"), "Target");
        assert_eq!(humanise("track2Name"), "Track2 name");
        assert_eq!(humanise("_"), "_");
        assert_eq!(param_title(&param("maxCps", "integer")), "Max cps");
        let titled = Param { title: Some("Characters per second".into()), ..param("maxCps", "integer") };
        assert_eq!(param_title(&titled), "Characters per second");
    }

    #[test]
    fn manifest_labels_follow_the_ui_language() {
        // a slice of translate-captions' manifest, with its Vietnamese (I18N1)
        let m = Manifest::parse(
            &json!({
                "schema": "bashcut-plugin/1", "id": "bashcut.translate-captions", "name": "Translate Captions", "version": "0.1.3",
                "entry": {"command": ["python3", "plugin.py"]}, "output": "bashcut-result/1",
                "i18n": {"vi": {"name": "Dịch phụ đề"}},
                "settings": [{"id": "service", "title": "Service", "type": "enum", "choices": ["gemini", "openai"], "default": "gemini",
                    "help": "Who translates. The key is sent only to this service.",
                    "i18n": {"vi": {"title": "Dịch vụ", "help": "Bên dịch. Khóa chỉ được gửi tới dịch vụ này.",
                        "choices": {"gemini": "Gemini (Google)"}}}}],
                "actions": [{"id": "translate", "title": "Translate captions…", "args": ["translate"], "needs": "project",
                    "i18n": {"vi": {"title": "Dịch phụ đề…"}},
                    "params": [{"id": "maxCps", "title": "Max characters a second", "flag": "--max-cps", "type": "integer",
                        "i18n": {"vi": {"title": "Số ký tự tối đa mỗi giây"}}},
                        {"id": "brief", "title": "Brief", "flag": "--brief", "type": "string"}]}]
            })
            .to_string(),
        )
        .unwrap();
        let (st, a) = (&m.settings[0], &m.actions[0]);
        super::super::strings::set_language(Some("vi"));
        let lang = strings::current();
        assert_eq!(
            (m.name_in(lang), st.title_in(lang), st.help_in(lang), a.title_in(lang)),
            ("Dịch phụ đề", "Dịch vụ", "Bên dịch. Khóa chỉ được gửi tới dịch vụ này.", "Dịch phụ đề…")
        );
        assert_eq!(
            (param_title(&a.params[0]), param_title(&a.params[1])),
            ("Số ký tự tối đa mỗi giây".to_string(), "Brief".to_string()),
            "untranslated: English"
        );
        let spec = Spec::setting(st);
        assert_eq!((spec.choice_label("gemini"), spec.choice_label("openai")), ("Gemini (Google)".into(), "openai".into()));
        assert_eq!(Spec::param(&a.params[1]).choice_label("x"), "x");
        super::super::strings::set_language(Some("en"));
        assert_eq!(Spec::setting(st).choice_label("gemini"), "gemini");
        let lang = strings::current();
        assert_eq!((m.name_in(lang), st.title_in(lang), a.title_in(lang)), ("Translate Captions", "Service", "Translate captions…"));
        assert_eq!(param_title(&a.params[0]), "Max characters a second");
        super::super::strings::set_language(None);
    }

    #[test]
    fn required_params_are_the_ones_without_optional_or_default() {
        let answer = param("answer", "path");
        let lines = Param { optional: true, ..param("lines", "path") };
        let target = Param { default: Some(json!("vi")), ..param("target", "enum") };
        assert!(required(&answer));
        assert!(!required(&lines));
        assert!(!required(&target));
        assert!(!required(&param("dry", "bool")));
        let m = Manifest { id: "p".into(), ..Manifest::default() };
        let a = Action { id: "apply".into(), params: vec![answer, lines, target], ..Action::default() };
        let mut d = Drafts::default();
        assert_eq!(missing(&d, &m, &a), ["Answer"]);
        d.params.insert("p/apply/answer".into(), " ".into());
        assert_eq!(missing(&d, &m, &a), ["Answer"]);
        d.params.insert("p/apply/answer".into(), "/tmp/a.json".into());
        assert!(missing(&d, &m, &a).is_empty());
    }

    #[test]
    fn run_waits_for_the_control_channel_and_the_required_fields() {
        let project = Action { id: "translate".into(), needs: "project".into(), ..Action::default() };
        let none = Action { id: "check-key".into(), needs: "none".into(), ..Action::default() };
        super::super::strings::set_language(Some("en"));
        let answer = ["Answer".to_string()];
        assert_eq!(run_blocker(true, &project, &[]), None);
        assert_eq!(run_blocker(false, &none, &[]), None);
        assert_eq!(run_blocker(false, &project, &[]).as_deref(), Some(HOW_TO_TURN_ON));
        assert_eq!(run_blocker(true, &project, &answer).as_deref(), Some("Fill in: Answer"));
        let both = run_blocker(false, &project, &answer).unwrap_or_default();
        assert_eq!(both.lines().collect::<Vec<_>>(), [HOW_TO_TURN_ON, "Fill in: Answer"], "one list, the channel first");
        super::super::strings::set_language(None);
    }

    #[test]
    fn param_values_send_numbers_as_numbers_within_bounds() {
        let cps = Param { minimum: Some(8.0), maximum: Some(30.0), ..param("maxCps", "integer") };
        assert_eq!(param_value(&cps, "17"), Some(json!(17)));
        assert_eq!(param_value(&cps, "17.6"), Some(json!(18)));
        assert_eq!(param_value(&cps, "99"), Some(json!(30)));
        assert_eq!(param_value(&cps, "1"), Some(json!(8)));
        assert_eq!(param_value(&cps, ""), None);
        assert_eq!(param_value(&cps, "lots"), Some(json!("lots")));
        assert_eq!(param_value(&param("n", "integer"), "1e30"), Some(json!(i64::MAX)));
        let secs = param("seconds", "number");
        assert_eq!(param_value(&secs, "2.5"), Some(json!(2.5)));
        assert_eq!(param_value(&secs, "NaN"), Some(json!("NaN")));
        let reversed = Param { minimum: Some(5.0), maximum: Some(1.0), ..param("x", "number") };
        assert_eq!(param_value(&reversed, "3"), Some(json!(5.0)));
        assert_eq!(param_value(&param("dry", "bool"), "true"), Some(json!(true)));
        assert_eq!(param_value(&param("dry", "bool"), ""), Some(json!(false)));
        assert_eq!(param_value(&param("track", "string"), "V1"), Some(json!("V1")));
    }
}
