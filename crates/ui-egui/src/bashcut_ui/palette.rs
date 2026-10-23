//! The command palette (#52, `bashcut/in-app-ui.md` §1a): one search over engine and UI commands
//! (with their current shortcut and enabled state), `bashcut.*` actions, plugin actions, looks,
//! output profiles, templates and recipes, ranked by recent use.
//!
//! A foreground `egui::Area` drawn from the layout hook; Cmd/Ctrl+Shift+P opens it (Cmd/Ctrl+K is
//! upstream's Add Edit). Enter runs the selected item through `menus::invoke`, the normal command
//! path (engine edits are undoable); Tab, or Enter on an item that needs parameters, opens a JSON
//! parameter editor. State lives in egui context data; recent items in
//! `<data dir>/bashcut/palette.json`. Commands: `bashcut.palette.{open {query?}, close, search
//! {query, limit?}, run {item, params?}, recipe {recipe}}`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bashcut_plugin_host::install::Installed;
use egui::{Align2, Key, Modifiers, Rect, Sense, pos2, vec2};
use filmcraft_engine::shortcuts::CommandInfo;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::strings::{f, s};
use super::widgets::{META, code_note, note};
use crate::FilmcraftApp;
use crate::theme::Tokens;

/// Rows a search returns at most (the overlay and `bashcut.palette.search`).
const MAX_ROWS: usize = 50;
/// Recent items remembered.
const MAX_RECENT: usize = 30;
/// Largest `palette.json` read back.
const MAX_FILE: u64 = 64 * 1024;
const WIDTH: f32 = 600.0;
const ROW: f32 = 26.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// An upstream engine or UI command.
    Command,
    /// One of our `bashcut.*` commands.
    Pro,
    Plugin,
    Look,
    Profile,
    Template,
    Recipe,
}

impl Kind {
    pub const ALL: [Kind; 7] = [Kind::Command, Kind::Pro, Kind::Plugin, Kind::Look, Kind::Profile, Kind::Template, Kind::Recipe];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Command => "Command",
            Kind::Pro => "BashCutPro",
            Kind::Plugin => "Plugin",
            Kind::Look => "Look",
            Kind::Profile => "Profile",
            Kind::Template => "Template",
            Kind::Recipe => "Recipe",
        }
    }
}

/// One thing the palette can run: a command id with its parameters.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Stable key: `command:<id>`, `plugin:<plugin>/<action>`, `look:<id>`, `profile:<id>`,
    /// `template:<path>`, `recipe:<id>`. Recent use is remembered by key.
    pub key: String,
    pub kind: Kind,
    pub label: String,
    /// Menu path, purpose or summary (searched too).
    pub detail: String,
    /// The current shortcut, in this OS's notation.
    pub shortcut: Option<String>,
    pub enabled: bool,
    /// What runs: an engine or UI command id.
    pub command: String,
    pub params: Value,
    /// The command's parameter description, shown in the parameter editor.
    pub signature: String,
    /// Enter opens the parameter editor instead of running.
    pub needs_params: bool,
}

/// A list command whose entries become palette items, each running `run {param: <id>}`.
pub struct Catalog {
    pub kind: Kind,
    pub list: &'static str,
    /// The array in the list result.
    pub array: &'static str,
    pub id: &'static str,
    pub name: &'static str,
    pub detail: &'static str,
    pub run: &'static str,
    pub param: &'static str,
    pub needs_params: bool,
}

/// Catalogs read when the palette opens; a list command that is not built in is skipped. Batch
/// presets join here as one row once the batch runner has a list command (#45 / #46).
pub const CATALOGS: &[Catalog] = &[
    Catalog {
        kind: Kind::Look,
        list: "bashcut.looks.list",
        array: "looks",
        id: "id",
        name: "name",
        detail: "purpose",
        run: "bashcut.looks.apply",
        param: "look",
        needs_params: false,
    },
    Catalog {
        kind: Kind::Profile,
        list: "bashcut.profiles.list",
        array: "profiles",
        id: "id",
        name: "name",
        detail: "notes",
        run: "bashcut.profiles.apply",
        param: "profile",
        needs_params: false,
    },
    Catalog {
        kind: Kind::Template,
        list: "bashcut.template.list",
        array: "templates",
        id: "path",
        name: "name",
        detail: "path",
        run: "bashcut.template.instantiate",
        param: "template",
        needs_params: true,
    },
];

