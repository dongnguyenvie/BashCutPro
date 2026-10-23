//! A plugin's `bashcut-view/1` form, drawn in its card in the Plugins section
//! (`bashcut/in-app-ui.md` §7): text, input, select, toggle, button, list with row buttons, image
//! and progress. The plugin replaces the form while its `view` action runs; what the person types
//! stays (egui context data, not `UiState`) until the plugin changes that field itself.
//!
//! Automation ids: `bashcut.view.<plugin>.<field or button>` and
//! `bashcut.view.<plugin>.<list>.<item>.<button>`; text, image and progress rows are
//! `bashcut.view.<plugin>.row<n>`.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;
use std::time::SystemTime;

use bashcut_plugin_host::view::{Row, View};
use serde_json::{Map, Value};

use super::widgets::{NOTE, button, note, scroll_clear_of_bar};
use crate::FilmcraftApp;

/// Largest picture file read.
const MAX_IMAGE_FILE: u64 = 16 * 1024 * 1024;
/// Largest picture decoded (px a side).
const MAX_IMAGE_SIDE: u32 = 4096;
const IMAGE_HEIGHT: f32 = 240.0;
const LIST_HEIGHT: f32 = 320.0;

/// A button pressed in a form: the button row's id, or `(list, (item, button))`, and the form's
/// values.
#[derive(Clone, Debug, PartialEq)]
pub struct Click {
    pub button: String,
    pub item: Option<(String, String)>,
    pub values: Map<String, Value>,
}

/// The form's fields: id → (what the plugin last said, what the form holds).
#[derive(Clone, Default)]
struct Fields(BTreeMap<String, (Value, Value)>);

fn fields_key(plugin: &str, job: u64) -> egui::Id {
    egui::Id::new(("bashcut.view.fields", plugin, job))
}

/// A field row's value as the plugin set it.
fn row_value(row: &Row) -> Option<(&str, Value)> {
    match row {
        Row::Input { id, value, .. } | Row::Select { id, value, .. } => Some((id.as_str(), Value::String(value.clone()))),
        Row::Toggle { id, value, .. } => Some((id.as_str(), Value::Bool(*value))),
        _ => None,
    }
}

/// Take the plugin's value where it changed (or is new); keep what the person typed elsewhere.
fn sync(fields: &mut Fields, view: &View) {
    let mut seen = Vec::new();
    for (id, v) in view.rows.iter().filter_map(row_value) {
        seen.push(id.to_string());
        if fields.0.get(id).is_none_or(|(said, _)| *said != v) {
            fields.0.insert(id.to_string(), (v.clone(), v));
        }
    }
    fields.0.retain(|k, _| seen.contains(k));
}

