//! The Looks section (#47, `bashcut/in-app-ui.md` §1a): the curated look pack with each look's
//! purpose, tags and an engine-rendered thumbnail; one intensity knob; apply to the selection
//! (`bashcut.looks.apply`, undoable), preview on the current program frame, favourites.
//!
//! Thumbnails are rendered by `filmcraft_engine::bashcut::looks::thumbnail` (upstream's Lumetri on
//! its procedural preview picture, or on the program frame) a few per frame and cached as textures
//! by look fingerprint. Favourites, the knob and the preview source are saved to
//! `<data dir>/bashcut/looks-ui.json`. State lives in egui context data.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use filmcraft_engine::bashcut::looks::{self, Look};
use filmcraft_engine::shortcuts::CommandInfo;
use filmcraft_render::Image;
use serde_json::{Value, json};

use super::strings::{f, s};
use super::widgets::{META, NOTE, editor_open, note};
use crate::FilmcraftApp;

const THUMB_W: usize = 128;
const THUMB_H: usize = 72;
/// Thumbnails rendered per frame (the rest follow on the next frames).
const PER_FRAME: usize = 3;
/// Cached thumbnails kept before the cache is cleared.
const MAX_THUMBS: usize = 256;
const MAX_FILE: u64 = 64 * 1024;

#[derive(Clone)]
struct Panel {
    filter: String,
    tag: Option<String>,
    /// Every tag is shown, not only the most used ones.
    all_tags: bool,
    /// The knob, 1.0 = each look as designed (clamped to each look's safe range when applied).
    intensity: f64,
    /// Thumbnails show the current program frame instead of the preview picture.
    on_frame: bool,
    favourites: Vec<String>,
    loaded: bool,
    saved: Option<String>,
    /// `<look>|<fingerprint>|<source>` → texture.
    thumbs: HashMap<String, egui::TextureHandle>,
    frame: Option<Arc<Image>>,
    /// Bumped each time the program frame is grabbed again.
    frame_gen: u64,
    status: Option<(bool, String)>,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            filter: String::new(),
            tag: None,
            all_tags: false,
            intensity: 1.0,
            on_frame: false,
            favourites: Vec::new(),
            loaded: false,
            saved: None,
            thumbs: HashMap::new(),
            frame: None,
            frame_gen: 0,
            status: None,
        }
    }
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.looks.panel")
}

fn get(ctx: &egui::Context) -> Panel {
    super::shared::temp::<Panel>(ctx, key())
}

fn put(ctx: &egui::Context, p: Panel) {
    super::shared::put_temp(ctx, key(), p);
}

/// `<data dir>/bashcut/looks-ui.json`.
pub fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("bashcut").join("looks-ui.json")
}

fn prefs(p: &Panel) -> Value {
    json!({"favourites": p.favourites, "intensity": p.intensity, "onFrame": p.on_frame})
}

fn load(p: &mut Panel, dir: Option<&Path>) {
    if p.loaded {
        return;
    }
    p.loaded = true;
    let Some(dir) = dir else { return };
    let Some(text) = super::prefs::read(&file(dir), MAX_FILE) else { return };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if let Some(a) = v.get("favourites").and_then(Value::as_array) {
        p.favourites = a.iter().filter_map(Value::as_str).filter(|id| looks::find(id).is_some()).take(64).map(str::to_string).collect();
    }
    if let Some(x) = v.get("intensity").and_then(Value::as_f64).filter(|x| x.is_finite()) {
        p.intensity = x.clamp(0.0, looks::MAX_AMOUNT);
    }
    // the frame itself is not saved: grab it again when asked
    p.saved = Some(prefs(p).to_string());
}

fn save(p: &mut Panel, dir: Option<&Path>) {
    let Some(dir) = dir else { return };
    super::prefs::save_if_changed(&file(dir), prefs(p).to_string(), &mut p.saved, "bashcut looks");
}

/// Grab the program frame at the playhead for previews; `false` when there is none.
fn grab_frame(app: &FilmcraftApp, p: &mut Panel) -> bool {
    match looks::program_frame(&app.session, THUMB_W * 2) {
        Some(img) => {
            p.frame = Some(Arc::new(img));
            p.frame_gen = p.frame_gen.wrapping_add(1);
            true
        }
        None => false,
    }
}

/// The look applied to the first selected video clip, if any.
fn applied_to_selection(app: &FilmcraftApp) -> Option<(String, f64)> {
    let q = app.session.active_sequence()?;
    let sel = &app.session.state.selection;
    q.video_tracks
        .iter()
        .flat_map(|t| t.items.iter())
        .filter(|i| sel.contains(&i.id))
        .find_map(|i| i.effects.iter().find(|e| looks::is_look(e)).and_then(looks::look_of))
}

