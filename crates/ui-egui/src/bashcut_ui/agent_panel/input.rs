//! The message box and the `/` list (AG12).

use bashcut_agent::stream_json::SlashCommand;
use serde_json::json;

use super::super::strings::s;
use super::super::widgets::{META, NOTE, button, note};
use super::{View, call, input_id};
use crate::FilmcraftApp;

/// Rows the `/` list shows at most (AG12).
const SLASH_ROWS: usize = 8;

/// The `/` word the caret ends (AG12): its byte start (at the `/`) and what follows the `/`. A word
/// is one at the start or after whitespace, with nothing but whitespace after the caret; a path
/// (`/a/b`) is not one.
pub(super) fn slash_word(text: &str, caret: usize) -> Option<(usize, &str)> {
    let before = text.get(..caret)?;
    let start = before.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8());
    let q = before.get(start..)?.strip_prefix('/')?;
    let continues = text.get(caret..).and_then(|r| r.chars().next()).is_some_and(|c| !c.is_whitespace());
    (!continues && !q.contains('/')).then_some((start, q))
}

/// The commands matching `q` (AG12), at most [`SLASH_ROWS`]: a name (or its part after the plugin
/// namespace, `setup` of `bcp:setup`) starting with `q` first, then names containing it; skills
/// before other commands within each, else in the agent's order. Case is ignored.
pub(super) fn slash_matches<'a>(commands: &'a [SlashCommand], q: &str) -> Vec<&'a SlashCommand> {
    let q = q.to_lowercase();
    let mut hits: Vec<(u8, &SlashCommand)> = commands
        .iter()
        .filter_map(|c| {
            let name = c.name.to_lowercase();
            let short = name.rsplit(':').next().unwrap_or_default();
            let rank = if name.starts_with(&q) || short.starts_with(&q) {
                0
            } else if name.contains(&q) {
                2
            } else {
                return None;
            };
            Some((rank + u8::from(!c.skill), c))
        })
        .collect();
    hits.sort_by_key(|(rank, _)| *rank);
    hits.into_iter().map(|(_, c)| c).take(SLASH_ROWS).collect()
}

/// The caret's byte offset in the message box (its end when the box has no caret yet).
pub(super) fn caret(ctx: &egui::Context, text: &str) -> usize {
    let chars = egui::text_edit::TextEditState::load(ctx, input_id()).and_then(|st| st.cursor.char_range()).map(|r| usize::from(r.primary.index));
    chars.map_or(text.len(), |n| text.char_indices().nth(n).map_or(text.len(), |(i, _)| i))
}

/// Replace the `/` word `start..caret` with `/name ` and put the caret after it (AG12).
pub(super) fn insert_slash(ctx: &egui::Context, v: &mut View, start: usize, caret: usize, name: &str) {
    if start > caret || !v.input.is_char_boundary(start) || !v.input.is_char_boundary(caret) {
        return;
    }
    let spaced = v.input.get(caret..).and_then(|r| r.chars().next()).is_some_and(char::is_whitespace);
    let word = if spaced { format!("/{name}") } else { format!("/{name} ") };
    v.input.replace_range(start..caret, &word);
    let at = v.input.get(..start.saturating_add(word.len())).map_or(0, |t| t.chars().count()) + usize::from(spaced);
    let mut st = egui::text_edit::TextEditState::load(ctx, input_id()).unwrap_or_default();
    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
    st.store(ctx, input_id());
    ctx.memory_mut(|m| m.request_focus(input_id()));
    v.slash_rect = None;
}