/// The agent kit's recipes (`bashcutpro-agent-kit/skills/vlog-*`): running one opens the Agent
/// section with a prompt that starts it.
pub const RECIPES: &[(&str, &str, &str)] = &[
    ("vlog-plan", "Plan a vlog", "Brief to plan: picks the recipe, survey, plan in the project notes"),
    ("vlog-ideate", "Ideas for a vlog", "A vague idea or a folder of footage into a brief"),
    ("vlog-hook-script", "Hook and script", "Hook, beats and voiceover script sized to the voice"),
    ("vlog-publish", "Finish for posting", "Exports per platform, covers, chapters, title and hashtags"),
    ("vlog-travel", "Travel vlog", "Cost or time hook, numbered places, a cost recap"),
    ("vlog-food", "Food vlog", "The bite as the hook, process inserts, real sizzle"),
    ("vlog-daily", "Daily-life vlog", "Time-stamped moments, calm holds, a diary voice"),
    ("vlog-product-review", "Product review", "Price-question hook, claims shown in use, a verdict"),
    ("vlog-product-ad", "Product ad", "Hook and CTA as a pair, truth rules, A/B variants"),
    ("vlog-talking-head", "Talking head", "Strongest line first, fillers cut, punch-ins, B-roll on claims"),
    ("vlog-podcast-clips", "Podcast clips", "Shorts by quote from a long recording"),
    ("vlog-tutorial", "Tutorial", "Result first, step cards, zoom on what is clicked"),
    ("vlog-explainer", "Explainer", "Question-and-number hook, one data graphic per section"),
    ("vlog-news", "News short", "Date cards, credits on every picture, verbatim quotes"),
    ("vlog-promo", "Promo", "Beat cuts, kinetic type, rolling counters"),
    ("vlog-scene-prompt", "Prompt a missing shot", "Prompts for AI video and image models"),
];

#[derive(Clone, Default)]
struct Palette {
    open: bool,
    /// Give the search field focus next frame.
    focus: bool,
    query: String,
    selected: usize,
    /// The item whose parameters are being edited.
    editing: Option<Item>,
    params: String,
    /// Scroll the selected row into view (after a key moved it).
    scroll: bool,
    error: Option<String>,
    items: Arc<Vec<Item>>,
    recent: Vec<String>,
    loaded: bool,
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.palette")
}

fn get(ctx: &egui::Context) -> Palette {
    super::shared::temp::<Palette>(ctx, key())
}

fn put(ctx: &egui::Context, p: Palette) {
    super::shared::put_temp(ctx, key(), p);
}

// ---------------------------------------------------------------- items

/// Whether a parameter description (`{"time":ticks,"track":id?}`) has a required field: one with
/// neither `?` nor a `=` default.
pub fn needs_params(signature: &str) -> bool {
    !required_keys(signature).is_empty()
}

/// The required keys of a parameter description, in order.
pub fn required_keys(signature: &str) -> Vec<String> {
    let Some(start) = signature.find('{') else { return Vec::new() };
    let mut depth = 0i32;
    let mut fields = vec![String::new()];
    for ch in signature[start..].chars() {
        match ch {
            '{' | '[' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            '}' | ']' => {
                depth -= 1;
                if depth <= 0 {
                    break;
                }
            }
            ',' if depth == 1 => {
                fields.push(String::new());
                continue;
            }
            _ => {}
        }
        if let Some(f) = fields.last_mut() {
            f.push(ch);
        }
    }
    fields
        .iter()
        .filter(|f| !f.trim().is_empty() && !f.contains('?') && !f.contains('='))
        .filter_map(|f| f.split('"').nth(1).filter(|k| !k.is_empty()).map(str::to_string))
        .collect()
}

