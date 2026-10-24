//! The transcript: one fn per kind of item; consecutive tool calls with the same key fold into one row.

use std::collections::BTreeMap;

use bashcut_agent::Item;

use super::super::markdown::copy_button;
use super::super::strings::{f, s};
use super::super::widgets::{CODE, META, NOTE, code_note, note_text, strong_note};
use super::super::{motion, shared};
use super::cards::{failed_reason, permission_line, problem_card, result_error};
use super::tools::tool_group;
use crate::FilmcraftApp;

/// Widest share of the panel a user bubble takes.
const BUBBLE_SHARE: f32 = 0.85;
/// Items that arrive in one frame beyond this many are a conversation being opened, not a live turn.
const RELOAD_JUMP: usize = 8;
/// A gap between frames longer than this (egui seconds) means the panel was closed meanwhile.
const CLOSED_GAP: f64 = 1.0;

/// Which items fade in: those appended while the panel stays open (egui context data).
#[derive(Clone, Default)]
struct Flow {
    first: u64,
    len: usize,
    /// Items from this index on are new; earlier ones (a saved conversation) never animate.
    base: usize,
    at: f64,
    /// Item index → its fade finished.
    fades: BTreeMap<usize, bool>,
}

impl Flow {
    /// The opacity of item `i` this frame.
    fn alpha(&mut self, ctx: &egui::Context, i: usize) -> f32 {
        if i < self.base || !motion::on(ctx) || self.fades.get(&i) == Some(&true) {
            return 1.0;
        }
        let id = egui::Id::new(("bashcut.agent.appear", i));
        let a = if self.fades.contains_key(&i) { ctx.animate_bool_with_time(id, true, 0.18) } else { motion::appear(ctx, id) };
        self.fades.insert(i, a >= 1.0);
        if a < 1.0 {
            ctx.request_repaint();
        }
        a
    }
}

fn hash_of(items: &[Item]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match items.first() {
        Some(Item::User { text } | Item::Assistant { text }) => text.hash(&mut h),
        Some(_) | None => 0u8.hash(&mut h),
    }
    h.finish()
}

pub(super) fn transcript(app: &mut FilmcraftApp, ui: &mut egui::Ui, items: &[Item], agent_name: &str) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let flow_id = egui::Id::new("bashcut.agent.flow");
    let mut flow = shared::temp::<Flow>(&ctx, flow_id);
    let first = hash_of(items);
    let reload = flow.at == 0.0 || now - flow.at > CLOSED_GAP || first != flow.first || items.len() < flow.len || items.len() > flow.len + RELOAD_JUMP;
    if reload {
        flow = Flow { first, base: items.len(), ..Flow::default() };
    }
    flow.first = first;
    flow.len = items.len();
    flow.at = now;

    let mut i = 0;
    while let Some(item) = items.get(i) {
        // the name row opens a run of assistant and tool items, not every block
        let run_start = i.checked_sub(1).and_then(|p| items.get(p)).is_none_or(|p| !matches!(p, Item::Assistant { .. } | Item::Tool { .. }));
        let a = flow.alpha(&ctx, i);
        if a < 1.0 {
            ui.add_space((1.0 - a) * 6.0);
        }
        let mut next = i + 1;
        ui.scope(|ui| {
            ui.multiply_opacity(a);
            match item {
                Item::User { text } => user_block(app, ui, i, text),
                Item::Assistant { text } => assistant_block(app, ui, i, text, agent_name, run_start),
                Item::Tool { .. } => {
                    let key = item.tool_key().unwrap_or_default();
                    let mut end = i + 1;
                    while items.get(end).is_some_and(|n| matches!(n, Item::Tool { .. }) && n.tool_key().as_deref() == Some(key.as_str())) {
                        end += 1;
                    }
                    let group = items.get(i..end).unwrap_or_default();
                    tool_group(app, ui, i, &key, group);
                    next = end;
                }
                other => status_item(app, ui, items.get(..i).unwrap_or_default(), i, other),
            }
        });
        i = next;
    }
    shared::put_temp(&ctx, flow_id, flow);
}

/// 10 px and a hairline between two turns.
fn turn_separator(app: &FilmcraftApp, ui: &mut egui::Ui) {
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, app.tokens.separator));
}

fn user_block(app: &mut FilmcraftApp, ui: &mut egui::Ui, i: usize, text: &str) {
    let t = app.tokens;
    if i > 0 {
        turn_separator(app, ui);
    }
    ui.add_space(6.0);
    let width = ui.available_width();
    let max = (width * BUBBLE_SHARE).max(60.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        let bubble = ui.scope(|ui| {
            ui.set_max_width(max);
            egui::Frame::new().fill(t.field_bg).corner_radius(t.radius).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                ui.set_max_width((max - 16.0).max(40.0));
                ui.add(egui::Label::new(egui::RichText::new(text).color(t.text)).selectable(true).wrap());
            });
        });
        // the Copy button sits at the bubble's top-left, outside it
        let hot = ui.rect_contains_pointer(bubble.response.rect.expand2(egui::vec2(24.0, 0.0)));
        let mix = motion::hover_mix(ui.ctx(), egui::Id::new(("bashcut.agent.copy.hover", i)), hot);
        copy_button(app, ui, &format!("bashcut.agent.copy.{i}"), text, mix);
    });
}

