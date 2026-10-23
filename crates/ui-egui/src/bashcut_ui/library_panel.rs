//! The Library section (#68): **Online** searches open music, sound effects, stock photos and
//! video, icons and fonts through the resources plugin (`bashcut.resources`, `search` and
//! `download`) and **Mine** searches the creator's own library (`bashcut.library`).
//!
//! Every result shows its licence and source before anything is downloaded. Add to bin / Place at
//! playhead downloads the file with its licence record (plugin), then imports and places it with
//! the app's own commands (`file.import`, `timeline.place` on a new track, so nothing is
//! overwritten), so it works without the control channel. Credit lines go into the project notes
//! only when the user turns credits on (owner rule: opt-in). Searches and downloads are plugin runs
//! (they show in Runs); results are cached per query. Native egui for now: plugin views
//! (`bashcut-view/1`, #37) can wrap it later. State lives in egui context data; the credits switch
//! in `<data dir>/bashcut/library-ui.json`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use bashcut_plugin_host::JobState;
use filmcraft_engine::shortcuts::CommandInfo;
use serde_json::{Value, json};

use super::plugins;
use super::strings::{f, s};
use super::widgets::{META, NOTE, note, small_button};
use crate::FilmcraftApp;

pub const RESOURCES: &str = "bashcut.resources";
pub const LIBRARY: &str = "bashcut.library";
/// Online kinds: (id, English label).
pub const KINDS: [(&str, &str); 6] =
    [("music", "Music"), ("sfx", "Sound effects"), ("photo", "Photos"), ("video", "Video"), ("icon", "Icons"), ("font", "Fonts")];
/// The library's kinds the app places itself (a file on the timeline); the rest go through the plugin's `apply`.
const MEDIA_KINDS: [&str; 4] = ["music", "sound", "sticker", "overlay"];
/// "Mine" kinds: (id, English label); "" is every kind.
pub const MINE_KINDS: [(&str, &str); 12] = [
    ("", "Everything"),
    ("music", "Music"),
    ("sound", "Sounds"),
    ("sticker", "Stickers"),
    ("overlay", "Overlays"),
    ("title", "Titles"),
    ("text-style", "Text styles"),
    ("look", "Looks"),
    ("lut", "LUTs"),
    ("effect-preset", "Effect presets"),
    ("template", "Templates"),
    ("brand-kit", "Brand kits"),
];
const MAX_CACHE: usize = 32;
const MAX_ITEMS: usize = 100;
const RUN_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_FILE: u64 = 16 * 1024;
/// How long a done message ("Added …") stays once nothing runs; errors stay until the next action (#168).
const STATUS_SECONDS: f64 = 6.0;
pub const CREDITS_START: &str = "<!-- bashcut.resources credits -->";
pub const CREDITS_END: &str = "<!-- /bashcut.resources credits -->";

#[derive(Clone, Debug, PartialEq)]
struct Download {
    job: u64,
    item: Value,
    place: bool,
}

#[derive(Clone)]
struct Panel {
    online: bool,
    kind: String,
    query: String,
    mine_kind: String,
    mine_query: String,
    /// Credit lines into the project notes (off unless the user turns it on).
    credits: bool,
    /// Running searches: (job, cache key).
    searches: Vec<(u64, String)>,
    downloads: Vec<Download>,
    /// Cache key → result items, oldest first in `order`.
    results: HashMap<String, Vec<Value>>,
    order: Vec<String>,
    shown: Option<String>,
    mine_shown: Option<String>,
    status: Option<(bool, String)>,
    /// The done message on show and when it was first drawn (egui time), to clear it.
    status_since: Option<(String, f64)>,
    loaded: bool,
    saved: Option<String>,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            online: true,
            kind: "music".into(),
            query: String::new(),
            mine_kind: String::new(),
            mine_query: String::new(),
            credits: false,
            searches: Vec::new(),
            downloads: Vec::new(),
            results: HashMap::new(),
            order: Vec::new(),
            shown: None,
            mine_shown: None,
            status: None,
            status_since: None,
            loaded: false,
            saved: None,
        }
    }
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.library.panel")
}