/// The message box; `busy`: Send is off (a turn of a per-message agent, or a stopping one), `running`:
/// Stop is on; `slash`: the agent's slash commands (AG12; `None`: it has none, `Some(None)`: not
/// known before its first init).
#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    app: &mut FilmcraftApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    v: &mut View,
    busy: bool,
    running: bool,
    missing: Option<&str>,
    slash: Option<Option<&[SlashCommand]>>,
) {
    let focused = ctx.memory(|m| m.has_focus(input_id()));
    // the `/` list: open while the caret ends a `/` word, the box (or the list under the pointer) has
    // focus, and Esc did not close it for this word
    let on_list = v.slash_rect.is_some_and(|r| ctx.pointer_latest_pos().is_some_and(|p| r.contains(p)));
    let at = caret(ctx, &v.input);
    let word = slash.filter(|_| focused || on_list).and_then(|cmds| slash_word(&v.input, at).map(|(start, q)| (start, q.to_string(), cmds)));
    if word.as_ref().is_none_or(|(_, q, _)| v.slash_closed.as_deref() != Some(q.as_str())) {
        v.slash_closed = None;
    }
    let word = word.filter(|_| v.slash_closed.is_none());
    if let Some((_, q, _)) = &word
        && *q != v.slash_word
    {
        v.slash_word = q.clone();
        v.slash_sel = 0;
    }
    let hits: Vec<SlashCommand> = word.as_ref().and_then(|(_, q, c)| c.map(|c| slash_matches(c, q).into_iter().cloned().collect())).unwrap_or_default();
    v.slash_sel = v.slash_sel.min(hits.len().saturating_sub(1));
    let mut inserted = false;
    if let Some((start, q, _)) = &word
        && focused
    {
        let key = |i: &mut egui::InputState, k: egui::Key| i.consume_key(egui::Modifiers::NONE, k);
        if ui.input_mut(|i| key(i, egui::Key::Escape)) {
            v.slash_closed = Some(q.clone());
        } else if !hits.is_empty() {
            let (up, down, pick) =
                ui.input_mut(|i| (key(i, egui::Key::ArrowUp), key(i, egui::Key::ArrowDown), key(i, egui::Key::Tab) || key(i, egui::Key::Enter)));
            if up {
                v.slash_sel = v.slash_sel.checked_sub(1).unwrap_or(hits.len() - 1);
            }
            if down {
                v.slash_sel = (v.slash_sel + 1) % hits.len();
            }
            if pick && let Some(c) = hits.get(v.slash_sel) {
                insert_slash(ctx, v, *start, at, &c.name);
                inserted = true;
            }
        }
    }
    let open = word.filter(|(_, _, cmds)| !inserted && v.slash_closed.is_none() && (cmds.is_none() || !hits.is_empty()));
    // Enter sends, Shift+Enter is a new line: take the Enter press away before the text box sees it
    let enter = focused
        && ui.input_mut(|i| {
            let hit = !i.modifiers.shift && i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, .. }));
            if hit {
                i.events.retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::Enter, .. }));
            }
            hit
        });
    let r = ui.add(
        egui::TextEdit::multiline(&mut v.input)
            .id(input_id())
            .hint_text(s("Message the agent (Enter sends, Shift+Enter for a new line)"))
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            // while the `/` list is open, Tab and Esc are its keys, not focus moves (egui applies
            // the filter to the next frame's keys)
            .event_filter(egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, tab: open.is_some(), escape: open.is_some() }),
    );
    app.auto.add_response("bashcut.agent.input", &r, "message");
    v.slash_rect = match &open {
        Some((start, _, cmds)) => slash_list(app, ctx, v, r.rect, *start, at, cmds.is_none(), &hits),
        None => None,
    };
    ui.horizontal(|ui| {
        let can_send = !busy && !v.input.trim().is_empty() && missing.is_none();
        let mut r = ui.add_enabled(can_send, egui::Button::new(s("Send")));
        if let Some(why) = missing {
            r = r.on_disabled_hover_text(why);
        }
        app.auto.add_response("bashcut.agent.send", &r, s("Send"));
        let clicked = r.clicked();
        let text = v.input.trim().to_string();
        if (clicked || (enter && can_send)) && call(app, ctx, v, "send", json!({"text": text})) {
            v.input.clear();
        }
        if button(app, ui, "bashcut.agent.stop", s("Stop"), running) {
            call(app, ctx, v, "stop", json!({}));
        }
    });
}

