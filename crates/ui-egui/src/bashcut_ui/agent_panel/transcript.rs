//! The transcript: one fn per kind of item; consecutive tool calls with the same key fold into one row.

use bashcut_agent::Item;

use super::super::strings::{f, s};
use super::super::widgets::{CODE, META, NOTE, code_note, note, strong_note};
use super::cards::permission_line;
use super::tools::tool_group;
use crate::FilmcraftApp;

pub(super) fn transcript(app: &mut FilmcraftApp, ui: &mut egui::Ui, items: &[Item]) {
    let mut i = 0;
    while let Some(item) = items.get(i) {
        match item {
            Item::User { text } => user_block(app, ui, text),
            Item::Assistant { text } => assistant_block(app, ui, text),
            Item::Tool { .. } => {
                let key = item.tool_key().unwrap_or_default();
                let mut end = i + 1;
                while items.get(end).is_some_and(|n| matches!(n, Item::Tool { .. }) && n.tool_key().as_deref() == Some(key.as_str())) {
                    end += 1;
                }
                let group = items.get(i..end).unwrap_or_default();
                tool_group(app, ui, i, &key, group);
                i = end;
                continue;
            }
            other => status_item(app, ui, other),
        }
        i += 1;
    }
}

fn user_block(app: &mut FilmcraftApp, ui: &mut egui::Ui, text: &str) {
    let t = app.tokens;
    ui.add_space(6.0);
    strong_note(ui, s("You"), META, t.text_dim);
    ui.add(egui::Label::new(egui::RichText::new(text).color(t.text)).selectable(true).wrap());
}

fn assistant_block(app: &mut FilmcraftApp, ui: &mut egui::Ui, text: &str) {
    let t = app.tokens;
    ui.add_space(6.0);
    strong_note(ui, s("Agent"), META, t.text_dim);
    super::super::markdown::show(ui, text, t.text, t.text_dim);
}

/// A gate answer, an error, a notice, an answered permission or the agent's own auth message.
fn status_item(app: &mut FilmcraftApp, ui: &mut egui::Ui, item: &Item) {
    let t = app.tokens;
    match item {
        Item::Gate { stage, state, note } => {
            let what = if state == bashcut_agent::plan::APPROVED { s("approved") } else { s("changes requested") };
            let mut line = f("Gate {stage}: {state}", &[("stage", stage), ("state", what)]);
            if !note.is_empty() {
                line.push_str(&format!(" — {note}"));
            }
            super::super::widgets::note(ui, line, NOTE, t.accent);
        }
        Item::Error { message } => {
            note(ui, super::super::strings::agent(message), NOTE, t.danger);
        }
        Item::Notice { message } => {
            note(ui, super::super::strings::agent(message), META, t.text_dim);
        }
        Item::Permission { tool, detail, answer } => {
            let color = if answer == "denied" { t.danger } else { t.text_dim };
            note(ui, permission_line(tool, answer), META, color);
            code_note(ui, bashcut_agent::clip(detail, 300), CODE, t.text_dim);
        }
        Item::Auth { category, message } => {
            // the readiness line says what to do; here the agent's own words, selectable
            let text = if message.is_empty() { category } else { message };
            ui.add(egui::Label::new(egui::RichText::new(text).size(META).color(t.text_dim)).selectable(true).wrap());
        }
        Item::User { .. } | Item::Assistant { .. } | Item::Tool { .. } => {}
    }
}
