//! `bashcut-view/1`: the declarative panel a plugin shows in the app (`bashcut/in-app-ui.md` §7).
//!
//! A plugin prints a view as one JSON object on a stdout line while an action runs (each new line replaces the view,
//! so a running action can update it live), or returns one as `data.view` in its `bashcut-result/1`. Rows:
//!
//! | `type` | fields |
//! |---|---|
//! | `text` | `text`, `style`: `body` \| `title` \| `dim` \| `error` |
//! | `input` | `id`, `label`, `value`, `multiline` |
//! | `select` | `id`, `label`, `options` [str], `value` |
//! | `toggle` | `id`, `label`, `value` (bool) |
//! | `button` | `label`, `action`, `params` {}, `primary` |
//! | `list` | `id`, `items` [{`id`, `text`, `detail`, `buttons` [{`label`, `action`, `params`}]}] |
//! | `image` | `path` (a file in the plugin's folder or the temp dir), `width` |
//! | `progress` | `value` 0..1, `label` |
//!
//! A button runs another action of the same plugin with its `params` plus the current values of the view's inputs
//! (`{id: value}`), and a list row button adds `item` (the row's id). Everything is hostile input: unknown rows are
//! dropped, strings and lists are capped, numbers are clamped.

use serde::Serialize;
use serde_json::{Map, Value};

