//! The transcript a conversation shows: messages, tool calls (with their results), answered
//! gates, errors and notices. Built from [`Event`]s; saved with the conversation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::stream_json::Event;

/// Items kept per conversation; the oldest go first.
pub const MAX_ITEMS: usize = 2000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Item {
    User {
        text: String,
    },
    Assistant {
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    Tool {
        id: String,
        name: String,
        input: Value,
        output: Option<String>,
        is_error: bool,
    },
    /// A gate the user answered (`approved` or `changes`) and their note.
    Gate {
        stage: String,
        state: String,
        note: String,
    },
    Error {
        message: String,
    },
    Notice {
        message: String,
    },
    /// AG8: a permission request the user answered (`allowed`, `always`, `denied`; `withdrawn` when
    /// the agent stopped waiting first).
    Permission {
        tool: String,
        detail: String,
        answer: String,
    },
    /// AG6: the agent is not signed in or its account cannot be used ([`Event::Auth`]); the panel
    /// shows a readiness line with Sign in while this is the latest turn's outcome.
    Auth {
        category: String,
        message: String,
    },
}

impl Item {
    /// What tool calls are grouped by in the panel: the FilmCraft command id when the call names
    /// one (`command`, `id` or `method` in its input), else the tool's name without the MCP prefix.
    pub fn tool_key(&self) -> Option<String> {
        let Item::Tool { name, input, .. } = self else { return None };
        let cmd = ["command", "id", "method"].iter().find_map(|k| input.get(*k).and_then(Value::as_str));
        Some(match cmd {
            Some(c) => c.to_string(),
            None => name.rsplit("__").next().unwrap_or(name).to_string(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub items: Vec<Item>,
}

impl Transcript {
    pub fn push(&mut self, item: Item) {
        if self.items.len() >= MAX_ITEMS {
            let extra = self.items.len() + 1 - MAX_ITEMS;
            self.items.drain(..extra);
        }
        self.items.push(item);
    }

    /// Add what `e` shows. `Init`, `Picked` and `Result` change the conversation, not the transcript,
    /// except a failed result, which shows as an error.
    pub fn apply(&mut self, e: &Event) {
        match e {
            Event::Text(t) => {
                // consecutive text blocks read as one message
                if let Some(Item::Assistant { text }) = self.items.last_mut()
                    && text.len() < crate::stream_json::MAX_TEXT
                {
                    text.push_str("\n\n");
                    text.push_str(t);
                } else {
                    self.push(Item::Assistant { text: t.clone() });
                }
            }
            Event::ToolUse { id, name, input } => {
                self.push(Item::Tool { id: id.clone(), name: name.clone(), input: input.clone(), output: None, is_error: false })
            }
            Event::ToolResult { id, text, is_error } => {
                let found = self.items.iter_mut().rev().take(200).find(|i| matches!(i, Item::Tool { id: tid, .. } if tid == id));
                if let Some(Item::Tool { output, is_error: err, .. }) = found {
                    *output = Some(text.clone());
                    *err = *is_error;
                }
            }
            Event::Auth { category, message } => {
                // the same problem reported again (retries, then the result) shows once
                let item = Item::Auth { category: category.clone(), message: message.clone() };
                match self.items.last_mut() {
                    Some(Item::Auth { category: c, message: m }) if c == category => {
                        if m.is_empty() {
                            *m = message.clone();
                        }
                    }
                    _ => self.push(item),
                }
            }
            // the turn already ended on a sign-in problem: no generic error after it
            Event::Result { ok: false, .. } if matches!(self.items.last(), Some(Item::Auth { .. })) => {}
            Event::Result { ok: false, text, .. } => {
                let message = if text.is_empty() { "the agent stopped with an error".to_string() } else { text.clone() };
                self.push(Item::Error { message });
            }
            Event::Notice(m) => self.push(Item::Notice { message: m.clone() }),
            Event::Init { .. } | Event::Result { .. } | Event::Picked { .. } | Event::Control { .. } => {}
            // the session keeps open requests; the answer is added as an Item::Permission
            Event::Permission(_) | Event::PermissionWithdrawn { .. } => {}
        }
    }

    /// The sign-in or account problem the latest turn ended on (category, message): the last
    /// [`Item::Auth`] when no message was sent after it.
    pub fn auth_problem(&self) -> Option<(&str, &str)> {
        self.items.iter().rev().find_map(|i| match i {
            Item::Auth { category, message } => Some(Some((category.as_str(), message.as_str()))),
            Item::User { .. } => Some(None),
            _ => None,
        })?
    }

    /// The last assistant text (what the turn answered).
    pub fn last_answer(&self) -> Option<&str> {
        self.items.iter().rev().find_map(|i| match i {
            Item::Assistant { text } => Some(text.as_str()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_from_events() {
        let mut t = Transcript::default();
        t.apply(&Event::Text("a".into()));
        t.apply(&Event::Text("b".into()));
        t.apply(&Event::ToolUse { id: "1".into(), name: "mcp__bashcutpro__command_execute".into(), input: json!({"command": "edit.razor"}) });
        t.apply(&Event::ToolResult { id: "1".into(), text: "done".into(), is_error: false });
        t.apply(&Event::ToolResult { id: "nope".into(), text: "x".into(), is_error: true });
        t.apply(&Event::Result { ok: false, text: String::new(), session: "s".into(), cost_usd: None, turns: None });
        assert_eq!(t.items.len(), 3);
        assert_eq!(t.items[0], Item::Assistant { text: "a\n\nb".into() });
        assert!(matches!(&t.items[1], Item::Tool { output: Some(o), .. } if o == "done"));
        assert_eq!(t.items[1].tool_key().as_deref(), Some("edit.razor"));
        assert!(matches!(&t.items[2], Item::Error { .. }));
        let other = Item::Tool { id: String::new(), name: "mcp__bashcutpro__doc_inspect".into(), input: json!({}), output: None, is_error: false };
        assert_eq!(other.tool_key().as_deref(), Some("doc_inspect"));
        assert_eq!(t.last_answer(), Some("a\n\nb"));
    }

    #[test]
    fn keeps_at_most_max_items() {
        let mut t = Transcript::default();
        for i in 0..MAX_ITEMS + 5 {
            t.push(Item::User { text: i.to_string() });
        }
        assert_eq!(t.items.len(), MAX_ITEMS);
        assert_eq!(t.items[0], Item::User { text: "5".into() });
    }
}