fn assistant_block(app: &mut FilmcraftApp, ui: &mut egui::Ui, i: usize, text: &str, name: &str, with_name: bool) {
    let t = app.tokens;
    ui.add_space(6.0);
    if with_name {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            crate::icons::paint(ui.painter(), rect, crate::icons::Icon::Sparkle, t.accent);
            strong_note(ui, name, META, t.text_dim);
        });
    }
    let block = ui.scope(|ui| super::super::markdown::show(app, ui, &format!("bashcut.agent.copy.{i}"), text, t.text, t.text_dim));
    let rect = block.response.rect;
    let hot = ui.rect_contains_pointer(rect);
    let mix = motion::hover_mix(ui.ctx(), egui::Id::new(("bashcut.agent.copy.hover", i)), hot);
    // top-right of the block; the whole reply's raw Markdown
    let slot = egui::Rect::from_min_size(egui::pos2(rect.right() - 20.0, rect.top()), egui::vec2(20.0, 18.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(slot));
    copy_button(app, &mut child, &format!("bashcut.agent.copy.{i}"), text, mix);
}

/// One answered line with a small check or cross in front.
fn answered_line(ui: &mut egui::Ui, text: String, ok: bool, color: egui::Color32) {
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
        let icon = if ok { crate::icons::Icon::Marker } else { crate::icons::Icon::Close };
        crate::icons::paint(ui.painter(), egui::Rect::from_center_size(rect.center(), egui::vec2(9.0, 9.0)), icon, color);
        ui.add(egui::Label::new(super::super::widgets::note_text(text, NOTE, color)).selectable(true).wrap());
    });
}

/// A gate answer, an error, a notice, an answered permission or the agent's own auth message.
fn status_item(app: &mut FilmcraftApp, ui: &mut egui::Ui, before: &[Item], i: usize, item: &Item) {
    let t = app.tokens;
    match item {
        Item::Gate { stage, state, note } => {
            let what = if state == bashcut_agent::plan::APPROVED { s("approved") } else { s("changes requested") };
            let mut line = f("Gate {stage}: {state}", &[("stage", stage), ("state", what)]);
            if !note.is_empty() {
                line.push_str(&format!(" — {note}"));
            }
            let ok = state == bashcut_agent::plan::APPROVED;
            answered_line(ui, line, ok, t.accent);
        }
        Item::Error { message } => {
            ui.add_space(4.0);
            problem_card(app, ui, &format!("bashcut.agent.error.{i}"), &result_error(message), failed_reason(before).as_deref());
        }
        Item::Notice { message } => {
            // centered, no bullet
            let text = note_text(super::super::strings::agent(message), META, t.text_dim);
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(text).halign(egui::Align::Center).selectable(true).wrap());
            });
        }
        Item::Permission { tool, detail, answer } => {
            let color = if answer == "denied" { t.danger } else { t.text_dim };
            let ok = matches!(answer.as_str(), "allowed" | "always");
            answered_line(ui, permission_line(tool, answer), ok, color);
            code_note(ui, bashcut_agent::clip(detail, 300), CODE, t.text_dim);
        }
        Item::Auth { category, message } => {
            // the readiness line carries Sign in; here the agent's own words in a problem card
            let text = if message.is_empty() { category } else { message };
            ui.add_space(4.0);
            problem_card(app, ui, &format!("bashcut.agent.autherror.{i}"), text, None);
        }
        Item::TurnEnd { turns, cost_usd, ms, .. } => turn_end(app, ui, i, *turns, *cost_usd, *ms),
        Item::User { .. } | Item::Assistant { .. } | Item::Tool { .. } => {}
    }
}

/// AGU10: the faint centred line after a finished turn, `9 steps · 41 s · $0.12`; nothing when
/// the agent reported none of it.
fn turn_end(app: &mut FilmcraftApp, ui: &mut egui::Ui, i: usize, turns: Option<u64>, cost_usd: Option<f64>, ms: Option<u64>) {
    let mut parts = Vec::new();
    if let Some(n) = turns.filter(|n| *n > 0) {
        parts.push(if n == 1 { s("1 step").to_string() } else { f("{n} steps", &[("n", &n.to_string())]) });
    }
    if let Some(secs) = ms.map(|ms| ms / 1000).filter(|s| *s > 0) {
        parts.push(if secs < 60 {
            f("{n} s", &[("n", &secs.to_string())])
        } else {
            f("{m} min {s} s", &[("m", &(secs / 60).to_string()), ("s", &(secs % 60).to_string())])
        });
    }
    if let Some(c) = cost_usd.filter(|c| c.is_finite() && *c > 0.0) {
        parts.push(format!("${c:.2}"));
    }
    if parts.is_empty() {
        return;
    }
    ui.add_space(4.0);
    let line = parts.join(" · ");
    let text = note_text(line.clone(), META, app.tokens.text_dim);
    let r = ui
        .with_layout(egui::Layout::top_down(egui::Align::Center), |ui| ui.add(egui::Label::new(text).halign(egui::Align::Center).selectable(true).wrap()))
        .inner;
    app.auto.add_response(&format!("bashcut.agent.turn.{i}"), &r, &line);
}