pub const SCHEMA: &str = "bashcut-view/1";
pub const MAX_ROWS: usize = 200;
pub const MAX_ITEMS: usize = 1000;
pub const MAX_BUTTONS: usize = 8;
pub const MAX_OPTIONS: usize = 200;
pub const MAX_TEXT: usize = 8 * 1024;
const MAX_ID: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct View {
    pub title: String,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Row {
    Text { text: String, style: String },
    Input { id: String, label: String, value: String, multiline: bool },
    Select { id: String, label: String, options: Vec<String>, value: String },
    Toggle { id: String, label: String, value: bool },
    Button(Button),
    List { id: String, items: Vec<Item> },
    Image { path: String, width: f32 },
    Progress { value: f32, label: String },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Button {
    pub label: String,
    pub action: String,
    pub params: Map<String, Value>,
    pub primary: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Item {
    pub id: String,
    pub text: String,
    pub detail: String,
    pub buttons: Vec<Button>,
}

fn text_of(v: &Value, k: &str, cap: usize) -> String {
    crate::clip(v.get(k).and_then(Value::as_str).unwrap_or_default(), cap)
}

fn id_of(v: &Value) -> Option<String> {
    let id = v.get("id").and_then(Value::as_str)?;
    (!id.is_empty() && id.len() <= MAX_ID && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))).then(|| id.to_string())
}

fn button(v: &Value) -> Option<Button> {
    let action = v.get("action").and_then(Value::as_str).filter(|a| crate::valid_id(a))?.to_string();
    let params = match v.get("params") {
        Some(Value::Object(m)) if m.len() <= 64 => m.clone(),
        _ => Map::new(),
    };
    Some(Button { label: text_of(v, "label", 128), action, params, primary: v.get("primary").and_then(Value::as_bool).unwrap_or(false) })
}

fn row(v: &Value) -> Option<Row> {
    let kind = v.get("type").and_then(Value::as_str)?;
    Some(match kind {
        "text" => {
            let style = match v.get("style").and_then(Value::as_str) {
                Some(s @ ("title" | "dim" | "error")) => s.to_string(),
                _ => "body".to_string(),
            };
            Row::Text { text: text_of(v, "text", MAX_TEXT), style }
        }
        "input" => Row::Input {
            id: id_of(v)?,
            label: text_of(v, "label", 256),
            value: text_of(v, "value", MAX_TEXT),
            multiline: v.get("multiline").and_then(Value::as_bool).unwrap_or(false),
        },
        "select" => {
            let options: Vec<String> = v
                .get("options")
                .and_then(Value::as_array)
                .map(|a| a.iter().take(MAX_OPTIONS).filter_map(Value::as_str).map(|s| crate::clip(s, 256)).collect())
                .unwrap_or_default();
            let value = text_of(v, "value", 256);
            let value = if options.contains(&value) { value } else { options.first().cloned().unwrap_or_default() };
            Row::Select { id: id_of(v)?, label: text_of(v, "label", 256), options, value }
        }
        "toggle" => Row::Toggle { id: id_of(v)?, label: text_of(v, "label", 256), value: v.get("value").and_then(Value::as_bool).unwrap_or(false) },
        "button" => Row::Button(button(v)?),
        "list" => {
            let items = v
                .get("items")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .take(MAX_ITEMS)
                        .map(|it| Item {
                            id: id_of(it).unwrap_or_default(),
                            text: text_of(it, "text", 2048),
                            detail: text_of(it, "detail", 2048),
                            buttons: it
                                .get("buttons")
                                .and_then(Value::as_array)
                                .map(|b| b.iter().take(MAX_BUTTONS).filter_map(button).collect())
                                .unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            Row::List { id: id_of(v).unwrap_or_default(), items }
        }
        "image" => {
            let path = text_of(v, "path", 4096);
            if path.is_empty() {
                return None;
            }
            let width = v.get("width").and_then(Value::as_f64).filter(|w| w.is_finite()).unwrap_or(0.0).clamp(0.0, 2048.0) as f32;
            Row::Image { path, width }
        }
        "progress" => {
            let value = v.get("value").and_then(Value::as_f64).filter(|p| p.is_finite()).unwrap_or(0.0);
            let value = if value > 1.0 { value / 100.0 } else { value };
            Row::Progress { value: value.clamp(0.0, 1.0) as f32, label: text_of(v, "label", 256) }
        }
        _ => return None,
    })
}

/// A view from JSON: an object with `"schema": "bashcut-view/1"` and `rows`.
pub fn parse(v: &Value) -> Option<View> {
    if v.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
        return None;
    }
    let rows = v.get("rows").and_then(Value::as_array).map(|a| a.iter().take(MAX_ROWS).filter_map(row).collect()).unwrap_or_default();
    Some(View { title: text_of(v, "title", 256), rows })
}

/// A view from one stdout line (`None` for any other line).
pub fn parse_line(line: &str) -> Option<View> {
    let t = line.trim();
    if !t.starts_with('{') || !t.contains(SCHEMA) {
        return None;
    }
    parse(&serde_json::from_str::<Value>(t).ok()?)
}

/// The input values a view starts with: `{id: value}` of its inputs, selects and toggles.
pub fn initial_values(view: &View) -> Map<String, Value> {
    let mut m = Map::new();
    for r in &view.rows {
        match r {
            Row::Input { id, value, .. } | Row::Select { id, value, .. } => {
                m.insert(id.clone(), Value::String(value.clone()));
            }
            Row::Toggle { id, value, .. } => {
                m.insert(id.clone(), Value::Bool(*value));
            }
            _ => {}
        }
    }
    m
}

/// The params a button press sends: the view's current values, then the button's own params, then `item`.
pub fn press_params(button: &Button, values: &Map<String, Value>, item: Option<&str>) -> Map<String, Value> {
    let mut p = values.clone();
    for (k, v) in &button.params {
        p.insert(k.clone(), v.clone());
    }
    if let Some(it) = item.filter(|i| !i.is_empty()) {
        p.insert("item".into(), Value::String(it.to_string()));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_every_row_kind() {
        let v = json!({"schema": SCHEMA, "title": "Review", "rows": [
            {"type": "text", "text": "12 lines", "style": "title"},
            {"type": "input", "id": "glossary", "label": "Glossary", "value": "cut=cắt"},
            {"type": "select", "id": "tone", "options": ["natural", "formal"], "value": "formal"},
            {"type": "toggle", "id": "blur", "label": "Blur", "value": true},
            {"type": "button", "label": "Apply", "action": "apply", "params": {"target": "vi"}, "primary": true},
            {"type": "list", "id": "lines", "items": [{"id": "c1", "text": "Xin chào", "detail": "Hello", "buttons": [{"label": "Redo", "action": "translate"}]}]},
            {"type": "image", "path": "frame.png", "width": 160},
            {"type": "progress", "value": 40, "label": "translating"},
            {"type": "webview", "url": "https://x"}
        ]});
        let view = parse(&v).unwrap();
        assert_eq!(view.title, "Review");
        assert_eq!(view.rows.len(), 8, "the unknown row is dropped");
        assert!(matches!(&view.rows[7], Row::Progress { value, .. } if (*value - 0.4).abs() < 1e-6));
        let values = initial_values(&view);
        assert_eq!(values["tone"], "formal");
        assert_eq!(values["blur"], true);
        let Row::List { items, .. } = &view.rows[5] else { panic!("list") };
        let p = press_params(&items[0].buttons[0], &values, Some("c1"));
        assert_eq!(p["item"], "c1");
        assert_eq!(p["glossary"], "cut=cắt");
    }

    #[test]
    fn hostile_views_are_tamed() {
        assert!(parse(&json!({"rows": []})).is_none(), "no schema");
        assert!(parse_line("progress 40%").is_none());
        assert!(parse_line(r#"{"ok": true}"#).is_none());
        let rows: Vec<Value> = (0..MAX_ROWS * 2).map(|i| json!({"type": "text", "text": i.to_string()})).collect();
        assert_eq!(parse(&json!({"schema": SCHEMA, "rows": rows})).unwrap().rows.len(), MAX_ROWS);
        let v = parse(&json!({"schema": SCHEMA, "rows": [
            {"type": "input", "id": "../x"},
            {"type": "button", "label": "x", "action": "Bad Action!"},
            {"type": "select", "id": "s", "options": ["a"], "value": "zzz"},
            {"type": "progress", "value": "NaN"},
            {"type": "text", "text": "x".repeat(MAX_TEXT * 3)},
            {"type": "image", "path": ""}
        ]}))
        .unwrap();
        assert_eq!(v.rows.len(), 3, "{:?}", v.rows);
        assert!(matches!(&v.rows[0], Row::Select { value, .. } if value == "a"));
        assert!(matches!(&v.rows[2], Row::Text { text, .. } if text.len() <= MAX_TEXT + 4));
        let line = format!(r#"{{"schema":"{SCHEMA}","rows":[{{"type":"text","text":"live"}}]}}"#);
        assert_eq!(parse_line(&line).unwrap().rows.len(), 1);
    }
}
