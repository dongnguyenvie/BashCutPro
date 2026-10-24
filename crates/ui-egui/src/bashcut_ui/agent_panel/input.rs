//! The message box and the `/` list (AG12).

use bashcut_agent::stream_json::SlashCommand;
use serde_json::json;

use super::super::strings::{f, s};
use super::super::widgets::{META, NOTE, note, note_text};
use super::{View, call, input_id};
use crate::FilmcraftApp;

/// Rows the `/` list shows at most (AG12).
const SLASH_ROWS: usize = 8;
/// Rows the message box grows to before it scrolls.
const COMPOSER_ROWS: f32 = 8.0;
/// Diameter of the Send / Stop button (px).
const BTN: f32 = 24.0;

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
    agent: &str,
    context: &str,
) {
    if super::super::agent::terminal_live(ctx) {
        locked(app, ui, ctx, v);
        return;
    }
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
    let tk = app.tokens;
    let stroke = egui::Stroke::new(1.0, if focused { tk.accent } else { tk.field_border });
    let mut box_rect = ui.max_rect();
    let frame = egui::Frame::new().fill(tk.field_bg).stroke(stroke).corner_radius(tk.radius).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        let row_h = ui.text_style_height(&egui::TextStyle::Body);
        let hint = f("Message {agent}…", &[("agent", agent)]);
        // grows with its content from 1 row, then scrolls inside the frame
        let r = egui::ScrollArea::vertical()
            .id_salt("bashcut.agent.input.scroll")
            .max_height(row_h * COMPOSER_ROWS)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut v.input)
                        .id(input_id())
                        .hint_text(hint)
                        .frame(egui::Frame::NONE)
                        .desired_rows(1)
                        .desired_width(f32::INFINITY)
                        // while the `/` list is open, Tab and Esc are its keys, not focus moves (egui
                        // applies the filter to the next frame's keys)
                        .event_filter(egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, tab: open.is_some(), escape: open.is_some() }),
                )
            })
            .inner;
        app.auto.add_response("bashcut.agent.input", &r, "message");
        box_rect = r.rect;
        bottom_row(app, ui, ctx, v, enter, busy, running, missing, context);
    });
    v.composer_h = frame.response.rect.height();
    v.slash_rect = match &open {
        Some((start, _, cmds)) => slash_list(app, ctx, v, box_rect, *start, at, cmds.is_none(), &hits),
        None => None,
    };
}

/// The box while the terminal holds the session (AGT3): off, with the reason and Take back (confirmed first).
fn locked(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View) {
    let tk = app.tokens;
    let key = egui::Id::new("bashcut.agent.takeBack.confirm");
    let asking = ctx.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let reason = s("Claude Code is running in the terminal. Close it to continue here.");
    let frame =
        egui::Frame::new().fill(tk.field_bg).stroke(egui::Stroke::new(1.0, tk.field_border)).corner_radius(tk.radius).inner_margin(egui::Margin::same(6)).show(
            ui,
            |ui| {
                ui.set_width(ui.available_width());
                let r = note(ui, reason, NOTE, tk.text_dim);
                app.auto.add_response("bashcut.agent.input", &r, reason);
                ui.horizontal(|ui| {
                    if asking {
                        note(ui, s("Take back the conversation? Close the terminal window first, or two writers share one session."), META, tk.text_dim);
                    }
                });
                ui.horizontal(|ui| {
                    let (label, id) = if asking { (s("Yes, take back"), "bashcut.agent.takeBack.confirm") } else { (s("Take back"), "bashcut.agent.takeBack") };
                    let r = ui.add(egui::Button::new(label).small());
                    app.auto.add_response(id, &r, label);
                    if r.clicked() {
                        if asking {
                            ctx.data_mut(|d| d.remove::<bool>(key));
                            super::super::agent::take_back(app, ctx);
                        } else {
                            ctx.data_mut(|d| d.insert_temp(key, true));
                        }
                    }
                    if asking {
                        let r = ui.add(egui::Button::new(s("Cancel")).small());
                        app.auto.add_response("bashcut.agent.takeBack.cancel", &r, s("Cancel"));
                        if r.clicked() {
                            ctx.data_mut(|d| d.remove::<bool>(key));
                        }
                    }
                });
            },
        );
    v.composer_h = frame.response.rect.height();
    v.slash_rect = None;
}