fn get(ctx: &egui::Context) -> Panel {
    super::shared::temp::<Panel>(ctx, key())
}

fn put(ctx: &egui::Context, p: Panel) {
    super::shared::put_temp(ctx, key(), p);
}

/// `<data dir>/bashcut/library-ui.json`.
pub fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("bashcut").join("library-ui.json")
}

fn load(p: &mut Panel, dir: Option<&Path>) {
    if p.loaded {
        return;
    }
    p.loaded = true;
    let Some(dir) = dir else { return };
    let Some(text) = super::prefs::read(&file(dir), MAX_FILE) else { return };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    p.credits = v.get("credits").and_then(Value::as_bool).unwrap_or(false);
    p.saved = Some(json!({"credits": p.credits}).to_string());
}

fn save(p: &mut Panel, dir: Option<&Path>) {
    let Some(dir) = dir else { return };
    super::prefs::save_if_changed(&file(dir), json!({"credits": p.credits}).to_string(), &mut p.saved, "bashcut library");
}

/// A Mine row's kind tag: the kind's translated label, or the raw id for a kind the app doesn't know.
fn mine_kind_label(kind: &str) -> &str {
    MINE_KINDS.iter().find(|(id, _)| !id.is_empty() && *id == kind).map_or(kind, |(_, label)| s(label))
}

fn cache_key(online: bool, kind: &str, query: &str) -> String {
    format!("{}|{kind}|{}", if online { "online" } else { "mine" }, query.trim().to_lowercase())
}

fn remember(p: &mut Panel, key: String, items: Vec<Value>) {
    p.order.retain(|k| *k != key);
    p.order.push(key.clone());
    p.results.insert(key, items);
    while p.order.len() > MAX_CACHE {
        let old = p.order.remove(0);
        p.results.remove(&old);
    }
}

/// The credits block of the project notes with `line` added (earlier lines kept, sorted, no repeats).
pub fn merge_credits(notes: &str, line: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut rest = notes.to_string();
    if let (Some(a), Some(b)) = (notes.find(CREDITS_START), notes.find(CREDITS_END))
        && a < b
    {
        lines = notes[a..b].lines().filter(|l| l.starts_with("- ")).map(str::to_string).collect();
        let end = b + CREDITS_END.len();
        rest = format!("{}\n\n{}", notes[..a].trim_end_matches('\n'), notes[end..].trim_start_matches('\n')).trim_matches('\n').to_string();
    }
    let line = line.trim();
    if !line.is_empty() {
        lines.push(if line.starts_with("- ") { line.to_string() } else { format!("- {line}") });
    }
    lines.sort();
    lines.dedup();
    let mut block = vec![CREDITS_START.to_string(), "Credits (fetched by bashcut.resources):".to_string()];
    block.extend(lines);
    block.push(CREDITS_END.to_string());
    let block = block.join("\n");
    if rest.is_empty() { format!("{block}\n") } else { format!("{rest}\n\n{block}\n") }
}

/// Start a plugin action; its job id.
fn start(app: &FilmcraftApp, ctx: &egui::Context, plugin: &str, action: &str, params: BTreeMap<String, Value>) -> Result<u64, String> {
    let rc = plugins::run_context();
    plugins::with(app, ctx, |h| h.run(plugin, action, &params, &rc, Some(RUN_TIMEOUT))).map_err(|e| e.0)
}

fn installed(app: &FilmcraftApp, ctx: &egui::Context, plugin: &str) -> bool {
    plugins::with(app, ctx, |h| h.plugin(plugin).is_some_and(|p| p.manifest.is_some()))
}

/// Start a search of the current tab; returns the job.
fn search(app: &FilmcraftApp, ctx: &egui::Context, p: &mut Panel) -> Result<u64, String> {
    let (online, kind, query) = if p.online { (true, p.kind.clone(), p.query.clone()) } else { (false, p.mine_kind.clone(), p.mine_query.clone()) };
    if online && query.trim().is_empty() {
        return Err(s("Type what you are looking for.").to_string());
    }
    let key = cache_key(online, &kind, &query);
    let mut params = BTreeMap::new();
    params.insert("query".to_string(), json!(query.trim()));
    let job = if online {
        params.insert("kind".to_string(), json!(kind));
        params.insert("count".to_string(), json!(20));
        start(app, ctx, RESOURCES, "search", params)?
    } else {
        if !kind.is_empty() {
            params.insert("kind".to_string(), json!(kind));
        }
        start(app, ctx, LIBRARY, "search", params)?
    };
    p.searches.push((job, key.clone()));
    if online {
        p.shown = Some(key);
    } else {
        p.mine_shown = Some(key);
    }
    p.status = None;
    Ok(job)
}