/// Draw `view` (shown by run `job` of `plugin`, installed in `dir`); returns the button pressed.
pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui, plugin: &str, dir: &Path, job: u64, view: &View) -> Option<Click> {
    let ctx = ui.ctx().clone();
    let key = fields_key(plugin, job);
    let mut fields = super::shared::temp::<Fields>(&ctx, key);
    sync(&mut fields, view);
    let t = app.tokens;
    let mut pressed: Option<(String, Option<(String, String)>)> = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        if !view.title.is_empty() {
            let r = ui.label(egui::RichText::new(&view.title).strong().color(t.text));
            app.auto.add_response(&format!("bashcut.view.{plugin}.title"), &r, &view.title);
        }
        for (n, row) in view.rows.iter().enumerate() {
            let row_id = format!("bashcut.view.{plugin}.row{n}");
            match row {
                Row::Text { text, muted } => {
                    let rt = egui::RichText::new(text);
                    let r = ui.label(if *muted { rt.size(11.0).color(t.text_dim) } else { rt.color(t.text) });
                    app.auto.add_response(&row_id, &r, text);
                }
                Row::Input { id, label, multiline, .. } => {
                    let mut text = field_text(&fields, id);
                    ui.horizontal(|ui| {
                        if !label.is_empty() {
                            ui.label(label.as_str());
                        }
                        let edit = if *multiline { egui::TextEdit::multiline(&mut text).desired_rows(3) } else { egui::TextEdit::singleline(&mut text) };
                        let r = ui.add(edit.desired_width(ui.available_width().min(260.0)));
                        app.auto.add_response(&format!("bashcut.view.{plugin}.{id}"), &r, label);
                    });
                    set(&mut fields, id, Value::String(text));
                }
                Row::Select { id, label, choices, .. } => {
                    let mut value = field_text(&fields, id);
                    ui.horizontal(|ui| {
                        if !label.is_empty() {
                            ui.label(label.as_str());
                        }
                        let shown = choices.iter().find(|c| c.value == value).map_or(value.clone(), |c| c.label.clone());
                        let r = egui::ComboBox::from_id_salt(("bashcut.view", plugin, id.as_str()))
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                for c in choices {
                                    ui.selectable_value(&mut value, c.value.clone(), c.label.as_str());
                                }
                            })
                            .response;
                        app.auto.add_response(&format!("bashcut.view.{plugin}.{id}"), &r, label);
                    });
                    set(&mut fields, id, Value::String(value));
                }
                Row::Toggle { id, label, .. } => {
                    let mut on = fields.0.get(id).and_then(|(_, v)| v.as_bool()).unwrap_or(false);
                    let r = ui.checkbox(&mut on, label.as_str());
                    app.auto.add_response(&format!("bashcut.view.{plugin}.{id}"), &r, label);
                    set(&mut fields, id, Value::Bool(on));
                }
                Row::Button { id, label, .. } => {
                    let text = if label.is_empty() { id.as_str() } else { label.as_str() };
                    if button(app, ui, &format!("bashcut.view.{plugin}.{id}"), text, true) {
                        pressed = Some((id.clone(), None));
                    }
                }
                Row::List { id, label, items } => {
                    if !label.is_empty() {
                        ui.label(egui::RichText::new(label).strong().color(t.text));
                    }
                    let area = egui::ScrollArea::vertical().id_salt(("bashcut.view.list", plugin, id.as_str())).max_height(LIST_HEIGHT);
                    scroll_clear_of_bar(ui, area, egui::Id::new(("bashcut.view.list.bar", plugin, id.as_str())), |ui| {
                        for it in items {
                            ui.horizontal_wrapped(|ui| {
                                let r = ui.label(egui::RichText::new(&it.text).color(t.text));
                                app.auto.add_response(&format!("bashcut.view.{plugin}.{id}.{}", it.id), &r, &it.text);
                                for b in &it.buttons {
                                    let text = if b.label.is_empty() { b.id.as_str() } else { b.label.as_str() };
                                    if button(app, ui, &format!("bashcut.view.{plugin}.{id}.{}.{}", it.id, b.id), text, true) {
                                        pressed = Some((id.clone(), Some((it.id.clone(), b.id.clone()))));
                                    }
                                }
                            });
                            if !it.detail.is_empty() {
                                note(ui, &it.detail, NOTE, t.text_dim);
                            }
                            ui.add_space(2.0);
                        }
                    });
                }
                Row::Image { path, caption } => {
                    let full = if Path::new(path).is_absolute() { Path::new(path).to_path_buf() } else { dir.join(path) };
                    match image(&ctx, &full) {
                        Ok(tex) => {
                            let [w, h] = tex.size();
                            let scale = (ui.available_width() / w.max(1) as f32).min(IMAGE_HEIGHT / h.max(1) as f32).min(1.0);
                            let size = egui::vec2(w as f32 * scale, h as f32 * scale);
                            let r = ui.add(egui::Image::new((tex.id(), size)));
                            app.auto.add_response(&row_id, &r, caption);
                        }
                        Err(e) => {
                            let r = note(ui, format!("{path}: {e}"), NOTE, t.text_dim);
                            app.auto.add_response(&row_id, &r, caption);
                        }
                    }
                    if !caption.is_empty() {
                        note(ui, caption, NOTE, t.text_dim);
                    }
                }
                Row::Progress { value, label } => {
                    let mut bar = egui::ProgressBar::new(value.unwrap_or(0.0)).animate(value.is_none());
                    if !label.is_empty() {
                        bar = bar.text(label.as_str());
                    } else if value.is_some() {
                        bar = bar.show_percentage();
                    }
                    let r = ui.add(bar);
                    app.auto.add_response(&row_id, &r, label);
                }
            }
        }
    });
    super::shared::put_temp(&ctx, key, fields.clone());
    let (button, item) = pressed?;
    Some(Click { button, item, values: fields.0.into_iter().map(|(k, (_, v))| (k, v)).collect() })
}

