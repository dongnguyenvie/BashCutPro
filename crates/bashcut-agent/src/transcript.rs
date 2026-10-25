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
    /// AGU10: a turn that finished: steps (`num_turns`, or the tool calls it made), cost when the
    /// agent reports one and the wall-clock time the session measured. Every field may be missing.
    TurnEnd {
        #[serde(default)]
        ok: bool,
        #[serde(default)]
        turns: Option<u64>,
        #[serde(default)]
        cost_usd: Option<f64>,
        #[serde(default)]
        ms: Option<u64>,
        /// AGM1: The model from the session's `system/init`; empty when the agent did not report one.
        #[serde(default)]
        model: String,
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
    /// AGU11: the assistant item that streamed text is growing (item index, byte offset where the
    /// streamed block starts). Never saved; the block's final [`Event::Text`] replaces it.
    #[serde(skip)]
    live: Option<(usize, usize)>,
}

impl Transcript {
    pub fn push(&mut self, item: Item) {
        if self.items.len() >= MAX_ITEMS {
            let extra = self.items.len() + 1 - MAX_ITEMS;
            self.items.drain(..extra);
        }
        self.items.push(item);
    }

    /// Add what `e` shows. `Init` and `Picked` change the conversation, not the transcript; a failed
    /// result shows as an error and a successful one as an [`Item::TurnEnd`].
    pub fn apply(&mut self, e: &Event) {
        self.apply_timed(e, None, "");
    }

    /// [`Self::apply`] with the turn's wall-clock time in ms (the session measures it from the
    /// message it wrote to the result), which a successful result's [`Item::TurnEnd`] keeps.
    /// `init_model` is the model from the session's last `system/init` (AGM1).
    pub fn apply_timed(&mut self, e: &Event, ms: Option<u64>, init_model: &str) {
        if !matches!(e, Event::TextDelta(_) | Event::Text(_)) {
            self.live = None;
        }
        match e {
            Event::TextDelta(d) => self.stream(d),
            Event::Text(t) if self.live.is_some() => {
                // the finished block replaces its provisional text
                if let Some((at, start)) = self.live.take()
                    && let Some(Item::Assistant { text }) = self.items.get_mut(at)
                {
                    text.truncate(start);
                    text.push_str(t);
                }
            }
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
            Event::Result { ok: true, cost_usd, turns, .. } => {
                // Codex sends no step count: the tool calls since the last message
                let tools = self.items.iter().rev().take_while(|i| !matches!(i, Item::User { .. })).filter(|i| matches!(i, Item::Tool { .. })).count();
                let turns = turns.or(u64::try_from(tools).ok().filter(|n| *n > 0));
                // AGM1: record the model so the turn summary line can show it
                self.push(Item::TurnEnd { ok: true, turns, cost_usd: *cost_usd, ms, model: init_model.to_string() });
            }
            Event::Notice(m) => self.push(Item::Notice { message: m.clone() }),
            Event::Init { .. } | Event::Picked { .. } | Event::Control { .. } => {}
            // the session keeps open requests; the answer is added as an Item::Permission
            Event::Permission(_) | Event::PermissionWithdrawn { .. } => {}
        }
    }