/// Download an online result (then import, and place it when `place`).
fn download(app: &FilmcraftApp, ctx: &egui::Context, p: &mut Panel, item: &Value, place: bool) -> Result<u64, String> {
    let text = |k: &str| item.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let url = text("url");
    if url.is_empty() {
        return Err(s("This result has no file to download.").to_string());
    }
    let mut params = BTreeMap::new();
    for (k, v) in
        [("kind", text("kind")), ("url", url), ("title", text("title")), ("license", text("license")), ("author", text("author")), ("source", text("source"))]
    {
        params.insert(k.to_string(), json!(v));
    }
    let job = start(app, ctx, RESOURCES, "download", params)?;
    p.downloads.push(Download { job, item: item.clone(), place });
    p.status = Some((true, f("Downloading {title}…", &[("title", text("title").as_str())])));
    Ok(job)
}

/// Import `path` (and place it at the playhead on a new track); with credits on, add `credit` to the notes.
fn bring_in(app: &mut FilmcraftApp, ctx: &egui::Context, path: &str, place: bool, credit: Option<&str>, credits: bool) -> Result<Value, String> {
    let r = crate::menus::invoke(app, ctx, "file.import", json!({"paths": [path]}))?;
    let item = r.get("items").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_u64).ok_or_else(|| {
        let why = r.get("errors").map(Value::to_string).unwrap_or_default();
        format!("{} {why}", s("The file was not imported."))
    })?;
    let mut out = json!({"path": path, "item": item});
    if place {
        out["placed"] = crate::menus::invoke(app, ctx, "timeline.place", json!({"item": item, "track": "new", "audioTrack": "new"}))?;
    }
    if credits && let Some(line) = credit.filter(|l| !l.trim().is_empty()) {
        let notes = crate::menus::invoke(app, ctx, "project.notes", json!({}))?;
        let text = merge_credits(notes.get("text").and_then(Value::as_str).unwrap_or(""), line);
        crate::menus::invoke(app, ctx, "project.setNotes", json!({"text": text}))?;
        out["credited"] = json!(true);
    }
    Ok(out)
}