/// The JSON the parameter editor starts from: the item's own params plus each required key.
pub fn template(item: &Item) -> Value {
    let mut m = match &item.params {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    for k in required_keys(&item.signature) {
        m.entry(k).or_insert(Value::Null);
    }
    Value::Object(m)
}

fn command_item(id: &str, label: &str, menu: &[String], signature: &str, shortcut: Option<String>, enabled: bool) -> Item {
    Item {
        key: format!("command:{id}"),
        kind: if id.starts_with("bashcut.") { Kind::Pro } else { Kind::Command },
        // BR5: upstream labels that name FilmCraft ("Selection as FilmCraft Project…", "FilmCraft Help…") as ours;
        // AG10: ours in the UI language (a label without a catalog row stays English)
        label: if id.starts_with("bashcut.") { s(label) } else { crate::bashcut_brand::text(label).unwrap_or(label) }.to_string(),
        detail: if menu.is_empty() { id.to_string() } else { format!("{} · {id}", menu.join(" › ")) },
        shortcut,
        enabled,
        command: id.to_string(),
        params: json!({}),
        signature: signature.to_string(),
        needs_params: needs_params(signature),
    }
}

/// Engine commands (with live shortcut and enabled state) and the frontend's commands.
pub fn command_items(session: &filmcraft_engine::Session) -> Vec<Item> {
    let short = |id: &str| session.shortcuts.primary(id).map(|k| crate::menus::shortcut_text(&k));
    let mut seen = HashSet::new();
    let mut v = Vec::new();
    for c in filmcraft_engine::commands::command_specs() {
        if !seen.insert(c.id.to_string()) {
            continue;
        }
        let menu: Vec<String> = c.menu.iter().map(|m| m.to_string()).collect();
        v.push(command_item(c.id, c.label, &menu, c.params, short(c.id), session.is_enabled(c.id)));
    }
    for c in crate::menus::external_commands() {
        if !seen.insert(c.id.clone()) {
            continue;
        }
        v.push(command_item(&c.id, &c.label, &c.menu, super::params(&c.id), short(&c.id), true));
    }
    v
}

/// Every action of the installed plugins (`bashcut.plugins.run`).
pub fn plugin_items(installed: &[Installed]) -> Vec<Item> {
    let lang = super::strings::current();
    let mut v = Vec::new();
    for p in installed {
        let Some(m) = &p.manifest else { continue };
        let plugin = if m.name_in(lang).is_empty() { p.id.as_str() } else { m.name_in(lang) };
        for a in &m.actions {
            let defaults: Map<String, Value> = a.params.iter().filter_map(|x| x.default.clone().map(|d| (x.id.clone(), d))).collect();
            let signature = a
                .params
                .iter()
                .map(|x| format!("{}: {}{}", x.id, x.kind, if x.optional || x.default.is_some() { "?" } else { "" }))
                .collect::<Vec<_>>()
                .join(", ");
            v.push(Item {
                key: format!("plugin:{}/{}", p.id, a.id),
                kind: Kind::Plugin,
                label: format!("{plugin}: {}", if a.title_in(lang).is_empty() { a.id.as_str() } else { a.title_in(lang) }),
                detail: m.summary_in(lang).to_string(),
                shortcut: None,
                enabled: true,
                command: "bashcut.plugins.run".into(),
                params: json!({"plugin": p.id, "action": a.id, "params": defaults}),
                signature: format!("params: {{{signature}}}"),
                needs_params: a.params.iter().any(|x| !x.optional && x.default.is_none()),
            });
        }
    }
    v
}

/// The entries of one catalog's list result.
pub fn catalog_items(c: &Catalog, list: &Value) -> Vec<Item> {
    let text = |e: &Value, k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let signature = filmcraft_engine::commands::find(c.run).map(|s| s.params).unwrap_or("");
    let Some(entries) = list.get(c.array).and_then(Value::as_array) else { return Vec::new() };
    entries
        .iter()
        .take(1000)
        .filter_map(|e| {
            let id = text(e, c.id);
            if id.is_empty() {
                return None;
            }
            let name = text(e, c.name);
            let mut params = Map::new();
            params.insert(c.param.to_string(), Value::String(id.clone()));
            Some(Item {
                key: format!("{}:{id}", c.kind.label().to_lowercase()),
                kind: c.kind,
                label: if name.is_empty() { id.clone() } else { name },
                detail: text(e, c.detail),
                shortcut: None,
                enabled: true,
                command: c.run.into(),
                params: Value::Object(params),
                signature: signature.into(),
                needs_params: c.needs_params,
            })
        })
        .collect()
}

pub fn recipe_items() -> Vec<Item> {
    RECIPES
        .iter()
        .map(|(id, title, purpose)| Item {
            key: format!("recipe:{id}"),
            kind: Kind::Recipe,
            label: s(title).to_string(),
            detail: format!("{} · bcp:{id}", s(purpose)),
            shortcut: None,
            enabled: true,
            command: "bashcut.palette.recipe".into(),
            params: json!({"recipe": id}),
            signature: String::new(),
            needs_params: false,
        })
        .collect()
}

/// Everything the palette lists right now.
pub fn collect(app: &mut FilmcraftApp, ctx: &egui::Context) -> Vec<Item> {
    let mut v = command_items(&app.session);
    v.extend(super::plugins::with(app, ctx, |h| plugin_items(h.installed())));
    for c in CATALOGS {
        if filmcraft_engine::commands::find(c.list).is_none() {
            continue;
        }
        match app.session.execute(c.list, json!({})) {
            Ok(list) => v.extend(catalog_items(c, &list)),
            Err(e) => log::warn!("palette {}: {e}", c.list),
        }
    }
    v.extend(recipe_items());
    v
}

// ---------------------------------------------------------------- ranking

/// How well `query` matches `text` (case-insensitive): a substring beats a scattered
/// subsequence, earlier and at a word start is better. `None` when it does not match.
pub fn score(query: &str, text: &str) -> Option<i64> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let t = text.to_lowercase();
    let len = t.chars().count().min(200) as i64;
    if let Some(pos) = t.find(&q) {
        let word = t[..pos].chars().last().is_none_or(|c| !c.is_alphanumeric());
        return Some(1000 - (pos.min(200) as i64) + if word { 300 } else { 0 } - len / 4);
    }
    let q: Vec<char> = q.chars().filter(|c| !c.is_whitespace()).collect();
    let mut qi = 0;
    let mut sc = 0i64;
    let mut run = false;
    let mut prev: Option<char> = None;
    for c in t.chars() {
        if q.get(qi) == Some(&c) {
            sc += 10;
            if run {
                sc += 15;
            }
            if prev.is_none_or(|p| !p.is_alphanumeric()) {
                sc += 20;
            }
            qi += 1;
            run = true;
        } else {
            run = false;
            sc -= 1;
        }
        prev = Some(c);
    }
    (qi == q.len()).then_some(sc)
}

