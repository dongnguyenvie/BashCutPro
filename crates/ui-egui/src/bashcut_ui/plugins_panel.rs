//! The Plugins section (`bashcut/in-app-ui.md` §2): installed plugins with their settings and
//! actions, and what the registry offers. Every button goes through the `bashcut.plugins.*`
//! commands ([`plugins::invoke`]) and registers an automation id.

use std::collections::BTreeMap;

use bashcut_plugin_host::JobState;
use bashcut_plugin_host::install::Installed;
use bashcut_plugin_host::manifest::{Action, Manifest, Param, Setting};
use bashcut_plugin_host::registry::Entry;
use serde_json::{Map, Value, json};

use super::hints::{self, CONTROL_OFF, HOW_TO_TURN_ON};
use super::plugins;
use super::strings::{f, s};
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
    /// The action whose form is open: `(plugin, action)`.
    open: Option<(String, String)>,
    /// `<plugin>/<action>/<param>` → value being edited.
    params: BTreeMap<String, String>,
    /// The last command's outcome: `(ok, message)`.
    status: Option<(bool, String)>,
    /// Per plugin, the run whose form last opened its card (a new form opens it once).
    shown_view: BTreeMap<String, u64>,
    /// The registry fetch the "Checking…" line waits for.
    checking: Option<u64>,
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.plugins.drafts")
}

fn drafts(ctx: &egui::Context) -> Drafts {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Drafts>(key()).clone())
}

/// A button with an automation id; `true` when clicked.
pub fn button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let r = ui.add_enabled(enabled, egui::Button::new(label));
    app.auto.add_response(id, &r, label);
    r.clicked()
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

pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut d = drafts(&ctx);
    let t = app.tokens;
    let (installed, registry, location, running, checked) = plugins::with(app, &ctx, |h| {
        let checked = d.checking.and_then(|id| h.job(id)).filter(|j| j.state != JobState::Running).map(|j| (j.state, j.message.clone()));
        (h.installed().to_vec(), h.registry().cloned(), format!("{}@{}", h.registry_location.0, h.registry_location.1), h.any_running(), checked)
    });
    // the fetch ended: drop "Checking…", or say why it failed
    if let Some((state, msg)) = checked {
        d.checking = None;
        d.status = (state != JobState::Succeeded).then(|| (false, f("Could not check {location}: {error}", &[("location", &location), ("error", &msg)])));
    }
    let can_install = super::data_dir(app).is_some();

    // before any click: without the control channel, project actions cannot run
    if !hints::control_on() {
        let r = ui.label(egui::RichText::new(s(CONTROL_OFF)).size(11.0).color(t.text_dim)).on_hover_text(s(HOW_TO_TURN_ON));
        app.auto.add_response("bashcut.plugins.setup", &r, s(CONTROL_OFF));
        ui.add_space(4.0);
    }
    if let Some((ok, msg)) = &d.status {
        let color = if *ok { t.text_dim } else { t.danger };
        let r = ui.label(egui::RichText::new(msg).size(11.0).color(color));
        app.auto.add_response("bashcut.plugins.status", &r, msg);
    }

    // installed
    ui.label(egui::RichText::new(s("Installed")).strong().color(t.text));
    if installed.is_empty() {
        ui.label(egui::RichText::new(s("No plugins installed yet.")).size(11.0).color(t.text_dim));
    }
    for p in &installed {
        plugin_card(app, ui, &ctx, &mut d, p);
    }

    // registry
    ui.add_space(8.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(s("Available")).strong().color(t.text));
        let label = if registry.is_some() { s("Refresh") } else { s("Check registry") };
        if button(app, ui, "bashcut.plugins.registry.refresh", label, can_install)
            && let Some(v) = run(app, &ctx, &mut d, "registry", json!({}))
        {
            d.checking = v["job"].as_u64();
            d.status = Some((true, f("Checking {location}…", &[("location", &location)])));
        }
    });
    if !can_install {
        ui.label(egui::RichText::new(s("No data folder in this session: plugins cannot be installed.")).size(11.0).color(t.text_dim));
    }
    match &registry {
        None => {
            ui.label(egui::RichText::new(f("Registry: {location}", &[("location", &location)])).size(11.0).color(t.text_dim));
        }
        Some(reg) => {
            for e in reg.plugins.iter().filter(|e| !installed.iter().any(|p| p.id == e.id)) {
                registry_row(app, ui, &ctx, &mut d, e, can_install);
            }
        }
    }
    if running {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    ctx.data_mut(|data| data.insert_temp(key(), d));
}

fn registry_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, e: &Entry, can_install: bool) {
    let t = app.tokens;
    let supported = e.supports(bashcut_plugin_host::os());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{} {}", e.name, e.version)).color(t.text));
        if button(app, ui, &format!("bashcut.plugins.{}.install", e.id), s("Install"), can_install && supported) {
            run(app, ctx, d, "install", json!({"plugin": e.id}));
        }
        if !supported {
            ui.label(egui::RichText::new(s("Not available on this system")).size(10.5).color(t.text_dim));
        }
    });
    if !e.summary.is_empty() {
        ui.label(egui::RichText::new(plain(&e.summary)).size(11.0).color(t.text_dim));
    }
}

