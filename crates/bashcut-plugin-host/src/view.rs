//! `bashcut-view/1`: a small form a plugin shows in its card (`bashcut/plugin-host.md` §views).
//!
//! An action marked `"view": true` prints view objects on stdout while it runs (one JSON object per
//! line, each replacing the last), and may put a final one in its result as `data.view`. A view is
//! a title and a list of rows: text, input, select, toggle, button, list (rows with their own
//! buttons), image and progress. A button runs another action of the same plugin with the view's
//! current values as params.
//!
//! Views are hostile input: rows are read one by one (an unknown or broken row is skipped, not the
//! whole view), and counts and strings are capped.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SCHEMA: &str = "bashcut-view/1";
/// Rows kept per view.
pub const MAX_ROWS: usize = 200;
/// Items kept per list.
pub const MAX_ITEMS: usize = 1000;
/// Buttons kept per list item.
pub const MAX_ITEM_BUTTONS: usize = 6;
/// Choices kept per select.
pub const MAX_CHOICES: usize = 200;
/// Longest text kept (labels, values, details).
pub const MAX_TEXT: usize = 16 * 1024;
/// Longest view line read from stdout.
pub const MAX_VIEW_LINE: usize = 1024 * 1024;
const MAX_ID: usize = 64;
const MAX_LABEL: usize = 256;

/// One view.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub title: String,
    pub rows: Vec<Row>,
}

/// One row of a view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Row {
    Text {
        text: String,
        muted: bool,
    },
    Input {
        id: String,
        label: String,
        value: String,
        multiline: bool,
    },
    Select {
        id: String,
        label: String,
        value: String,
        choices: Vec<Choice>,
    },
    Toggle {
        id: String,
        label: String,
        value: bool,
    },
    /// Runs `action` with the view's values plus `params`.
    Button {
        id: String,
        label: String,
        action: String,
        params: Map<String, Value>,
    },
    List {
        id: String,
        label: String,
        items: Vec<Item>,
    },
    /// A picture file (PNG) under the plugin's folder or the project's.
    Image {
        path: String,
        caption: String,
    },
    /// `value` in 0..=1, or `None` for a busy bar.
    Progress {
        value: Option<f32>,
        label: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

/// A list row: its text, a muted detail line and its buttons (each gets `{"item": id}` added).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub text: String,
    pub detail: String,
    pub buttons: Vec<ItemButton>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemButton {
    pub id: String,
    pub label: String,
    pub action: String,
    pub params: Map<String, Value>,
}

impl View {
    /// The view in `v` if it says `"schema": "bashcut-view/1"`.
    pub fn from_value(v: &Value) -> Option<View> {
        let o = v.as_object()?;
        if o.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
            return None;
        }
        let rows = o.get("rows").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        Some(View { title: text(o.get("title"), MAX_LABEL), rows: rows.iter().filter_map(row).take(MAX_ROWS).collect() })
    }

    /// The view on one stdout line, if the line is one.
    pub fn from_line(line: &str) -> Option<View> {
        let t = line.trim();
        if !t.starts_with('{') || !t.contains(SCHEMA) || t.len() > MAX_VIEW_LINE {
            return None;
        }
        View::from_value(&serde_json::from_str::<Value>(t).ok()?)
    }

    /// The value ids of the inputs, selects and toggles (what a button sends).
    pub fn fields(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|r| matches!(r, Row::Input { .. } | Row::Select { .. } | Row::Toggle { .. }))
    }

    /// The action and params of the button `id`, or of the button `button` on item `item` of list
    /// `id` (with `"item"` set), before the view's values are added.
    pub fn button(&self, id: &str, item: Option<(&str, &str)>) -> Option<(String, Map<String, Value>)> {
        self.rows.iter().find_map(|r| match (r, item) {
            (Row::Button { id: b, action, params, .. }, None) if b == id => Some((action.clone(), params.clone())),
            (Row::List { id: l, items, .. }, Some((item, button))) if l == id => {
                let it = items.iter().find(|it| it.id == item)?;
                let b = it.buttons.iter().find(|b| b.id == button)?;
                let mut params = b.params.clone();
                params.insert("item".into(), Value::String(it.id.clone()));
                Some((b.action.clone(), params))
            }
            _ => None,
        })
    }
}