/// Indices of the items matching `query`, best first; recent items rank higher, disabled ones
/// lower. An empty query lists recent items first, then the rest in order.
pub fn rank(items: &[Item], query: &str, recent: &[String], limit: usize) -> Vec<usize> {
    let mut scored: Vec<(i64, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let label = score(query, &it.label).map(|x| x + 50);
            let best = [label, score(query, &it.command), score(query, &it.detail).map(|x| x / 2)].into_iter().flatten().max()?;
            let recent_bonus = recent.iter().position(|k| *k == it.key).map_or(0, |r| 400 - 10 * r.min(30) as i64);
            let disabled = if it.enabled { 0 } else { 200 };
            Some((best + recent_bonus - disabled, i))
        })
        .collect();
    scored.sort_by_key(|(sc, i)| (std::cmp::Reverse(*sc), *i));
    scored.into_iter().take(limit).map(|(_, i)| i).collect()
}

// ---------------------------------------------------------------- recent use

/// `<data dir>/bashcut/palette.json`.
pub fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("bashcut").join("palette.json")
}

fn load_recent(dir: Option<&Path>) -> Vec<String> {
    let Some(dir) = dir else { return Vec::new() };
    let Some(text) = super::prefs::read(&file(dir), MAX_FILE) else { return Vec::new() };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let list = v.get("recent").and_then(Value::as_array).cloned().unwrap_or_default();
    list.iter().filter_map(Value::as_str).filter(|k| k.len() <= 512).take(MAX_RECENT).map(str::to_string).collect()
}

fn save_recent(dir: Option<&Path>, recent: &[String]) {
    let Some(dir) = dir else { return };
    let text = serde_json::to_string_pretty(&json!({"recent": recent})).unwrap_or_default();
    super::prefs::write(&file(dir), &text, "bashcut palette");
}

/// Move `key` to the front of the recent list.
pub fn remember(recent: &mut Vec<String>, key: &str) {
    recent.retain(|k| k != key);
    recent.insert(0, key.to_string());
    recent.truncate(MAX_RECENT);
}

// ---------------------------------------------------------------- running

/// Run an item through the normal command path; on success it becomes the most recent item.
pub fn run(app: &mut FilmcraftApp, ctx: &egui::Context, item: &Item, params: Option<Value>) -> Result<Value, String> {
    if !item.enabled {
        return Err(f("{label} is not available now", &[("label", item.label.as_str())]));
    }
    let params = params.unwrap_or_else(|| item.params.clone());
    let r = crate::menus::invoke(app, ctx, &item.command, params)?;
    let dir = super::data_dir(app);
    let mut p = get(ctx);
    if !p.loaded {
        p.recent = load_recent(dir.as_deref());
        p.loaded = true;
    }
    remember(&mut p.recent, &item.key);
    save_recent(dir.as_deref(), &p.recent);
    put(ctx, p);
    Ok(r)
}

/// An engine error as a person reads it: no error code or command id
/// (`badParams: invalid parameters for \`bashcut.looks.apply\`: select clips first` → "Select a clip in the timeline first.").
pub fn plain_error(e: &str) -> String {
    let mut m = e.trim();
    if let Some((code, rest)) = m.split_once(": ")
        && !code.is_empty()
        && code.chars().all(|c| c.is_ascii_alphanumeric())
    {
        m = rest;
    }
    for (prefix, sep) in [("invalid parameters for `", "`: "), ("command `", "` is not available right now: ")] {
        if let Some((_, msg)) = m.strip_prefix(prefix).and_then(|rest| rest.split_once(sep)) {
            m = msg;
        }
    }
    if m == "select clips first" {
        return s("Select a clip in the timeline first.").to_string();
    }
    let mut out: String = m.chars().take(1).flat_map(char::to_uppercase).chain(m.chars().skip(1)).collect();
    if !out.is_empty() && !out.ends_with(['.', '!', '?', ')']) {
        out.push('.');
    }
    out
}

fn open(app: &mut FilmcraftApp, ctx: &egui::Context, query: Option<&str>) {
    let items = Arc::new(collect(app, ctx));
    let dir = super::data_dir(app);
    let mut p = get(ctx);
    if !p.loaded {
        p.recent = load_recent(dir.as_deref());
        p.loaded = true;
    }
    p.open = true;
    p.focus = true;
    p.query = query.unwrap_or("").to_string();
    p.selected = 0;
    p.editing = None;
    p.error = None;
    p.items = items;
    put(ctx, p);
}

fn close(ctx: &egui::Context) {
    let mut p = get(ctx);
    p.open = false;
    p.editing = None;
    p.error = None;
    put(ctx, p);
}

