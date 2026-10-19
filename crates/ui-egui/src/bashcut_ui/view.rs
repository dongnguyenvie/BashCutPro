//! Plugin views (`bashcut-view/1`, `bashcut/in-app-ui.md` §7): the panel a plugin declares, drawn with egui widgets
//! and our theme inside its run in the Runs section. The model and its checks live in
//! `bashcut_plugin_host::view`; this file only draws it and turns a button press into a run of the plugin's action.
//!
//! Input values are kept per job in egui context data, so a live update of the view (a new stdout line) keeps what
//! the user typed. Every widget registers an automation id `bashcut.view.<job>.<id>`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bashcut_plugin_host::view::{self, Button, Row, View};
use bashcut_plugin_host::{Job, JobKind};
use serde_json::{Map, Value, json};

use super::plugins;
use crate::FilmcraftApp;

/// Largest image file a view may show.
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
/// Largest image side kept as a texture.
const MAX_IMAGE_SIDE: u32 = 2048;

/// What a button press asks for: run `action` of the view's plugin with `params`.
#[derive(Clone, Debug, PartialEq)]
pub struct Pressed {
    pub action: String,
    pub params: Map<String, Value>,
}

/// The values of every view on screen, by job.
#[derive(Clone, Default)]
struct Values(BTreeMap<u64, Map<String, Value>>);

fn key() -> egui::Id {
    egui::Id::new("bashcut.view.values")
}

/// The values of `job`'s view: what the user changed, else the view's own values (new fields of an updated view
/// start at the view's value).
fn values_for(ctx: &egui::Context, job: u64, v: &View) -> Map<String, Value> {
    let mut all = ctx.data_mut(|d| d.get_temp_mut_or_default::<Values>(key()).clone());
    let mine = all.0.entry(job).or_default();
    for (k, val) in view::initial_values(v) {
        mine.entry(k).or_insert(val);
    }
    mine.clone()
}

fn store(ctx: &egui::Context, job: u64, values: Map<String, Value>) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Values>(key()).0.insert(job, values);
    });
}

/// Forget the values of jobs no longer listed.
pub fn retain(ctx: &egui::Context, jobs: &[u64]) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Values>(key()).0.retain(|id, _| jobs.contains(id)));
}

/// The view of a plugin run inside its Runs row (one call from `runs_panel`); a press runs the plugin's action.
pub fn job_view(app: &mut FilmcraftApp, ui: &mut egui::Ui, job: &Job) {
    let Some(v) = &job.view else { return };
    let JobKind::Run { plugin, .. } = &job.kind else { return };
    let ctx = ui.ctx().clone();
    let dir = plugins::with(app, &ctx, |h| h.plugin(plugin).map(|p| p.dir.clone()));
    ui.add_space(4.0);
    if let Some(p) = show(app, ui, job.id, v, dir.as_deref()) {
        // only the params the action declares (the host refuses others): a view's values serve several buttons
        let declared: Vec<String> = plugins::with(app, &ctx, |h| {
            h.plugin(plugin).and_then(|i| i.manifest.as_ref()).and_then(|m| m.action(&p.action)).map(|a| a.params.iter().map(|x| x.id.clone()).collect())
        })
        .unwrap_or_default();
        let params = Value::Object(p.params.into_iter().filter(|(k, _)| declared.contains(k)).collect());
        if let Err(e) = plugins::invoke(app, &ctx, "run", &json!({"plugin": plugin, "action": p.action, "params": params})) {
            ui.label(egui::RichText::new(e).size(11.0).color(egui::Color32::from_rgb(0xe0, 0x6c, 0x5f)));
        }
    }
}