fn row(v: &Value) -> Option<Row> {
    let o = v.as_object()?;
    let s = |k: &str, cap: usize| text(o.get(k), cap);
    let id = || ident(o.get("id"));
    Some(match o.get("type")?.as_str()? {
        "text" => Row::Text { text: s("text", MAX_TEXT), muted: o.get("muted").and_then(Value::as_bool).unwrap_or(false) },
        "input" => Row::Input {
            id: id()?,
            label: s("label", MAX_LABEL),
            value: s("value", MAX_TEXT),
            multiline: o.get("multiline").and_then(Value::as_bool).unwrap_or(false),
        },
        "select" => {
            let choices = o.get("choices").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            Row::Select {
                id: id()?,
                label: s("label", MAX_LABEL),
                value: s("value", MAX_LABEL),
                choices: choices.iter().filter_map(choice).take(MAX_CHOICES).collect(),
            }
        }
        "toggle" => Row::Toggle { id: id()?, label: s("label", MAX_LABEL), value: o.get("value").and_then(Value::as_bool).unwrap_or(false) },
        "button" => Row::Button { id: id()?, label: s("label", MAX_LABEL), action: ident(o.get("action"))?, params: params(o.get("params")) },
        "list" => {
            let items = o.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            Row::List { id: id()?, label: s("label", MAX_LABEL), items: items.iter().filter_map(item).take(MAX_ITEMS).collect() }
        }
        "image" => Row::Image { path: s("path", 4096), caption: s("caption", MAX_LABEL) },
        "progress" => {
            let value = o.get("value").and_then(Value::as_f64).filter(|f| f.is_finite()).map(|f| if f > 1.0 { f / 100.0 } else { f });
            Row::Progress { value: value.map(|f| f.clamp(0.0, 1.0) as f32), label: s("label", MAX_LABEL) }
        }
        _ => return None,
    })
}

fn choice(v: &Value) -> Option<Choice> {
    match v {
        Value::String(s) => Some(Choice { value: clip(s, MAX_LABEL), label: clip(s, MAX_LABEL) }),
        Value::Object(o) => {
            let value = text(o.get("value"), MAX_LABEL);
            let label = text(o.get("label"), MAX_LABEL);
            Some(Choice { label: if label.is_empty() { value.clone() } else { label }, value })
        }
        _ => None,
    }
}

fn item(v: &Value) -> Option<Item> {
    let o = v.as_object()?;
    let buttons = o.get("buttons").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let buttons = buttons
        .iter()
        .filter_map(|b| {
            let b = b.as_object()?;
            Some(ItemButton {
                id: ident(b.get("id"))?,
                label: text(b.get("label"), MAX_LABEL),
                action: ident(b.get("action"))?,
                params: params(b.get("params")),
            })
        })
        .take(MAX_ITEM_BUTTONS)
        .collect();
    Some(Item { id: ident(o.get("id"))?, text: text(o.get("text"), MAX_TEXT), detail: text(o.get("detail"), MAX_TEXT), buttons })
}

/// Button params: an object of at most 64 entries and 16 KiB.
fn params(v: Option<&Value>) -> Map<String, Value> {
    match v {
        Some(Value::Object(o)) if o.len() <= 64 && serde_json::to_string(o).is_ok_and(|t| t.len() <= 16 * 1024) => o.clone(),
        _ => Map::new(),
    }
}

/// An id: 1–64 chars of letters, digits, `.`, `-`, `_`, `:`.
fn ident(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    let ok = !s.is_empty() && s.len() <= MAX_ID && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':'));
    ok.then(|| s.to_string())
}