/// The `/` list above the message box at `rect` (AG12): `hits` as rows, or the line saying a message
/// loads them (`loading`). Its rect.
#[allow(clippy::too_many_arguments)]
pub(super) fn slash_list(
    app: &mut FilmcraftApp,
    ctx: &egui::Context,
    v: &mut View,
    rect: egui::Rect,
    start: usize,
    at: usize,
    loading: bool,
    hits: &[SlashCommand],
) -> Option<egui::Rect> {
    let t = app.tokens;
    let mut picked = None;
    let area = egui::Area::new(egui::Id::new("bashcut.agent.slash")).order(egui::Order::Foreground).fixed_pos(rect.left_top()).pivot(egui::Align2::LEFT_BOTTOM);
    let out = area.show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width((rect.width() - 12.0).max(120.0));
            if loading {
                let line = s("Start a message to load skills");
                let r = note(ui, line, NOTE, t.text_dim);
                app.auto.add_response("bashcut.agent.slash.hint", &r, line);
                return;
            }
            for (i, c) in hits.iter().enumerate() {
                let kind = if c.skill { s("skill") } else { s("command") };
                let label = format!("/{}", c.name);
                let r = ui.add(
                    egui::Button::selectable(i == v.slash_sel, label.as_str())
                        .right_text(egui::RichText::new(kind).color(t.text_dim))
                        .min_size(egui::vec2(ui.available_width(), 0.0)),
                );
                app.auto.add_response(&format!("bashcut.agent.slash.{i}"), &r, &label);
                if r.clicked() {
                    picked = Some(c.name.clone());
                }
            }
            note(ui, s("↑/↓ to pick, Enter or Tab to insert, Esc to close"), META, t.text_dim);
        });
    });
    if let Some(name) = picked {
        insert_slash(ctx, v, start, at, &name);
    }
    Some(out.response.rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_word_and_matches() {
        assert_eq!(slash_word("/se", 3), Some((0, "se")));
        assert_eq!(slash_word("/", 1), Some((0, "")));
        assert_eq!(slash_word("cut it /bc", 10), Some((7, "bc")));
        assert_eq!(slash_word("a\n/x", 4), Some((2, "x")));
        // not mid-word, not a path, not at a caret before more text
        assert_eq!(slash_word("a/b", 3), None);
        assert_eq!(slash_word("/a/b", 4), None);
        assert_eq!(slash_word("/sex", 3), None, "the caret is inside the word");
        assert_eq!(slash_word("/se rest", 3), Some((0, "se")), "a space after the caret ends the word");
        assert_eq!(slash_word("/se", 99), None);
        assert_eq!(slash_word("plain", 5), None);
        // multi-byte text before the word
        assert_eq!(slash_word("cắt /se", "cắt /se".len()), Some(("cắt ".len(), "se")));

        let c = |name: &str, skill: bool| SlashCommand { name: name.to_string(), skill };
        let cmds = vec![c("compact", false), c("resume-setup", true), c("setup", false), c("bcp:setup", true), c("bcp:review", true), c("Setup-all", false)];
        let names = |q: &str| slash_matches(&cmds, q).into_iter().map(|c| c.name.as_str()).collect::<Vec<_>>();
        // prefix (incl. after the namespace) before substring; skills before commands; case ignored
        assert_eq!(names("se"), ["bcp:setup", "setup", "Setup-all", "resume-setup"]);
        assert_eq!(names("SETUP"), ["bcp:setup", "setup", "Setup-all", "resume-setup"]);
        assert_eq!(names("bcp"), ["bcp:setup", "bcp:review"]);
        assert_eq!(names("zzz"), Vec::<&str>::new());
        assert_eq!(names("")[0], "resume-setup", "empty query: skills first, in the agent's order");
        // capped
        let many: Vec<SlashCommand> = (0..SLASH_ROWS + 5).map(|i| c(&format!("cmd{i}"), false)).collect();
        assert_eq!(slash_matches(&many, "cmd").len(), SLASH_ROWS);
    }
}