/// Collect finished searches and downloads; finished downloads are imported / placed here.
fn poll(app: &mut FilmcraftApp, ctx: &egui::Context, p: &mut Panel) {
    let mut running = false;
    for (job, key) in std::mem::take(&mut p.searches) {
        let Some(j) = plugins::with(app, ctx, |h| h.job(job).cloned()) else { continue };
        if j.state == JobState::Running {
            running = true;
            p.searches.push((job, key));
            continue;
        }
        match j.outcome.as_ref().and_then(|o| o.result.clone()).filter(|r| r.ok) {
            Some(r) => {
                let items: Vec<Value> = r.data.get("items").and_then(Value::as_array).map(|a| a.iter().take(MAX_ITEMS).cloned().collect()).unwrap_or_default();
                remember(p, key, items);
            }
            None => p.status = Some((false, j.outcome.as_ref().map(|o| o.summary()).unwrap_or_else(|| j.message.clone()))),
        }
    }
    for d in std::mem::take(&mut p.downloads) {
        let Some(j) = plugins::with(app, ctx, |h| h.job(d.job).cloned()) else { continue };
        if j.state == JobState::Running {
            running = true;
            p.downloads.push(d);
            continue;
        }
        let Some(r) = j.outcome.as_ref().and_then(|o| o.result.clone()).filter(|r| r.ok) else {
            p.status = Some((false, j.outcome.as_ref().map(|o| o.summary()).unwrap_or_else(|| j.message.clone())));
            continue;
        };
        let path = r.data.get("path").and_then(Value::as_str).unwrap_or("").to_string();
        let title = d.item.get("title").and_then(Value::as_str).unwrap_or("").to_string();
        if d.item.get("kind").and_then(Value::as_str) == Some("font") {
            p.status = Some((true, f("Font saved to {path}: install it in your system to use it in titles.", &[("path", path.as_str())])));
            continue;
        }
        let credit = r.data.get("credit").and_then(Value::as_str).map(str::to_string);
        p.status = Some(match bring_in(app, ctx, &path, d.place, credit.as_deref(), p.credits) {
            Ok(_) if d.place => (true, f("Placed {title} at the playhead", &[("title", title.as_str())])),
            Ok(_) => (true, f("Added {title} to the project", &[("title", title.as_str())])),
            Err(e) => (false, e),
        });
    }
    if running {
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

/// Clear a done message [`STATUS_SECONDS`] after it first showed, once no search or download runs.
fn expire_status(ctx: &egui::Context, p: &mut Panel) {
    let busy = !p.searches.is_empty() || !p.downloads.is_empty();
    let Some((true, msg)) = p.status.as_ref().filter(|_| !busy) else {
        p.status_since = None;
        return;
    };
    let now = ctx.input(|i| i.time);
    let since = match &p.status_since {
        Some((shown, t)) if shown == msg => *t,
        _ => {
            p.status_since = Some((msg.clone(), now));
            now
        }
    };
    let left = STATUS_SECONDS - (now - since);
    if left <= 0.0 {
        p.status = None;
        p.status_since = None;
    } else {
        ctx.request_repaint_after(Duration::try_from_secs_f64(left).unwrap_or(Duration::from_secs(1)));
    }
}

pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let t = app.tokens;
    let dir = super::data_dir(app);
    let mut p = get(&ctx);
    load(&mut p, dir.as_deref());
    poll(app, &ctx, &mut p);
    expire_status(&ctx, &mut p);

    ui.horizontal(|ui| {
        for (online, label, id) in [(false, s("Mine"), "bashcut.library.tab.mine"), (true, s("Online"), "bashcut.library.tab.online")] {
            let r = ui.selectable_label(p.online == online, label);
            app.auto.add_response(id, &r, label);
            if r.clicked() {
                p.online = online;
            }
        }
    });
    if let Some((ok, msg)) = &p.status {
        let color = if *ok { t.text_dim } else { t.danger };
        let r = ui.label(egui::RichText::new(msg).size(11.0).color(color));
        app.auto.add_response("bashcut.library.status", &r, msg);
    }
    let plugin = if p.online { RESOURCES } else { LIBRARY };
    if !installed(app, &ctx, plugin) {
        let name = if p.online { s("Online Resources") } else { s("Library") };
        note(ui, f("Install the {plugin} plugin to use this tab.", &[("plugin", name)]), NOTE, t.text_dim);
        if small_button(app, ui, "bashcut.library.install", s("Open Plugins"), true) {
            super::State::update(&ctx, |st| st.show(super::PLUGINS));
        }
        put(&ctx, p);
        return;
    }

    // search bar
    let mut go = false;
    if p.online {
        ui.horizontal_wrapped(|ui| {
            for (id, label) in KINDS {
                let r = ui.selectable_label(p.kind == id, egui::RichText::new(s(label)).size(11.0));
                app.auto.add_response(&format!("bashcut.library.kind.{id}"), &r, label);
                if r.clicked() && p.kind != id {
                    p.kind = id.to_string();
                    go = !p.query.trim().is_empty();
                }
            }
        });
    } else {
        egui::ComboBox::from_id_salt("bashcut.library.minekind")
            .selected_text(MINE_KINDS.iter().find(|(id, _)| *id == p.mine_kind).map_or(p.mine_kind.as_str(), |(_, label)| s(label)))
            .show_ui(ui, |ui| {
                for (k, label) in MINE_KINDS {
                    if ui.selectable_label(p.mine_kind == k, s(label)).clicked() {
                        p.mine_kind = k.to_string();
                        go = true;
                    }
                }
            });
    }
    ui.horizontal(|ui| {
        let q = if p.online { &mut p.query } else { &mut p.mine_query };
        let r = ui.add(egui::TextEdit::singleline(q).hint_text(s("Search")).desired_width(ui.available_width() - 70.0));
        let label = q.clone();
        app.auto.add_response("bashcut.library.query", &r, &label);
        go |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        go |= small_button(app, ui, "bashcut.library.search", s("Search"), true);
    });
    if go && let Err(e) = search(app, &ctx, &mut p) {
        p.status = Some((false, e));
    }
    if p.online {
        let r = ui.checkbox(&mut p.credits, egui::RichText::new(s("Credit lines in the project notes")).size(11.0));
        app.auto.add_response("bashcut.library.credits", &r, if p.credits { "on" } else { "off" });
        r.on_hover_text(s("Off by default: every file keeps its licence record either way."));
    }
    if !p.searches.is_empty() {
        ui.horizontal(|ui| {
            ui.spinner();
            note(ui, s("Searching…"), NOTE, t.text_dim);
        });
    }
    ui.add_space(4.0);

    let shown = if p.online { p.shown.clone() } else { p.mine_shown.clone() };
    let items = shown.as_ref().and_then(|k| p.results.get(k)).cloned().unwrap_or_default();
    if shown.is_some() && items.is_empty() && p.searches.is_empty() {
        note(ui, s("Nothing found."), NOTE, t.text_dim);
    }
    let has_seq = app.session.active_sequence().is_some();
    for (i, item) in items.iter().enumerate() {
        if p.online {
            online_row(app, ui, &ctx, &mut p, i, item, has_seq);
        } else {
            mine_row(app, ui, &ctx, &mut p, item, has_seq);
        }
    }
    save(&mut p, dir.as_deref());
    put(&ctx, p);
}

fn text(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
        _ => String::new(),
    }
}