    /// Grow the provisional assistant text with a streamed piece; a new block after text joins it
    /// like consecutive text blocks do.
    fn stream(&mut self, d: &str) {
        match self.live {
            Some((at, _)) => {
                if let Some(Item::Assistant { text }) = self.items.get_mut(at)
                    && text.len() < crate::stream_json::MAX_TEXT
                {
                    text.push_str(d);
                }
            }
            None => {
                let at = self.items.len().saturating_sub(1);
                if let Some(Item::Assistant { text }) = self.items.last_mut()
                    && text.len() < crate::stream_json::MAX_TEXT
                {
                    text.push_str("\n\n");
                    let start = text.len();
                    text.push_str(d);
                    self.live = Some((at, start));
                } else {
                    self.push(Item::Assistant { text: d.to_string() });
                    self.live = Some((self.items.len().saturating_sub(1), 0));
                }
            }
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
    fn streamed_text_is_replaced_by_the_message() {
        let delta = |d: &str| Event::TextDelta(d.into());
        let mut t = Transcript::default();
        t.apply(&Event::Text("first".into()));
        // the next block streams: it grows after a blank line, in the same item
        t.apply(&delta("Cut"));
        t.apply(&delta("ting"));
        assert_eq!(t.items, vec![Item::Assistant { text: "first\n\nCutting".into() }]);
        // the finished block replaces its provisional text, no duplicate
        t.apply(&Event::Text("Cutting now.".into()));
        assert_eq!(t.items, vec![Item::Assistant { text: "first\n\nCutting now.".into() }]);
        // with no live text a Text joins like before
        t.apply(&Event::Text("more".into()));
        assert_eq!(t.items, vec![Item::Assistant { text: "first\n\nCutting now.\n\nmore".into() }]);
        // the live item is never saved: a reloaded transcript has a plain item to stream after
        let mut back: Transcript = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        back.apply(&delta("x"));
        assert_eq!(back.items.len(), 1);
        assert!(matches!(&back.items[0], Item::Assistant { text } if text.ends_with("more\n\nx")));
    }

    #[test]
    fn streaming_starts_and_stops_with_the_turn() {
        let delta = |d: &str| Event::TextDelta(d.into());
        let mut t = Transcript::default();
        t.push(Item::User { text: "hi".into() });
        t.apply(&delta("Hel"));
        t.apply(&delta("lo"));
        assert_eq!(t.items[1], Item::Assistant { text: "Hello".into() });
        // a tool call ends the live text: a later Text is a new item after it
        t.apply(&Event::ToolUse { id: "1".into(), name: "Bash".into(), input: json!({"command": "ls"}) });
        t.apply(&Event::Text("Hello".into()));
        assert_eq!(t.items.len(), 4, "{:?}", t.items);
        assert_eq!(t.items[1], Item::Assistant { text: "Hello".into() });
        // a Text without deltas before it (Codex) is unchanged
        let mut codex = Transcript::default();
        codex.apply(&Event::Text("a".into()));
        codex.apply(&Event::ToolUse { id: "1".into(), name: "shell".into(), input: json!({}) });
        codex.apply(&Event::Text("b".into()));
        assert_eq!(codex.items.len(), 3);
        // deltas stop growing a message at the cap
        let mut big = Transcript::default();
        big.apply(&delta(&"x".repeat(crate::stream_json::MAX_TEXT)));
        big.apply(&delta("more"));
        assert!(matches!(&big.items[0], Item::Assistant { text } if text.len() == crate::stream_json::MAX_TEXT));
    }

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
    fn auth_problems_fold_and_clear() {
        let auth = |category: &str, message: &str| Event::Auth { category: category.into(), message: message.into() };
        let failed = Event::Result { ok: false, text: "Not logged in · Please run /login".into(), session: "s".into(), cost_usd: None, turns: None };
        let mut t = Transcript::default();
        t.push(Item::User { text: "cut it".into() });
        assert_eq!(t.auth_problem(), None);
        // retries without a message, then the assistant's words, then the result's: one item
        t.apply(&auth("authentication_failed", ""));
        t.apply(&auth("authentication_failed", ""));
        t.apply(&auth("authentication_failed", "Not logged in · Please run /login"));
        t.apply(&auth("authentication_failed", "something else"));
        t.apply(&failed);
        assert_eq!(t.items.len(), 2, "{:?}", t.items);
        assert_eq!(t.items[1], Item::Auth { category: "authentication_failed".into(), message: "Not logged in · Please run /login".into() });
        assert_eq!(t.auth_problem(), Some(("authentication_failed", "Not logged in · Please run /login")));
        // Init, Control and a successful result do not hide it
        t.apply(&Event::Init { session: "s".into(), model: "m".into(), capabilities: Vec::new(), plugins: Vec::new(), commands: Vec::new() });
        t.apply(&Event::Result { ok: true, text: String::new(), session: "s".into(), cost_usd: None, turns: None });
        assert_eq!(t.auth_problem().map(|a| a.0), Some("authentication_failed"));
        // another category is its own item and the latest problem
        t.apply(&auth("billing_error", "Credit balance is too low"));
        assert_eq!(t.items.len(), 4);
        assert_eq!(t.auth_problem(), Some(("billing_error", "Credit balance is too low")));
        // a later message clears it; a failed result then is a plain error again
        t.push(Item::User { text: "again".into() });
        assert_eq!(t.auth_problem(), None);
        t.apply(&Event::Result { ok: false, text: "error_max_turns".into(), session: "s".into(), cost_usd: None, turns: None });
        assert_eq!(t.items.last(), Some(&Item::Error { message: "error_max_turns".into() }));
        // an Auth after other items (not right after an Auth) is pushed again
        t.apply(&auth("billing_error", ""));
        assert_eq!(t.items.last(), Some(&Item::Auth { category: "billing_error".into(), message: String::new() }));
        assert_eq!(t.auth_problem(), Some(("billing_error", "")));
        // items survive the saved form
        let back: Transcript = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn turn_end_item() {
        let result = |cost_usd, turns| Event::Result { ok: true, text: String::new(), session: "s".into(), cost_usd, turns };
        // Claude Code: its own count and cost, and the time the session measured
        let mut t = Transcript::default();
        t.apply_timed(&result(Some(0.12), Some(9)), Some(4200), "claude-sonnet-4-6");
        assert_eq!(t.items, vec![Item::TurnEnd { ok: true, turns: Some(9), cost_usd: Some(0.12), ms: Some(4200), model: "claude-sonnet-4-6".into() }]);
        // without any of it: no steps, no cost, no time
        let mut t = Transcript::default();
        t.apply(&result(None, None));
        assert_eq!(t.items, vec![Item::TurnEnd { ok: true, turns: None, cost_usd: None, ms: None, model: String::new() }]);
        // Codex: steps are the tool calls since the user's message
        let mut t = Transcript::default();
        t.push(Item::User { text: "old".into() });
        t.apply(&Event::ToolUse { id: "0".into(), name: "shell".into(), input: json!({}) });
        t.push(Item::User { text: "new".into() });
        for i in 1..=3 {
            t.apply(&Event::ToolUse { id: i.to_string(), name: "shell".into(), input: json!({}) });
        }
        t.apply(&result(None, None));
        assert!(matches!(t.items.last(), Some(Item::TurnEnd { turns: Some(3), cost_usd: None, .. })), "{:?}", t.items.last());
        // a reported count wins over the tool count
        t.apply(&result(None, Some(2)));
        assert!(matches!(t.items.last(), Some(Item::TurnEnd { turns: Some(2), .. })));
        // a failed result is an error, not a turn end
        let mut t = Transcript::default();
        t.apply(&Event::Result { ok: false, text: "boom".into(), session: "s".into(), cost_usd: Some(1.0), turns: Some(1) });
        assert_eq!(t.items, vec![Item::Error { message: "boom".into() }]);
        // saved conversations from before TurnEnd load, and a TurnEnd with missing fields loads
        let old: Transcript = serde_json::from_value(json!({"items": [{"type": "user", "text": "hi"}, {"type": "assistant", "text": "yo"}]})).unwrap();
        assert_eq!(old.items.len(), 2);
        let sparse: Transcript = serde_json::from_value(json!({"items": [{"type": "turnEnd"}]})).unwrap();
        // AGM1: model defaults to empty string (old saved conversations have no model field)
        assert_eq!(sparse.items, vec![Item::TurnEnd { ok: false, turns: None, cost_usd: None, ms: None, model: String::new() }]);
        let full = Transcript {
            items: vec![Item::TurnEnd { ok: true, turns: Some(9), cost_usd: Some(0.12), ms: Some(5), model: "sonnet".into() }],
            ..Default::default()
        };
        let back: Transcript = serde_json::from_value(serde_json::to_value(&full).unwrap()).unwrap();
        assert_eq!(back, full);
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