/// Draw `v` for `job`; returns the button pressed this frame. `plugin_dir` is where relative image paths resolve.
pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, job: u64, v: &View, plugin_dir: Option<&Path>) -> Option<Pressed> {
    let t = app.tokens;
    let ctx = ui.ctx().clone();
    let mut values = values_for(&ctx, job, v);
    let mut pressed = None;
    let id = |name: &str| format!("bashcut.view.{job}.{name}");
    if !v.title.is_empty() {
        let r = ui.label(egui::RichText::new(&v.title).strong().color(t.text));
        app.auto.add_response(&id("title"), &r, &v.title);
    }
    for (i, row) in v.rows.iter().enumerate() {
        match row {
            Row::Text { text, style } => {
                let rich = egui::RichText::new(text);
                let rich = match style.as_str() {
                    "title" => rich.strong().size(13.0).color(t.text),
                    "dim" => rich.size(11.0).color(t.text_dim),
                    "error" => rich.size(11.0).color(egui::Color32::from_rgb(0xe0, 0x6c, 0x5f)),
                    _ => rich.color(t.text),
                };
                let r = ui.label(rich);
                app.auto.add_response(&id(&format!("text{i}")), &r, text);
            }
            Row::Input { id: field, label, multiline, .. } => {
                if !label.is_empty() {
                    ui.label(egui::RichText::new(label).size(11.0).color(t.text_dim));
                }
                let mut text = values.get(field).and_then(Value::as_str).unwrap_or_default().to_string();
                let edit = if *multiline { egui::TextEdit::multiline(&mut text).desired_rows(3) } else { egui::TextEdit::singleline(&mut text) };
                let r = ui.add(edit.desired_width(f32::INFINITY));
                app.auto.add_response(&id(field), &r, label);
                if r.changed() {
                    values.insert(field.clone(), Value::String(text));
                }
            }
            Row::Select { id: field, label, options, .. } => {
                let mut current = values.get(field).and_then(Value::as_str).unwrap_or_default().to_string();
                let before = current.clone();
                let r = egui::ComboBox::from_label(egui::RichText::new(label).size(11.0).color(t.text_dim))
                    .id_salt(("bashcut.view.select", job, field.as_str()))
                    .selected_text(current.clone())
                    .show_ui(ui, |ui| {
                        for o in options {
                            ui.selectable_value(&mut current, o.clone(), o);
                        }
                    });
                app.auto.add_response(&id(field), &r.response, label);
                if current != before {
                    values.insert(field.clone(), Value::String(current));
                }
            }
            Row::Toggle { id: field, label, .. } => {
                let mut on = values.get(field).and_then(Value::as_bool).unwrap_or(false);
                let r = ui.checkbox(&mut on, label.as_str());
                app.auto.add_response(&id(field), &r, label);
                if r.changed() {
                    values.insert(field.clone(), Value::Bool(on));
                }
            }
            Row::Button(b) => {
                if button(app, ui, &id(&format!("button{i}")), b) {
                    pressed = Some(Pressed { action: b.action.clone(), params: view::press_params(b, &values, None) });
                }
            }
            Row::List { id: list, items } => {
                let list_id = if list.is_empty() { format!("list{i}") } else { list.clone() };
                for (n, it) in items.iter().enumerate() {
                    let row_id = if it.id.is_empty() { n.to_string() } else { it.id.clone() };
                    let frame = egui::Frame::new().inner_margin(egui::Margin::same(3)).fill(if n % 2 == 1 { t.row_alt } else { t.panel_bg });
                    let r = frame.show(ui, |ui| {
                        ui.label(egui::RichText::new(&it.text).color(t.text));
                        if !it.detail.is_empty() {
                            ui.label(egui::RichText::new(&it.detail).size(11.0).color(t.text_dim));
                        }
                        if !it.buttons.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                for (k, b) in it.buttons.iter().enumerate() {
                                    if button(app, ui, &id(&format!("{list_id}.{row_id}.{k}")), b) {
                                        pressed = Some(Pressed { action: b.action.clone(), params: view::press_params(b, &values, Some(&it.id)) });
                                    }
                                }
                            });
                        }
                    });
                    app.auto.add(&id(&format!("{list_id}.{row_id}")), r.response.rect, &it.text);
                }
            }
            Row::Image { path, width } => match image_texture(&ctx, path, plugin_dir) {
                Some(tex) => {
                    let size = tex.size_vec2();
                    let w = if *width > 0.0 { *width } else { size.x }.min(ui.available_width()).max(1.0);
                    let h = if size.x > 0.0 { size.y * w / size.x } else { w };
                    let r = ui.add(egui::Image::new((tex.id(), egui::vec2(w, h))));
                    app.auto.add_response(&id(&format!("image{i}")), &r, path);
                }
                None => {
                    ui.label(egui::RichText::new(format!("(image not shown: {path})")).size(10.5).color(t.text_dim));
                }
            },
            Row::Progress { value, label } => {
                let r = ui.add(egui::ProgressBar::new(*value).show_percentage().desired_height(6.0));
                app.auto.add_response(&id(&format!("progress{i}")), &r, label);
                if !label.is_empty() {
                    ui.label(egui::RichText::new(label).size(10.5).color(t.text_dim));
                }
            }
        }
    }
    store(&ctx, job, values);
    pressed
}

fn button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, b: &Button) -> bool {
    let t = app.tokens;
    let label = if b.label.is_empty() { b.action.as_str() } else { b.label.as_str() };
    let w = if b.primary { egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE)).fill(t.accent) } else { egui::Button::new(label) };
    let r = ui.add(w);
    app.auto.add_response(id, &r, label);
    r.clicked()
}

/// Where an image row may be read from: a relative path in the plugin's folder, or an absolute one inside the
/// plugin's folder or the temp dir. Anything else is not shown.
pub fn image_path(path: &str, plugin_dir: Option<&Path>) -> Option<PathBuf> {
    let p = Path::new(path);
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return None;
    }
    let full = if p.is_absolute() { p.to_path_buf() } else { plugin_dir?.join(p) };
    let roots: Vec<PathBuf> = plugin_dir.map(Path::to_path_buf).into_iter().chain(std::iter::once(std::env::temp_dir())).collect();
    let canon = full.canonicalize().ok()?;
    roots.iter().filter_map(|r| r.canonicalize().ok()).any(|r| canon.starts_with(r)).then_some(canon)
}

/// A PNG as a texture, cached by path and modification time.
fn image_texture(ctx: &egui::Context, path: &str, plugin_dir: Option<&Path>) -> Option<egui::TextureHandle> {
    let full = image_path(path, plugin_dir)?;
    let meta = std::fs::metadata(&full).ok().filter(|m| m.is_file() && m.len() <= MAX_IMAGE_BYTES)?;
    let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
    let id = egui::Id::new(("bashcut.view.image", full.to_string_lossy().into_owned(), stamp));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let bytes = std::fs::read(&full).ok()?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
    let img = if img.width() > MAX_IMAGE_SIDE || img.height() > MAX_IMAGE_SIDE { img.thumbnail(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE) } else { img };
    let rgba = img.to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
    let tex = ctx.load_texture(format!("bashcut-view-{}", full.display()), color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_paths_stay_inside_the_plugin_or_temp() {
        let dir = std::env::temp_dir().join(format!("bashcut-view-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        assert!(image_path("a.png", Some(&dir)).is_some());
        assert!(image_path("../a.png", Some(&dir)).is_none());
        assert!(image_path("missing.png", Some(&dir)).is_none());
        assert!(image_path("a.png", None).is_none(), "a relative path needs the plugin folder");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