fn online_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, p: &mut Panel, i: usize, item: &Value, has_seq: bool) {
    let t = app.tokens;
    let id = format!("bashcut.library.online.{i}");
    let r = egui::Frame::new().inner_margin(egui::Margin::same(4)).corner_radius(t.radius_sm).fill(t.panel_bg).show(ui, |ui| {
        let title = text(item, "title");
        ui.label(egui::RichText::new(if title.is_empty() { s("Untitled") } else { title.as_str() }).strong().color(t.text));
        let mut facts = Vec::new();
        for k in ["author", "seconds", "size", "from"] {
            let v = text(item, k);
            if !v.is_empty() {
                facts.push(match k {
                    "author" => f("by {author}", &[("author", v.as_str())]),
                    "seconds" => f("{seconds} s", &[("seconds", v.as_str())]),
                    _ => v,
                });
            }
        }
        if !facts.is_empty() {
            note(ui, facts.join(" · "), META, t.text_dim);
        }
        let licence = text(item, "license");
        let (lic, color) = if licence.is_empty() {
            (s("Licence unknown: check before publishing").to_string(), t.danger)
        } else {
            (f("Licence: {licence}", &[("licence", licence.as_str())]), t.accent)
        };
        let lr = ui.label(egui::RichText::new(lic).size(10.5).color(color));
        app.auto.add_response(&format!("{id}.licence"), &lr, &licence);
        ui.horizontal_wrapped(|ui| {
            let preview = text(item, "preview");
            let preview = if preview.is_empty() { text(item, "url") } else { preview };
            if small_button(app, ui, &format!("{id}.preview"), s("Preview"), !preview.is_empty()) {
                ctx.open_url(egui::OpenUrl::new_tab(preview));
            }
            let source = text(item, "source");
            if !source.is_empty() && small_button(app, ui, &format!("{id}.source"), s("Source"), true) {
                ctx.open_url(egui::OpenUrl::new_tab(source));
            }
            let is_font = text(item, "kind") == "font";
            let busy = p.downloads.iter().any(|d| d.item == *item);
            if small_button(app, ui, &format!("{id}.add"), if is_font { s("Download") } else { s("Add to bin") }, !busy)
                && let Err(e) = download(app, ctx, p, item, false)
            {
                p.status = Some((false, e));
            }
            if !is_font
                && small_button(app, ui, &format!("{id}.place"), s("Place at playhead"), has_seq && !busy)
                && let Err(e) = download(app, ctx, p, item, true)
            {
                p.status = Some((false, e));
            }
        });
    });
    app.auto.add(&id, r.response.rect, &text(item, "title"));
}