fn plugin_card(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, p: &Installed) {
    let t = app.tokens;
    let Some(m) = &p.manifest else {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&p.id).color(t.text));
            if button(app, ui, &format!("bashcut.plugins.{}.remove", p.id), s("Remove"), true) {
                run(app, ctx, d, "remove", json!({"plugin": p.id}));
            }
        });
        let error = p.error.clone().unwrap_or_default();
        ui.label(egui::RichText::new(error).size(11.0).color(t.text_dim));
        return;
    };
    let (update, view) = plugins::with(app, ctx, |h| (h.update_for(&p.id), h.view(&p.id).map(|(job, v)| (job, v.clone()))));
    let head = egui::CollapsingHeader::new(egui::RichText::new(format!("{} {}", m.name, m.version)).color(t.text))
        .id_salt(("bashcut.plugins.card", &p.id))
        .open(view.as_ref().and_then(|(job, _)| (d.shown_view.insert(p.id.clone(), *job) != Some(*job)).then_some(true)))
        .show(ui, |ui| {
            if !m.summary.is_empty() {
                ui.label(egui::RichText::new(plain(&m.summary)).size(11.0).color(t.text_dim));
            }
            ui.horizontal(|ui| {
                if let Some(v) = &update
                    && button(app, ui, &format!("bashcut.plugins.{}.update", p.id), &f("Update to {version}", &[("version", v)]), true)
                {
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
                if let Some(v) = run(app, ctx, d, "click", params) {
                    let job = v.get("job").and_then(Value::as_u64).unwrap_or_default();
                    d.status = Some((true, f("Started run {job}", &[("job", &job.to_string())])));
                }
            }
        });
    app.auto.add_response(&format!("bashcut.plugins.{}", p.id), &head.header_response, &m.name);
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
    for st in &m.settings {
        let k = format!("{}/{}", m.id, st.id);
        let title = if st.title.is_empty() { st.id.clone() } else { st.title.clone() };
        if st.is_secret() {
            let stored = d.stored.get(&m.id).and_then(|x| x.get(&st.id)).copied().unwrap_or(false);
            ui.horizontal(|ui| {
                ui.label(title.as_str());
                let text = d.secrets.entry(k.clone()).or_default();
                let hint = if stored { s("stored") } else { s("not set") };
                let r = ui.add(egui::TextEdit::singleline(text).password(true).hint_text(hint).desired_width(140.0));
                app.auto.add_response(&format!("bashcut.plugins.{}.secret.{}", m.id, st.id), &r, &title);
                let value = text.clone();
                if button(app, ui, &format!("bashcut.plugins.{}.secret.{}.save", m.id, st.id), s("Save"), !value.is_empty())
                    && let Some(v) = run(app, ctx, d, "setSecret", json!({"plugin": m.id, "setting": st.id, "value": value}))
                {
                    d.secrets.remove(&k);
                    remember_stored(d, &m.id, &v);
                }
                if stored
                    && button(app, ui, &format!("bashcut.plugins.{}.secret.{}.clear", m.id, st.id), s("Clear"), true)
                    && let Some(v) = run(app, ctx, d, "setSecret", json!({"plugin": m.id, "setting": st.id}))
                {
                    remember_stored(d, &m.id, &v);
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
        if !st.help.is_empty() {
            ui.label(egui::RichText::new(plain(&st.help)).size(10.5).color(t.text_dim));
        }
    }
    if m.settings.iter().any(|st| !st.is_secret()) && button(app, ui, &format!("bashcut.plugins.{}.settings.save", m.id), s("Save settings"), true) {
        let mut values = Map::new();
        for st in m.settings.iter().filter(|st| !st.is_secret()) {
            let text = d.settings.get(&format!("{}/{}", m.id, st.id)).cloned().unwrap_or_default();
            values.insert(st.id.clone(), if text.is_empty() { Value::Null } else { Value::String(text) });
        }
        if run(app, ctx, d, "settings", json!({"plugin": m.id, "values": values})).is_some() {
            d.status = Some((true, f("Saved settings of {plugin}", &[("plugin", &m.name)])));
        }
    }
}

fn remember_stored(d: &mut Drafts, plugin: &str, v: &Value) {
    if let Ok(map) = serde_json::from_value::<BTreeMap<String, bool>>(v.get("secrets").cloned().unwrap_or(Value::Null)) {
        d.stored.insert(plugin.to_string(), map);
    }
}

fn action_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, m: &Manifest, a: &Action) {
    let t = app.tokens;
    let title = if a.title.is_empty() { a.id.clone() } else { a.title.clone() };
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
            ui.label(egui::RichText::new(s("uses the open project")).size(10.5).color(t.text_dim));
        } else if a.needs == "project" {
            let r = ui.label(egui::RichText::new(s("needs the control channel")).size(10.5).color(t.text_dim)).on_hover_text(s(HOW_TO_TURN_ON));
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
fn gated_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, why: Option<&str>) -> bool {
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
    if let Some(v) = run(app, ctx, d, "run", json!({"plugin": m.id, "action": a.id, "params": params})) {
        let job = v.get("job").and_then(Value::as_u64).unwrap_or_default();
        d.status = Some((true, f("Started run {job}", &[("job", &job.to_string())])));
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
fn required(p: &Param) -> bool {
    !p.optional && p.default.is_none() && p.kind != "bool"
}

/// The param's label: its `title`, else its id in words.
fn param_title(p: &Param) -> String {
    p.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| humanise(&p.id))
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
fn param_value(p: &Param, text: &str) -> Option<Value> {
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

fn value_text(v: &Value) -> String {
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
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

impl<'a> Spec<'a> {
    pub fn param(p: &'a Param) -> Self {
        Spec { kind: &p.kind, choices: p.choices.as_deref(), minimum: p.minimum, maximum: p.maximum }
    }

    pub fn setting(st: &'a Setting) -> Self {
        Spec { kind: &st.kind, choices: st.choices.as_deref(), ..Spec::default() }
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
            .selected_text(text.as_str())
            .show_ui(ui, |ui| {
                for c in choices {
                    ui.selectable_value(text, c.clone(), c.as_str());
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