/// `bashcut.palette.<name>`.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, name: &str, params: &Value) -> Result<Value, String> {
    let text = |k: &str| params.get(k).and_then(Value::as_str);
    match name {
        "open" => {
            open(app, ctx, text("query"));
            Ok(json!({"open": true, "items": get(ctx).items.len()}))
        }
        "close" => {
            close(ctx);
            Ok(Value::Null)
        }
        "search" => {
            let query = text("query").unwrap_or("");
            let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(20).clamp(1, MAX_ROWS as u64) as usize;
            let items = collect(app, ctx);
            let mut p = get(ctx);
            if !p.loaded {
                p.recent = load_recent(super::data_dir(app).as_deref());
                p.loaded = true;
            }
            if p.open {
                p.query = query.to_string();
                p.selected = 0;
            }
            let found: Vec<Value> = rank(&items, query, &p.recent, limit).into_iter().filter_map(|i| items.get(i)).map(|it| json!(it)).collect();
            put(ctx, p);
            Ok(json!(found))
        }
        "run" => {
            let want = text("item").ok_or("`item` (a key from bashcut.palette.search) is required")?;
            let items = collect(app, ctx);
            let item = items.iter().find(|it| it.key == want).cloned().ok_or(format!("no palette item {want}"))?;
            close(ctx);
            run(app, ctx, &item, params.get("params").filter(|v| !v.is_null()).cloned())
        }
        "recipe" => {
            let want = text("recipe").ok_or("`recipe` is required")?;
            let (id, ..) = RECIPES.iter().find(|(id, ..)| *id == want).ok_or(format!("no recipe {want}"))?;
            let prompt = format!("Use the bcp:{id} recipe on this project.");
            ctx.copy_text(prompt.clone());
            super::State::update(ctx, |st| st.show(super::AGENT));
            Ok(json!({"recipe": id, "prompt": prompt, "copied": true}))
        }
        _ => Err(format!("unknown command bashcut.palette.{name}")),
    }
}

pub fn commands() -> Vec<CommandInfo> {
    vec![
        // Cmd+K is upstream's Add Edit; Cmd+Shift+P is free in every built-in preset
        CommandInfo::new("bashcut.palette.open", "Command Palette…", &["Window", "BashCutPro"], Some("Cmd+Shift+P")),
        CommandInfo::new("bashcut.palette.close", "Close Command Palette", &[], None),
        CommandInfo::new("bashcut.palette.search", "Search Command Palette", &[], None),
        CommandInfo::new("bashcut.palette.run", "Run from Command Palette", &[], None),
        CommandInfo::new("bashcut.palette.recipe", "Start Recipe with the Agent", &[], None),
    ]
}

// ---------------------------------------------------------------- drawing

/// What the overlay asked for this frame.
enum Act {
    Run(Item, Option<Value>),
    Edit(Item),
}

/// Draw the palette if it is open (called by `layout` every frame, in every mode).
pub fn show(app: &mut FilmcraftApp, ctx: &egui::Context) {
    let mut p = get(ctx);
    if !p.open {
        return;
    }
    let editing = p.editing.is_some();
    let (up, down, enter, cmd_enter, tab, esc) = ctx.input_mut(|i| {
        let nav = |i: &mut egui::InputState, k: Key| !editing && i.consume_key(Modifiers::NONE, k);
        (
            nav(i, Key::ArrowUp),
            nav(i, Key::ArrowDown),
            nav(i, Key::Enter),
            i.consume_key(Modifiers::COMMAND, Key::Enter),
            nav(i, Key::Tab),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    if esc {
        if editing {
            p.editing = None;
            p.error = None;
            p.focus = true;
        } else {
            p.open = false;
            put(ctx, p);
            return;
        }
    }
    let shown = rank(&p.items, &p.query, &p.recent, MAX_ROWS);
    if down {
        p.selected = (p.selected + 1).min(shown.len().saturating_sub(1));
    }
    if up {
        p.selected = p.selected.saturating_sub(1);
    }
    p.scroll |= up || down;
    p.selected = p.selected.min(shown.len().saturating_sub(1));
    let current = shown.get(p.selected).and_then(|i| p.items.get(*i)).cloned();

    let mut act: Option<Act> = None;
    if !editing && let Some(it) = &current {
        if tab || (enter && it.needs_params) {
            act = Some(Act::Edit(it.clone()));
        } else if enter {
            act = Some(Act::Run(it.clone(), None));
        }
    }
    let area = egui::Area::new(key().with("area")).order(egui::Order::Foreground).anchor(Align2::CENTER_TOP, vec2(0.0, 72.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(WIDTH);
            if let Some(it) = p.editing.clone() {
                if let Some(a) = params_editor(app, ui, &mut p, &it, cmd_enter) {
                    act = Some(a);
                }
            } else if let Some(a) = list(app, ui, &mut p, &shown) {
                act = Some(a);
            }
            if let Some(e) = &p.error {
                ui.colored_label(app.tokens.danger, e.as_str());
            }
        });
    });
    app.auto.add("bashcut.palette", area.response.rect, s("Command Palette"));
    let clicked_outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pos| !area.response.rect.contains(pos)));
    if clicked_outside {
        p.open = false;
    }
    match act {
        Some(Act::Edit(it)) => {
            p.params = serde_json::to_string_pretty(&template(&it)).unwrap_or_else(|_| "{}".into());
            p.editing = Some(it);
            p.error = None;
            p.focus = true;
            put(ctx, p);
        }
        Some(Act::Run(it, params)) => {
            // close first: the command may itself change the palette (e.g. open it again)
            let back = p.clone();
            p.open = false;
            p.editing = None;
            p.error = None;
            put(ctx, p);
            if let Err(e) = run(app, ctx, &it, params) {
                let e = plain_error(&e);
                let mut again = back;
                again.recent = get(ctx).recent;
                again.error = Some(e.clone());
                put(ctx, again);
                app.ui.status = e;
            }
        }
        None => put(ctx, p),
    }
}