fn mine_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, p: &mut Panel, item: &Value, has_seq: bool) {
    let t = app.tokens;
    let item_id = text(item, "id");
    let id = format!("bashcut.library.mine.{item_id}");
    let r = egui::Frame::new().inner_margin(egui::Margin::same(4)).corner_radius(t.radius_sm).fill(t.panel_bg).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(text(item, "title")).strong().color(t.text));
            ui.label(egui::RichText::new(mine_kind_label(&text(item, "kind"))).size(10.5).color(t.text_faint));
            let fav = item.get("favourite").and_then(Value::as_bool).unwrap_or(false);
            if small_button(app, ui, &format!("{id}.favourite"), if fav { "★" } else { "☆" }, true) {
                let mut params = BTreeMap::new();
                params.insert("id".to_string(), json!(item_id));
                params.insert("off".to_string(), json!(fav));
                if let Err(e) = start(app, ctx, LIBRARY, "favourite", params) {
                    p.status = Some((false, e));
                }
            }
        });
        let purpose = text(item, "purpose");
        if !purpose.is_empty() {
            note(ui, purpose, NOTE, t.text_dim);
        }
        let licence = text(item, "license");
        if !licence.is_empty() {
            ui.label(egui::RichText::new(f("Licence: {licence}", &[("licence", licence.as_str())])).size(10.5).color(t.text_faint));
        }
        let kind = text(item, "kind");
        let file = item.get("paths").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).map(str::to_string);
        if MEDIA_KINDS.contains(&kind.as_str())
            && let Some(file) = file
        {
            if small_button(app, ui, &format!("{id}.place"), s("Place at playhead"), has_seq) {
                p.status = Some(match bring_in(app, ctx, &file, true, None, false) {
                    Ok(_) => (true, f("Placed {title} at the playhead", &[("title", text(item, "title").as_str())])),
                    Err(e) => (false, e),
                });
            }
        } else if small_button(app, ui, &format!("{id}.apply"), s("Apply"), has_seq) {
            let mut params = BTreeMap::new();
            params.insert("id".to_string(), json!(item_id));
            p.status = Some(match start(app, ctx, LIBRARY, "apply", params) {
                Ok(job) => (true, f("Applying {title} (run {job})", &[("title", text(item, "title").as_str()), ("job", job.to_string().as_str())])),
                Err(e) => (false, e),
            });
        }
    });
    app.auto.add(&id, r.response.rect, &text(item, "title"));
}

/// `bashcut.ui.library.<name>`.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, name: &str, params: &Value) -> Result<Value, String> {
    let dir = super::data_dir(app);
    let mut p = get(ctx);
    load(&mut p, dir.as_deref());
    poll(app, ctx, &mut p);
    let str_param = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_string);
    if let Some(tab) = str_param("tab") {
        p.online = tab != "mine";
    }
    let r = match name {
        "search" => {
            let query = str_param("query").unwrap_or_default();
            if p.online {
                if let Some(k) = str_param("kind") {
                    if !KINDS.iter().any(|(id, _)| *id == k) {
                        put(ctx, p);
                        return Err(format!("kind must be one of {}", KINDS.map(|k| k.0).join(", ")));
                    }
                    p.kind = k;
                }
                p.query = query;
            } else {
                p.mine_kind = str_param("kind").unwrap_or_default();
                p.mine_query = query;
            }
            super::State::update(ctx, |st| st.show(super::LIBRARY));
            search(app, ctx, &mut p).map(|job| json!({"job": job}))
        }
        "results" => {
            let shown = if p.online { p.shown.clone() } else { p.mine_shown.clone() };
            let items = shown.as_ref().and_then(|k| p.results.get(k)).cloned().unwrap_or_default();
            Ok(json!({"searching": !p.searches.is_empty(), "downloading": p.downloads.len(), "items": items, "status": p.status.as_ref().map(|s| s.1.clone())}))
        }
        "place" | "add" => {
            let index = params.get("index").and_then(Value::as_u64).ok_or("`index` (a result's position) is required")? as usize;
            let item = p.shown.as_ref().and_then(|k| p.results.get(k)).and_then(|v| v.get(index)).cloned().ok_or(format!("no result {index}"))?;
            download(app, ctx, &mut p, &item, name == "place").map(|job| json!({"job": job}))
        }
        "credits" => {
            p.credits = params.get("on").and_then(Value::as_bool).unwrap_or(!p.credits);
            Ok(json!({"credits": p.credits}))
        }
        "state" => Ok(
            json!({"tab": if p.online { "online" } else { "mine" }, "kind": p.kind, "query": p.query, "credits": p.credits, "searching": p.searches.len(), "downloading": p.downloads.len()}),
        ),
        _ => Err(format!("unknown command bashcut.ui.library.{name}")),
    };
    save(&mut p, dir.as_deref());
    put(ctx, p);
    r
}