fn text(v: Option<&Value>, cap: usize) -> String {
    match v {
        Some(Value::String(s)) => clip(s, cap),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// At most `cap` bytes, cut at a char boundary.
fn clip(s: &str, cap: usize) -> String {
    bashcut_dirs::fs::clip(s, cap, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_every_row_type() {
        let v = json!({"schema": SCHEMA, "title": "Review", "rows": [
            {"type": "text", "text": "3 captions", "muted": true},
            {"type": "input", "id": "lang", "label": "Language", "value": "vi"},
            {"type": "select", "id": "tone", "value": "plain", "choices": ["plain", {"value": "formal", "label": "Formal"}]},
            {"type": "toggle", "id": "keep", "label": "Keep timing", "value": true},
            {"type": "button", "id": "apply", "label": "Apply", "action": "apply", "params": {"all": true}},
            {"type": "list", "id": "lines", "items": [{"id": "c1", "text": "Xin chào", "detail": "Hello", "buttons": [{"id": "ok", "label": "OK", "action": "accept"}]}]},
            {"type": "image", "path": "thumb.png", "caption": "frame"},
            {"type": "progress", "value": 40},
            {"type": "chart"}
        ]});
        let view = View::from_value(&v).unwrap();
        assert_eq!(view.title, "Review");
        assert_eq!(view.rows.len(), 8, "the unknown row is skipped");
        assert!(matches!(&view.rows[2], Row::Select { choices, .. } if choices[1].label == "Formal" && choices[0].label == "plain"));
        assert!(matches!(view.rows[7], Row::Progress { value: Some(v), .. } if (v - 0.4).abs() < 1e-6));
        assert_eq!(view.fields().count(), 3);
        let (action, params) = view.button("apply", None).unwrap();
        assert_eq!((action.as_str(), &params["all"]), ("apply", &json!(true)));
        let (action, params) = view.button("lines", Some(("c1", "ok"))).unwrap();
        assert_eq!((action.as_str(), &params["item"]), ("accept", &json!("c1")));
        assert!(view.button("lines", Some(("c9", "ok"))).is_none());
        assert!(view.button("nope", None).is_none());
    }

    #[test]
    fn hostile_views_are_capped_or_refused() {
        assert!(View::from_value(&json!({"schema": "bashcut-view/2", "rows": []})).is_none());
        assert!(View::from_value(&json!([1])).is_none());
        assert!(View::from_line("progress 40%").is_none());
        assert!(View::from_line(r#"{"ok": true}"#).is_none());
        let line = format!(r#"{{"schema":"{SCHEMA}","title":"t","rows":[{{"type":"text","text":"hi"}}]}}"#);
        assert_eq!(View::from_line(&line).map(|v| v.rows.len()), Some(1));
        let rows: Vec<Value> = (0..MAX_ROWS + 50).map(|_| json!({"type": "text", "text": "x".repeat(MAX_TEXT + 10)})).collect();
        let view = View::from_value(&json!({"schema": SCHEMA, "rows": rows, "title": 7})).unwrap();
        assert_eq!(view.rows.len(), MAX_ROWS);
        assert!(matches!(&view.rows[0], Row::Text { text, .. } if text.len() == MAX_TEXT));
        // rows without the ids they need, bad ids and bad actions are skipped
        let view = View::from_value(&json!({"schema": SCHEMA, "rows": [
            {"type": "input"}, {"type": "button", "id": "a b", "action": "x"}, {"type": "button", "id": "b", "action": "../x"},
            {"type": "progress", "value": "NaN"}, {"type": 5}, "text"
        ]}))
        .unwrap();
        assert_eq!(view.rows, vec![Row::Progress { value: None, label: String::new() }]);
        assert_eq!(clip("héllo", 2), "h");
    }
}