/// The looks shown, favourites first, then the pack's order.
fn shown<'a>(p: &Panel, pack: &'a [Look]) -> Vec<&'a Look> {
    let q = p.filter.trim().to_lowercase();
    let mut v: Vec<&Look> = pack
        .iter()
        .filter(|l| p.tag.as_deref().is_none_or(|t| l.tags.contains(&t)))
        .filter(|l| {
            q.is_empty() || l.name.to_lowercase().contains(&q) || l.purpose.to_lowercase().contains(&q) || l.tags.iter().any(|t| t.contains(q.as_str()))
        })
        .collect();
    v.sort_by_key(|l| !p.favourites.iter().any(|f| f == l.id));
    v
}

/// Tags shown before "More tags".
const TOP_TAGS: usize = 10;

fn tag_count(pack: &[Look]) -> usize {
    let mut all: Vec<&str> = pack.iter().flat_map(|l| l.tags.iter().copied()).collect();
    all.sort_unstable();
    all.dedup();
    all.len()
}

/// The tag buttons: every tag when `all`, else the most used ones plus the chosen one, alphabetical.
fn shown_tags<'a>(pack: &'a [Look], chosen: Option<&str>, all: bool) -> Vec<&'a str> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for t in pack.iter().flat_map(|l| l.tags.iter().copied()) {
        *counts.entry(t).or_default() += 1;
    }
    let mut tags: Vec<(&str, usize)> = counts.into_iter().collect();
    if !all {
        tags.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut rank = 0;
        tags.retain(|(t, _)| {
            rank += 1;
            rank <= TOP_TAGS || Some(*t) == chosen
        });
    }
    let mut names: Vec<&str> = tags.into_iter().map(|(t, _)| t).collect();
    names.sort_unstable();
    names
}

fn texture(ctx: &egui::Context, name: &str, img: &Image) -> egui::TextureHandle {
    let rgba = img.over_black_rgba8();
    let ci = egui::ColorImage::from_rgba_unmultiplied([img.w, img.h], &rgba);
    ctx.load_texture(name, ci, egui::TextureOptions::LINEAR)
}

pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let t = app.tokens;
    let dir = super::data_dir(app);
    let mut p = get(&ctx);
    load(&mut p, dir.as_deref());
    let pack = looks::pack();

    // browsing works on every page; applying needs the timeline
    let editing = editor_open(app, ui, "bashcut.looks.editor");
    if let Some((ok, msg)) = &p.status {
        let color = if *ok { t.text_dim } else { t.danger };
        let r = ui.label(egui::RichText::new(msg).size(11.0).color(color));
        app.auto.add_response("bashcut.looks.status", &r, msg);
    }

    // search, knob, preview source
    let r = ui.add(egui::TextEdit::singleline(&mut p.filter).hint_text(s("Search looks")).desired_width(f32::INFINITY));
    app.auto.add_response("bashcut.looks.filter", &r, &p.filter);
    ui.horizontal(|ui| {
        note(ui, s("Intensity"), NOTE, t.text_dim);
        let mut pct = (p.intensity * 100.0).round();
        let r = ui.add(egui::Slider::new(&mut pct, 0.0..=looks::MAX_AMOUNT * 100.0).suffix("%").step_by(5.0));
        app.auto.add_response("bashcut.looks.intensity", &r, &format!("{pct:.0}"));
        if r.changed() {
            p.intensity = (pct / 100.0).clamp(0.0, looks::MAX_AMOUNT);
        }
    });
    ui.horizontal(|ui| {
        let mut on = p.on_frame;
        let r = ui.checkbox(&mut on, s("Preview on current frame"));
        app.auto.add_response("bashcut.looks.frame", &r, if on { "on" } else { "off" });
        if r.changed() {
            if on && !grab_frame(app, &mut p) {
                p.status = Some((false, s("No program frame to preview on.").to_string()));
                on = false;
            }
            p.on_frame = on;
        }
        if p.on_frame {
            let r = ui.small_button(s("Refresh"));
            app.auto.add_response("bashcut.looks.frame.refresh", &r, s("Refresh"));
            if r.clicked() && !grab_frame(app, &mut p) {
                p.status = Some((false, s("No program frame to preview on.").to_string()));
            }
        }
    });

    // genre tags: the most used ones (and the chosen one), the rest behind "More tags"
    let tags = shown_tags(pack, p.tag.as_deref(), p.all_tags);
    let hidden = tag_count(pack).saturating_sub(tags.len());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        let all = ui.selectable_label(p.tag.is_none(), egui::RichText::new(s("All")).size(11.0));
        app.auto.add_response("bashcut.looks.tag.all", &all, s("All"));
        if all.clicked() {
            p.tag = None;
        }
        for tag in tags {
            let on = p.tag.as_deref() == Some(tag);
            let r = ui.selectable_label(on, egui::RichText::new(tag).size(11.0));
            app.auto.add_response(&format!("bashcut.looks.tag.{tag}"), &r, tag);
            if r.clicked() {
                p.tag = if on { None } else { Some(tag.to_string()) };
            }
        }
        let more =
            if p.all_tags { Some(s("Fewer tags").to_string()) } else { (hidden > 0).then(|| f("More tags ({n})", &[("n", hidden.to_string().as_str())])) };
        if let Some(label) = more {
            let r = ui.small_button(egui::RichText::new(&label).size(11.0));
            app.auto.add_response("bashcut.looks.tags.more", &r, &label);
            if r.clicked() {
                p.all_tags = !p.all_tags;
            }
        }
    });

    // selection
    let selected = app.session.state.selection.len();
    let current = applied_to_selection(app);
    ui.horizontal(|ui| {
        if !editing {
            return;
        }
        let n = selected.to_string();
        let msg = match selected {
            0 => s("Select clips on the timeline to apply a look.").to_string(),
            1 => s("1 clip selected").to_string(),
            _ => f("{n} clips selected", &[("n", n.as_str())]),
        };
        note(ui, msg, NOTE, t.text_dim);
        if current.is_some() {
            let r = ui.small_button(s("Remove look"));
            app.auto.add_response("bashcut.looks.remove", &r, s("Remove look"));
            if r.clicked() {
                p.status = match crate::menus::invoke(app, &ctx, "bashcut.looks.remove", json!({})) {
                    Ok(_) => None,
                    Err(e) => Some((false, super::palette::plain_error(&e))),
                };
            }
        }
    });
    ui.add_space(4.0);

    // cards
    if p.thumbs.len() > MAX_THUMBS {
        p.thumbs.clear();
    }
    let source = if p.on_frame { format!("frame{}", p.frame_gen) } else { "picture".to_string() };
    let base = if p.on_frame { p.frame.clone() } else { None };
    let mut rendered = 0;
    let mut pending = false;
    let list = shown(&p, pack);
    if list.is_empty() {
        note(ui, s("No look matches."), NOTE, t.text_dim);
    }
    for look in list {
        let amount = look.clamp(Some(p.intensity));
        let tkey = format!("{}|{}|{source}", look.id, look.fingerprint(amount));
        if !p.thumbs.contains_key(&tkey) {
            if rendered < PER_FRAME {
                let img = looks::thumbnail(look, amount, base.as_deref(), THUMB_W, THUMB_H);
                p.thumbs.insert(tkey.clone(), texture(&ctx, &format!("bashcut.look.{tkey}"), &img));
                rendered += 1;
            } else {
                pending = true;
            }
        }
        let tex = p.thumbs.get(&tkey).cloned();
        let is_current = current.as_ref().is_some_and(|(id, _)| id == look.id);
        card(app, ui, &ctx, &mut p, look, amount, tex, is_current, editing && selected > 0);
    }
    if pending {
        ctx.request_repaint();
    }
    if !ctx.input(|i| i.pointer.any_down()) {
        save(&mut p, dir.as_deref());
    }
    put(&ctx, p);
}

#[allow(clippy::too_many_arguments)]
fn card(
    app: &mut FilmcraftApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    p: &mut Panel,
    look: &Look,
    amount: f64,
    tex: Option<egui::TextureHandle>,
    is_current: bool,
    can_apply: bool,
) {
    let t = app.tokens;
    let id = format!("bashcut.looks.{}", look.id);
    let frame = egui::Frame::new().inner_margin(egui::Margin::same(4)).corner_radius(t.radius_sm).fill(if is_current { t.pill_active_bg } else { t.panel_bg });
    let r = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            let size = egui::vec2(THUMB_W as f32 * 0.75, THUMB_H as f32 * 0.75);
            match &tex {
                Some(tex) => {
                    ui.add(egui::Image::new((tex.id(), size)));
                }
                None => {
                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    ui.painter().rect_filled(r, t.radius_sm, t.row_alt);
                }
            }
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(look.name).strong().color(t.text));
                    let fav = p.favourites.iter().any(|f| f == look.id);
                    let star = ui.small_button(if fav { "★" } else { "☆" });
                    app.auto.add_response(&format!("{id}.favourite"), &star, if fav { "on" } else { "off" });
                    if star.on_hover_text(s("Favourite")).clicked() {
                        toggle_favourite(p, look.id, None);
                    }
                    if is_current {
                        note(ui, s("Applied"), META, t.accent);
                    }
                });
                note(ui, look.purpose, NOTE, t.text_dim);
                ui.label(egui::RichText::new(look.tags.join(" · ")).size(10.0).color(t.text_faint));
                let apply = ui.add_enabled(can_apply, egui::Button::new(s("Apply")).small());
                app.auto.add_response(&format!("{id}.apply"), &apply, s("Apply"));
                if apply.clicked() {
                    let params = json!({"look": look.id, "intensity": amount});
                    p.status = match crate::menus::invoke(app, ctx, "bashcut.looks.apply", params) {
                        Ok(v) => {
                            let n = v["clips"].to_string();
                            let text = if n == "1" { "Applied {look} to 1 clip" } else { "Applied {look} to {n} clips" };
                            Some((true, f(text, &[("look", look.name), ("n", n.as_str())])))
                        }
                        Err(e) => Some((false, super::palette::plain_error(&e))),
                    };
                }
            });
        });
    });
    app.auto.add(&id, r.response.rect, look.name);
}