pub fn commands() -> Vec<CommandInfo> {
    vec![
        CommandInfo::new("bashcut.ui.library.search", "Search the Library", &[], None),
        CommandInfo::new("bashcut.ui.library.results", "Library Results", &[], None),
        CommandInfo::new("bashcut.ui.library.place", "Place a Library Result", &[], None),
        CommandInfo::new("bashcut.ui.library.add", "Add a Library Result to the Bin", &[], None),
        CommandInfo::new("bashcut.ui.library.credits", "Library Credit Lines", &[], None),
        CommandInfo::new("bashcut.ui.library.state", "Library State", &[], None),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_merge_into_one_block_without_repeats() {
        let once = merge_credits("Brief: a pottery video.", "- Morning Bed, by Ann, CC0");
        assert!(once.starts_with("Brief: a pottery video.\n\n"));
        assert_eq!(once.matches(CREDITS_START).count(), 1);
        let twice = merge_credits(&once, "Clay, by Bo, CC BY 4.0");
        assert_eq!(twice.matches(CREDITS_START).count(), 1);
        assert!(twice.contains("- Clay, by Bo, CC BY 4.0") && twice.contains("- Morning Bed, by Ann, CC0"));
        assert_eq!(merge_credits(&twice, "- Clay, by Bo, CC BY 4.0"), twice, "no repeats");
        assert!(merge_credits("", "x").starts_with(CREDITS_START));
        // a broken block (end before start) is left alone and a new block added
        let odd = format!("{CREDITS_END} then {CREDITS_START}");
        assert!(merge_credits(&odd, "y").contains("- y"));
    }

    #[test]
    fn the_cache_keeps_the_newest_queries() {
        let mut p = Panel::default();
        for i in 0..(MAX_CACHE + 5) {
            remember(&mut p, cache_key(true, "music", &format!("q{i}")), vec![json!(i)]);
        }
        assert_eq!(p.results.len(), MAX_CACHE);
        assert!(!p.results.contains_key(&cache_key(true, "music", "q0")));
        assert!(p.results.contains_key(&cache_key(true, "music", &format!("q{}", MAX_CACHE + 4))));
        assert_eq!(cache_key(true, "music", " Lofi "), cache_key(true, "music", "lofi"));
    }

    #[test]
    fn mine_rows_tag_the_kind_label() {
        assert_eq!(mine_kind_label("text-style"), s("Text styles"));
        assert_eq!(mine_kind_label("effect-preset"), s("Effect presets"));
        assert_eq!(mine_kind_label("brand-kit"), s("Brand kits"));
        assert_eq!(mine_kind_label("mystery"), "mystery", "unknown kinds show their id");
        assert_eq!(mine_kind_label(""), "", "no kind is not \"Everything\"");
    }

    #[test]
    fn values_read_as_text() {
        let v = json!({"title": "A", "seconds": 12, "tags": ["x", "y"], "none": null});
        assert_eq!(text(&v, "title"), "A");
        assert_eq!(text(&v, "seconds"), "12");
        assert_eq!(text(&v, "tags"), "x, y");
        assert_eq!(text(&v, "none"), "");
    }
}