/// The row under the text: the context chip on the left, Send (or Stop) on the right.
#[allow(clippy::too_many_arguments)]
fn bottom_row(
    app: &mut FilmcraftApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    v: &mut View,
    enter: bool,
    busy: bool,
    running: bool,
    missing: Option<&str>,
    context: &str,
) {
    let tk = app.tokens;
    let empty = v.input.trim().is_empty();
    let can_send = !busy && !empty && missing.is_none();
    // a running turn with nothing typed: Stop takes Send's place; typed text (live agent) shows both
    let show_send = !(running && empty);
    let mut send = false;
    let mut stop = false;
    ui.horizontal(|ui| {
        let chip = chip_text(app);
        let w = (ui.available_width() - 2.0 * (BTN + ui.spacing().item_spacing.x)).max(20.0);
        ui.allocate_ui_with_layout(egui::vec2(w, BTN), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let r = ui.add(egui::Label::new(note_text(chip.as_str(), META, tk.text_dim)).truncate().sense(egui::Sense::hover()));
            r.on_hover_text(context);
            app.auto.add("bashcut.agent.context", ui.min_rect(), &chip);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let none = egui::Rect::from_min_size(ui.cursor().right_top(), egui::Vec2::ZERO);
            if show_send {
                let tip = missing.unwrap_or_else(|| s("Send (Enter sends, Shift+Enter for a new line)"));
                send = round_button(app, ui, "bashcut.agent.send", crate::icons::Icon::ArrowUp, tip, can_send, true);
            } else {
                app.auto.add("bashcut.agent.send", none, s("Send"));
            }
            if running {
                stop = round_button(app, ui, "bashcut.agent.stop", crate::icons::Icon::Square, s("Stop"), true, false);
            } else {
                app.auto.add("bashcut.agent.stop", none, s("Stop"));
            }
        });
    });
    let text = v.input.trim().to_string();
    if (send || (enter && can_send)) && call(app, ctx, v, "send", json!({"text": text})) {
        v.input.clear();
    }
    if stop {
        call(app, ctx, v, "stop", json!({}));
    }
}

/// What the app sends with a message, short: the sequence, the playhead, the selection.
fn chip_text(app: &FilmcraftApp) -> String {
    let st = &app.session;
    let Some(id) = st.state.active_sequence else {
        return s("No sequence open").to_string();
    };
    let name = st.project.item(id).map(|i| i.name.clone()).unwrap_or_default();
    let secs = st.playhead().seconds().max(0.0);
    let mut parts = vec![bashcut_agent::clip(&name, 40), format!("{}:{:04.1}", (secs / 60.0) as u64, secs % 60.0)];
    match st.state.selection.len() {
        0 => {}
        1 => parts.push(s("1 clip selected").to_string()),
        n => parts.push(f("{n} clips selected", &[("n", &n.to_string())])),
    }
    parts.join(" · ")
}

/// A round icon button: accent fill for Send (`accent`), light for Stop, grey when off.
fn round_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, icon: crate::icons::Icon, tip: &str, enabled: bool, accent: bool) -> bool {
    let tk = app.tokens;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(BTN, BTN), egui::Sense::click());
    let r = r.on_hover_text(tip);
    let (fill, fg) = match (enabled, accent) {
        (false, _) => (tk.field_border, tk.text_faint),
        (true, true) => (if r.hovered() { tk.accent.gamma_multiply(0.85) } else { tk.accent }, egui::Color32::WHITE),
        (true, false) => (if r.hovered() { tk.text } else { tk.text_dim }, tk.panel_bg),
    };
    ui.painter().circle_filled(rect.center(), BTN / 2.0, fill);
    crate::icons::paint(ui.painter(), egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 10.0)), icon, fg);
    app.auto.add_response(id, &r, tip);
    enabled && r.clicked()
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