fn toggle_favourite(p: &mut Panel, id: &str, on: Option<bool>) -> bool {
    let has = p.favourites.iter().any(|f| f == id);
    let want = on.unwrap_or(!has);
    if want && !has {
        p.favourites.push(id.to_string());
    } else if !want {
        p.favourites.retain(|f| f != id);
    }
    want
}

/// `bashcut.ui.looks.<name>`.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, name: &str, params: &Value) -> Result<Value, String> {
    let dir = super::data_dir(app);
    let mut p = get(ctx);
    load(&mut p, dir.as_deref());
    let r = match name {
        "favourite" => {
            let id = params.get("look").and_then(Value::as_str).ok_or("`look` is required")?;
            let look = looks::find(id).ok_or(format!("no look `{id}`"))?;
            let on = toggle_favourite(&mut p, look.id, params.get("on").and_then(Value::as_bool));
            Ok(json!({"look": look.id, "favourite": on}))
        }
        "previewFrame" => {
            let on = params.get("on").and_then(Value::as_bool).unwrap_or(true);
            if on && !grab_frame(app, &mut p) {
                return Err("no program frame to preview on".into());
            }
            p.on_frame = on;
            Ok(json!({"onFrame": on}))
        }
        "state" => Ok(json!({"favourites": p.favourites, "intensity": p.intensity, "onFrame": p.on_frame, "tag": p.tag, "filter": p.filter})),
        _ => Err(format!("unknown command bashcut.ui.looks.{name}")),
    };
    save(&mut p, dir.as_deref());
    put(ctx, p);
    r
}

pub fn commands() -> Vec<CommandInfo> {
    vec![
        CommandInfo::new("bashcut.ui.looks.favourite", "Favourite Look", &[], None),
        CommandInfo::new("bashcut.ui.looks.previewFrame", "Preview Looks on Current Frame", &[], None),
        CommandInfo::new("bashcut.ui.looks.state", "Looks Panel State", &[], None),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favourites_sort_first_and_filters_apply() {
        let pack = looks::pack();
        let mut p = Panel::default();
        let last = pack.last().unwrap().id;
        assert!(toggle_favourite(&mut p, last, None));
        assert_eq!(shown(&p, pack)[0].id, last);
        assert!(!toggle_favourite(&mut p, last, None));
        p.tag = Some("food".into());
        assert!(shown(&p, pack).iter().all(|l| l.tags.contains(&"food")));
        p.tag = None;
        p.filter = "sunset".into();
        assert!(shown(&p, pack).iter().any(|l| l.id == "golden-hour"));
    }

    #[test]
    fn tags_show_the_most_used_until_more_is_asked_for() {
        let pack = looks::pack();
        let top = shown_tags(pack, None, false);
        assert!(top.len() <= TOP_TAGS && tag_count(pack) > TOP_TAGS, "{top:?}");
        assert!(top.windows(2).all(|w| w[0] < w[1]), "alphabetical");
        let rare = shown_tags(pack, None, true).into_iter().find(|t| !top.contains(t)).unwrap();
        assert!(shown_tags(pack, Some(rare), false).contains(&rare), "the chosen tag stays visible");
        assert_eq!(shown_tags(pack, None, true).len(), tag_count(pack));
    }

    #[test]
    fn prefs_survive_a_restart_and_hostile_files() {
        let dir = std::env::temp_dir().join(format!("bashcut-looks-ui-{}", std::process::id()));
        let mut p = Panel { favourites: vec!["teal-orange".into()], intensity: 0.6, ..Panel::default() };
        save(&mut p, Some(&dir));
        let mut back = Panel::default();
        load(&mut back, Some(&dir));
        assert_eq!(back.favourites, vec!["teal-orange".to_string()]);
        assert!((back.intensity - 0.6).abs() < 1e-9);
        let _ = std::fs::write(file(&dir), r#"{"favourites":["nope","teal-orange",3],"intensity":1e9}"#);
        let mut back = Panel::default();
        load(&mut back, Some(&dir));
        assert_eq!(back.favourites, vec!["teal-orange".to_string()]);
        assert_eq!(back.intensity, looks::MAX_AMOUNT);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
