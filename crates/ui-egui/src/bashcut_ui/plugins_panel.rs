//! The Plugins section (`bashcut/in-app-ui.md` §2): installed plugins with their settings and
//! actions, and what the registry offers. Every button goes through the `bashcut.plugins.*`
//! commands ([`plugins::invoke`]) and registers an automation id.

use std::collections::BTreeMap;

use bashcut_plugin_host::install::Installed;
use bashcut_plugin_host::manifest::{Action, Manifest, Param, Setting};
use bashcut_plugin_host::registry::Entry;
use serde_json::{Map, Value, json};

use super::plugins;
use super::strings::{f, s};
use crate::FilmcraftApp;

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

/// Run `bashcut.plugins.<name>` and keep its outcome for the status line.
fn run(app: &mut FilmcraftApp, ctx: &egui::Context, d: &mut Drafts, name: &str, params: Value) -> Option<Value> {
    match plugins::invoke(app, ctx, name, &params) {
        Ok(v) => {
            d.status = None;
            Some(v)
        }
        Err(e) => {
            d.status = Some((false, e));
            None
        }
    }
}

pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut d = drafts(&ctx);
    let t = app.tokens;
    let (installed, registry, location, running) = plugins::with(app, &ctx, |h| {
        (h.installed().to_vec(), h.registry().cloned(), format!("{}@{}", h.registry_location.0, h.registry_location.1), h.any_running())
    });
    let can_install = super::data_dir(app).is_some();

    if let Some((ok, msg)) = &d.status {
        let color = if *ok { t.text_dim } else { egui::Color32::from_rgb(0xe0, 0x6c, 0x5f) };
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
        if button(app, ui, "bashcut.plugins.registry.refresh", label, can_install) && run(app, &ctx, &mut d, "registry", json!({})).is_some() {
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
        ui.label(egui::RichText::new(&e.summary).size(11.0).color(t.text_dim));
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
    let update = plugins::with(app, ctx, |h| h.update_for(&p.id));
    let head = egui::CollapsingHeader::new(egui::RichText::new(format!("{} {}", m.name, m.version)).color(t.text))
        .id_salt(("bashcut.plugins.card", &p.id))
        .show(ui, |ui| {
            if !m.summary.is_empty() {
                ui.label(egui::RichText::new(&m.summary).size(11.0).color(t.text_dim));
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
                field(app, ui, &id, &title, &st.kind, st.choices.as_deref(), text);
            });
        }
        if !st.help.is_empty() {
            ui.label(egui::RichText::new(&st.help).size(10.5).color(t.text_dim));
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
    ui.horizontal(|ui| {
        let label = if a.params.is_empty() { format!("▶ {title}") } else { title.clone() };
        if button(app, ui, &format!("bashcut.plugins.{}.action.{}", m.id, a.id), &label, true) {
            if a.params.is_empty() {
                start(app, ctx, d, m, a);
            } else {
                d.open = if is_open { None } else { Some((m.id.clone(), a.id.clone())) };
            }
        }
        if a.needs == "project" {
            ui.label(egui::RichText::new(s("uses the open project")).size(10.5).color(t.text_dim));
        }
        // with parameters, the button sits in the form so the batch takes their values
        if a.needs == "project" && a.params.is_empty() {
            batch_button(app, ui, ctx, d, m, a);
        }
    });
    if !is_open {
        return;
    }
    egui::Frame::group(ui.style()).show(ui, |ui| {
        for p in &a.params {
            let k = format!("{}/{}/{}", m.id, a.id, p.id);
            let ptitle = p.title.clone().unwrap_or_else(|| p.id.clone());
            let text = d.params.entry(k).or_insert_with(|| p.default.as_ref().map(value_text).unwrap_or_default());
            let id = format!("bashcut.plugins.{}.action.{}.param.{}", m.id, a.id, p.id);
            ui.horizontal(|ui| {
                ui.label(ptitle.as_str());
                field(app, ui, &id, &ptitle, &p.kind, p.choices.as_deref(), text);
            });
        }
        ui.horizontal(|ui| {
            if button(app, ui, &format!("bashcut.plugins.{}.{}.run", m.id, a.id), s("Run"), true) {
                start(app, ctx, d, m, a);
            }
            if a.needs == "project" {
                batch_button(app, ui, ctx, d, m, a);
            }
        });
    });
}

/// "Run on many files…": the action, with the form's params, becomes a step of the Batch section's
/// draft (#46), which opens.
fn batch_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, d: &mut Drafts, m: &Manifest, a: &Action) {
    if !button(app, ui, &format!("bashcut.plugins.{}.{}.batch", m.id, a.id), s("Run on many files…"), true) {
        return;
    }
    let params = form_params(d, m, a);
    match super::batch::invoke(app, ctx, "prefill", &json!({"plugin": m.id, "action": a.id, "params": params})) {
        Ok(_) => d.open = None,
        Err(e) => d.status = Some((false, e)),
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

/// A form's text as the param's JSON value; `None` when left empty.
fn param_value(p: &Param, text: &str) -> Option<Value> {
    match p.kind.as_str() {
        "bool" => Some(Value::Bool(text == "true")),
        _ if text.trim().is_empty() => None,
        _ => Some(Value::String(text.to_string())),
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// One form field: a checkbox for `bool`, a combo box for choices, else a text field.
fn field(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, kind: &str, choices: Option<&[String]>, text: &mut String) {
    let r = if kind == "bool" {
        let mut on = text == "true";
        let r = ui.checkbox(&mut on, "");
        *text = on.to_string();
        r
    } else if let Some(choices) = choices {
        egui::ComboBox::from_id_salt(id)
            .selected_text(text.as_str())
            .show_ui(ui, |ui| {
                for c in choices {
                    ui.selectable_value(text, c.clone(), c.as_str());
                }
            })
            .response
    } else {
        ui.add(egui::TextEdit::singleline(text).desired_width(160.0))
    };
    app.auto.add_response(id, &r, label);
}