/// The search field and the ranked rows; a click runs a row (or opens its parameters).
fn list(app: &mut FilmcraftApp, ui: &mut egui::Ui, p: &mut Palette, shown: &[usize]) -> Option<Act> {
    let t = app.tokens;
    let before = p.query.clone();
    let field =
        ui.add(egui::TextEdit::singleline(&mut p.query).hint_text(s("Search commands, actions, looks, profiles, recipes…")).desired_width(f32::INFINITY));
    app.auto.add_response("bashcut.palette.input", &field, &p.query);
    if p.focus {
        field.request_focus();
        p.focus = false;
    }
    if p.query != before {
        p.selected = 0;
    }
    ui.add_space(4.0);
    if shown.is_empty() {
        ui.label(egui::RichText::new(s("Nothing matches.")).color(t.text_dim));
        return None;
    }
    let mut act = None;
    let items = p.items.clone();
    let scroll = std::mem::take(&mut p.scroll);
    egui::ScrollArea::vertical().max_height(ROW * 14.0).auto_shrink([false, true]).show(ui, |ui| {
        for (row, i) in shown.iter().enumerate() {
            let Some(it) = items.get(*i) else { continue };
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let selected = row == p.selected;
            if selected {
                ui.painter().rect_filled(r, t.radius_sm, t.row_selected);
                if scroll {
                    ui.scroll_to_rect(r, None);
                }
            } else if resp.hovered() {
                ui.painter().rect_filled(r, t.radius_sm, t.hover);
            }
            paint_row(ui, r, it, &t);
            app.auto.add_response(&format!("bashcut.palette.item.{}", it.key), &resp, &it.label);
            if resp.clicked() {
                p.selected = row;
                act = Some(if it.needs_params { Act::Edit(it.clone()) } else { Act::Run(it.clone(), None) });
            }
        }
    });
    ui.add_space(2.0);
    note(ui, s("↑↓ choose · Enter run · Tab parameters · Esc close"), META, t.text_faint);
    act
}

fn paint_row(ui: &egui::Ui, r: Rect, it: &Item, t: &Tokens) {
    let painter = ui.painter();
    let y = r.center().y;
    let tag = s(it.kind.label());
    painter.text(pos2(r.min.x + 6.0, y), Align2::LEFT_CENTER, tag, egui::FontId::proportional(10.0), t.text_faint);
    let color = if it.enabled { t.text } else { t.text_faint };
    let label = painter.text(pos2(r.min.x + 80.0, y), Align2::LEFT_CENTER, &it.label, egui::FontId::proportional(12.5), color);
    let right = match &it.shortcut {
        Some(k) => {
            // clear of the scroll bar, which floats over the rows
            let bar = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_outer_margin;
            painter.text(pos2(r.max.x - 6.0 - bar, y), Align2::RIGHT_CENTER, k, egui::FontId::proportional(11.0), t.text_dim).min.x
        }
        None => r.max.x,
    };
    let room = right - label.max.x - 16.0;
    if room > 40.0 && !it.detail.is_empty() {
        let max_chars = (room / 6.0) as usize;
        let detail: String = if it.detail.chars().count() > max_chars {
            it.detail.chars().take(max_chars.saturating_sub(1)).chain(['…']).collect()
        } else {
            it.detail.clone()
        };
        painter.text(pos2(label.max.x + 10.0, y), Align2::LEFT_CENTER, detail, egui::FontId::proportional(10.5), t.text_dim);
    }
}