fn field_text(fields: &Fields, id: &str) -> String {
    fields.0.get(id).and_then(|(_, v)| v.as_str()).unwrap_or_default().to_string()
}

fn set(fields: &mut Fields, id: &str, v: Value) {
    if let Some((_, held)) = fields.0.get_mut(id) {
        *held = v;
    }
}

/// The picture at `path` as a texture, decoded once per version of the file (a plugin may rewrite
/// it while it runs). PNG only, capped in file size and pixels.
fn image(ctx: &egui::Context, path: &Path) -> Result<egui::TextureHandle, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let stamp = meta.modified().ok().and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
    let key = egui::Id::new(("bashcut.view.image", path, meta.len(), stamp));
    if let Some(cached) = ctx.data(|d| d.get_temp::<Result<egui::TextureHandle, String>>(key)) {
        return cached;
    }
    let loaded = decode(path, meta.len()).map(|ci| ctx.load_texture(format!("bashcut.view.{}", path.display()), ci, egui::TextureOptions::LINEAR));
    ctx.data_mut(|d| d.insert_temp(key, loaded.clone()));
    loaded
}

fn decode(path: &Path, len: u64) -> Result<egui::ColorImage, String> {
    if len > MAX_IMAGE_FILE {
        return Err(format!("larger than {} MB", MAX_IMAGE_FILE / (1024 * 1024)));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view(rows: Value) -> View {
        View::from_value(&json!({"schema": "bashcut-view/1", "rows": rows})).unwrap()
    }

    #[test]
    fn typed_values_stay_until_the_plugin_changes_them() {
        let mut fields = Fields::default();
        sync(&mut fields, &view(json!([{"type": "input", "id": "lang", "value": "vi"}, {"type": "toggle", "id": "keep", "value": true}])));
        set(&mut fields, "lang", json!("en"));
        // the same view again (a live update of other rows): the typed value stays
        sync(&mut fields, &view(json!([{"type": "input", "id": "lang", "value": "vi"}, {"type": "toggle", "id": "keep", "value": true}])));
        assert_eq!(fields.0["lang"].1, json!("en"));
        // the plugin sets the field itself: its value wins; a field gone from the view is dropped
        sync(&mut fields, &view(json!([{"type": "input", "id": "lang", "value": "ja"}])));
        assert_eq!(fields.0["lang"].1, json!("ja"));
        assert!(!fields.0.contains_key("keep"));
        set(&mut fields, "nope", json!(1));
        assert!(!fields.0.contains_key("nope"));
    }

    #[test]
    fn pictures_are_capped() {
        let dir = std::env::temp_dir().join(format!("bashcut-view-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.png");
        image::RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255])).save(&ok).unwrap();
        assert_eq!(decode(&ok, std::fs::metadata(&ok).unwrap().len()).unwrap().size, [4, 2]);
        let big = dir.join("big.png");
        image::GrayImage::new(MAX_IMAGE_SIDE + 1, 1).save(&big).unwrap();
        assert!(decode(&big, std::fs::metadata(&big).unwrap().len()).is_err());
        let junk = dir.join("junk.png");
        std::fs::write(&junk, b"not a png").unwrap();
        assert!(decode(&junk, 9).is_err());
        assert!(decode(&junk, MAX_IMAGE_FILE + 1).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