/// The JSON parameter editor for one item; Cmd/Ctrl+Enter or Run runs it, Esc goes back.
fn params_editor(app: &mut FilmcraftApp, ui: &mut egui::Ui, p: &mut Palette, it: &Item, cmd_enter: bool) -> Option<Act> {
    let t = app.tokens;
    ui.label(egui::RichText::new(&it.label).font(Tokens::semibold(13.0)).color(t.text));
    note(ui, &it.command, META, t.text_dim);
    if !it.signature.is_empty() {
        code_note(ui, &it.signature, META, t.text_dim);
    }
    ui.add_space(4.0);
    let field = ui.add(egui::TextEdit::multiline(&mut p.params).code_editor().desired_rows(6).desired_width(f32::INFINITY));
    app.auto.add_response("bashcut.palette.params", &field, &it.key);
    if p.focus {
        field.request_focus();
        p.focus = false;
    }
    let mut go = cmd_enter;
    ui.horizontal(|ui| {
        let run = ui.button(s("Run"));
        app.auto.add_response("bashcut.palette.params.run", &run, s("Run"));
        go |= run.clicked();
        let back = ui.button(s("Back"));
        app.auto.add_response("bashcut.palette.params.back", &back, s("Back"));
        if back.clicked() {
            p.editing = None;
            p.error = None;
            p.focus = true;
        }
        note(ui, s("Cmd/Ctrl+Enter run · Esc back"), META, t.text_faint);
    });
    if !go {
        return None;
    }
    match serde_json::from_str::<Value>(&p.params) {
        Ok(v @ Value::Object(_)) => Some(Act::Run(it.clone(), Some(v))),
        Ok(_) => {
            p.error = Some(s("Parameters must be a JSON object.").to_string());
            None
        }
        Err(e) => {
            p.error = Some(e.to_string());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bashcut_plugin_host::manifest::{Action, Manifest, Param};

    fn item(key: &str, label: &str, enabled: bool) -> Item {
        let id = key.trim_start_matches("command:");
        let mut it = command_item(id, label, &[], "{}", None, enabled);
        it.key = key.into();
        it
    }

    #[test]
    fn kind_column_is_translated() {
        use super::super::strings::set_language;
        set_language(Some("vi"));
        let vi: Vec<&str> = Kind::ALL.iter().map(|k| s(k.label())).collect();
        set_language(None);
        assert_eq!(vi, ["Lệnh", "BashCutPro", "Plugin", "Màu phim", "Hồ sơ xuất", "Mẫu", "Công thức"]);
    }

    #[test]
    fn engine_errors_read_plainly() {
        assert_eq!(plain_error("badParams: invalid parameters for `bashcut.looks.apply`: select clips first"), "Select a clip in the timeline first.");
        assert_eq!(plain_error("disabled: command `edit.cut` is not available right now: nothing selected"), "Nothing selected.");
        assert_eq!(plain_error("no active sequence"), "No active sequence.");
        assert_eq!(plain_error("Teal & Orange is not available now"), "Teal & Orange is not available now.");
        assert_eq!(plain_error(""), "");
    }

    #[test]
    fn required_keys_from_signatures() {
        assert_eq!(required_keys(r#"{"time":ticks,"track":"V1"|id?,"clip":id?}"#), vec!["time"]);
        assert!(!needs_params(r#"{"seconds":f64? (estimate),"dir":str?}"#));
        assert!(!needs_params("{}"));
        assert!(!needs_params(""));
        assert_eq!(required_keys(r#"{"template":str (folder or name),"fills":[{"slot":str,"file":str}],"saveAs":str?}"#), vec!["template", "fills"]);
        assert!(!needs_params(r#"{"amount":0..1=0.5,"loudness":bool=true}"#));
    }

    #[test]
    fn rail_commands_say_what_to_type() {
        let sections = super::super::section_ids().join("|");
        for id in ["bashcut.ui.section", "bashcut.ui.popOut", "bashcut.ui.dockBack"] {
            assert!(super::super::params(id).contains(&sections), "{id} lists every section");
        }
        assert_eq!(required_keys(super::super::params("bashcut.ui.dockBack")), vec!["section"]);
        assert_eq!(required_keys(super::super::params("bashcut.ui.setWidth")), vec!["width"]);
        assert!(super::super::params("bashcut.ui.setLanguage").contains(&super::super::strings::LANGUAGES.join("|")));
        assert!(!needs_params(super::super::params("bashcut.ui.setLanguage")), "no language goes back to the app's");
    }

    #[test]
    fn fuzzy_scores_prefer_substrings_and_word_starts() {
        assert!(score("razor", "Razor").is_some());
        assert!(score("rzr", "Razor").is_some());
        assert!(score("xyz", "Razor").is_none());
        assert!(score("raz", "Razor").unwrap() > score("rzr", "Razor").unwrap());
        assert!(score("edit", "Add Edit").unwrap() > score("dit", "Add Edit").unwrap());
        assert_eq!(score("", "anything"), Some(0));
        assert!(score("ệ", "Hiệu ứng").is_some(), "non-ASCII text never panics");
    }

    #[test]
    fn ranking_uses_recent_and_enabled_state() {
        let items = vec![item("command:a", "Razor Tool", true), item("command:b", "Razor", true), item("command:c", "Razor All", false)];
        let r = rank(&items, "razor", &[], 10);
        assert_eq!(r.len(), 3);
        assert_eq!(r.last(), Some(&2), "disabled last");
        let r = rank(&items, "razor", &["command:a".into()], 10);
        assert_eq!(r.first(), Some(&0), "recent first");
        assert_eq!(rank(&items, "", &["command:c".into()], 10).first(), Some(&2));
        assert_eq!(rank(&items, "razor", &[], 1).len(), 1);
        let mut recent = vec!["x".to_string(), "y".to_string()];
        remember(&mut recent, "y");
        assert_eq!(recent, vec!["y", "x"]);
    }

    #[test]
    fn engine_commands_are_listed_with_shortcuts() {
        let s = filmcraft_engine::Session::default();
        let items = command_items(&s);
        let razor = items.iter().find(|i| i.key == "command:timeline.razor").expect("razor listed");
        assert!(razor.needs_params);
        let add = items.iter().find(|i| i.key == "command:sequence.addEdit").expect("add edit listed");
        assert!(add.shortcut.is_some());
        assert!(items.iter().any(|i| i.key == "command:bashcut.palette.open"), "frontend commands too");
        let found = rank(&items, "razor", &[], 10);
        assert!(found.iter().any(|i| items[*i].command == "timeline.razor"));
    }

    #[test]
    fn plugin_actions_looks_and_recipes_are_searchable() {
        let manifest = Manifest {
            id: "bashcut.vieneu-tts".into(),
            name: "VieNeu TTS".into(),
            summary: "Vietnamese text to speech".into(),
            actions: vec![Action {
                id: "speak".into(),
                title: "Speak a line".into(),
                params: vec![
                    Param { id: "text".into(), kind: "string".into(), ..Param::default() },
                    Param { id: "voice".into(), kind: "string".into(), default: Some(json!("female")), ..Param::default() },
                ],
                ..Action::default()
            }],
            ..Manifest::default()
        };
        let installed = vec![Installed { id: "bashcut.vieneu-tts".into(), dir: PathBuf::new(), manifest: Some(manifest), origin: None, error: None }];
        let mut items = plugin_items(&installed);
        assert_eq!(items.len(), 1);
        assert!(items[0].needs_params, "text has no default");
        assert_eq!(items[0].params, json!({"plugin": "bashcut.vieneu-tts", "action": "speak", "params": {"voice": "female"}}));

        let looks = json!({"looks": [{"id": "teal-orange", "name": "Teal & Orange", "purpose": "Warm skin against cool shadows"}, {"name": "no id"}]});
        items.extend(catalog_items(&CATALOGS[0], &looks));
        assert_eq!(items.len(), 2, "an entry without id is skipped");
        items.extend(recipe_items());

        let speak = rank(&items, "speak", &[], 5);
        assert_eq!(items[speak[0]].key, "plugin:bashcut.vieneu-tts/speak");
        let teal = rank(&items, "teal", &[], 5);
        assert_eq!(items[teal[0]].key, "look:teal-orange");
        assert_eq!(items[teal[0]].params, json!({"look": "teal-orange"}));
        let travel = rank(&items, "travel", &[], 5);
        assert_eq!(items[travel[0]].key, "recipe:vlog-travel");
        assert_eq!(template(&items[0]), items[0].params);
    }

    #[test]
    fn recent_survives_a_restart_and_hostile_files() {
        let dir = std::env::temp_dir().join(format!("bashcut-palette-{}", std::process::id()));
        save_recent(Some(&dir), &["command:timeline.razor".into()]);
        assert_eq!(load_recent(Some(&dir)), vec!["command:timeline.razor"]);
        let _ = std::fs::write(file(&dir), "[1,2");
        assert!(load_recent(Some(&dir)).is_empty());
        let _ = std::fs::write(file(&dir), r#"{"recent":[1,"ok",null]}"#);
        assert_eq!(load_recent(Some(&dir)), vec!["ok"]);
        assert!(load_recent(None).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn command_labels_name_bashcutpro() {
        use super::super::strings::set_language;
        let session = filmcraft_engine::Session::default();
        let label = |items: &[Item], id: &str| items.iter().find(|i| i.command == id).map(|i| i.label.clone()).unwrap_or_else(|| panic!("{id} listed"));
        let named = |items: &[Item]| items.iter().filter(|i| i.label.contains("FilmCraft")).map(|i| format!("{}: {}", i.command, i.label)).collect::<Vec<_>>();

        set_language(Some("en"));
        let en = command_items(&session);
        set_language(Some("vi"));
        let vi = command_items(&session);
        set_language(None);

        assert_eq!(label(&en, "file.exportSelectionProject"), "Selection as BashCutPro Project…");
        assert_eq!(label(&en, "help.filmcraftHelp"), "BashCutPro Help…");
        assert_eq!(label(&vi, "file.exportSelectionProject"), "Vùng chọn thành dự án BashCutPro…");
        assert_eq!(label(&vi, "help.filmcraftHelp"), "Trợ giúp BashCutPro…");
        // ours in the UI language (AG11's rows)
        assert_eq!(label(&en, "bashcut.agent.installKit"), "Install BashCutPro Kit");
        assert_ne!(label(&vi, "bashcut.agent.installKit"), "Install BashCutPro Kit");
        assert_eq!(named(&en), Vec::<String>::new(), "English labels that name FilmCraft");
        assert_eq!(named(&vi), Vec::<String>::new(), "Vietnamese labels that name FilmCraft");
    }
}
