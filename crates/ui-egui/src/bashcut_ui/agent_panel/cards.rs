//! The cards of the Agent transcript (AG8, AGU6): one `card()` frame in three tones, with a 3 px left bar.
//! Needs you (accent): permission request, pending gate. Problem (danger): errors, auth. Info (text_dim): notes.

use bashcut_agent::Gate;
use serde_json::json;

use super::super::markdown::copy_button;
use super::super::strings::{f, s};
use super::super::widgets::{CODE, NOTE, code_text, note};
use super::{View, call};
use crate::FilmcraftApp;

/// What a card asks of the user; picks the bar colour.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
    Needs,
    Problem,
    /// Setup notes (kit missing, kit gap, project not saved, control off, auth): the readiness lines in setup.rs.
    Info,
}

/// A card: soft fill (bar colour at about 10 % over the panel), 3 px bar on the left, rounded corners.
pub(super) fn card<R>(
    app: &mut FilmcraftApp,
    ui: &mut egui::Ui,
    tone: Tone,
    add: impl FnOnce(&mut FilmcraftApp, &mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let t = app.tokens;
    let bar = match tone {
        Tone::Needs => t.accent,
        Tone::Problem => t.danger,
        Tone::Info => t.text_dim,
    };
    let fill = t.panel_bg.blend(bar.gamma_multiply(0.10));
    let out = egui::Frame::new().fill(fill).corner_radius(t.radius).inner_margin(egui::Margin { left: 11, right: 8, top: 7, bottom: 7 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(app, ui)
    });
    let r = out.response.rect;
    let bar_rect = egui::Rect::from_min_max(r.min, egui::pos2(r.left() + 3.0, r.bottom()));
    ui.painter().rect_filled(bar_rect, egui::CornerRadius { nw: t.radius as u8, sw: t.radius as u8, ne: 0, se: 0 }, bar);
    egui::InnerResponse::new(out.inner, out.response)
}

/// How prominent an action in a card is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Weight {
    /// The one filled button.
    Primary,
    Secondary,
    /// A plain button with danger text.
    Danger,
}

/// A card action with a tooltip; `true` when clicked.
pub(super) fn action(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, hover: &str, weight: Weight, enabled: bool) -> bool {
    let t = app.tokens;
    let button = match weight {
        Weight::Primary => egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE)).fill(t.accent),
        Weight::Secondary => egui::Button::new(label),
        Weight::Danger => egui::Button::new(egui::RichText::new(label).color(t.danger)),
    };
    let r = ui.add_enabled(enabled, button.small());
    let r = if hover.is_empty() { r } else { r.on_hover_text(hover) };
    app.auto.add_response(id, &r, label);
    r.clicked()
}

/// A card title: icon and strong text, wrapping.
fn title_row(app: &FilmcraftApp, ui: &mut egui::Ui, icon: crate::icons::Icon, color: egui::Color32, text: String) {
    let t = app.tokens;
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), egui::Sense::hover());
        crate::icons::paint(ui.painter(), egui::Rect::from_center_size(rect.center(), egui::vec2(11.0, 11.0)), icon, color);
        ui.add(egui::Label::new(egui::RichText::new(text).strong().color(t.text)).wrap());
    });
}

/// Mono text in a code frame, selectable, with Copy at the top right.
fn detail_frame(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, text: &str) {
    let t = app.tokens;
    let shown = bashcut_agent::clip(text, 2000);
    egui::Frame::new().fill(t.field_bg).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let r = ui.add(egui::Label::new(code_text(shown, CODE, t.text)).selectable(true).wrap());
        app.auto.add_response(&format!("{id}.detail"), &r, text);
    });
}

/// A sentence for a result that ended a turn badly (`error_during_execution` and the like); anything else
/// stays as the agent wrote it, in our language when it is one of the crate's own messages.
pub(super) fn result_error(message: &str) -> String {
    match message {
        "error_during_execution" => s("The turn ended with an error.").to_string(),
        "error_max_turns" => s("The turn stopped at its turn limit.").to_string(),
        "error_max_budget_usd" => s("The turn stopped at its spending limit.").to_string(),
        other => super::super::strings::agent(other),
    }
}

/// The failed turn's first reason: the first line of the last failed tool output before this error.
pub(super) fn failed_reason(before: &[bashcut_agent::Item]) -> Option<String> {
    before.iter().rev().take_while(|i| !matches!(i, bashcut_agent::Item::User { .. })).find_map(|i| match i {
        bashcut_agent::Item::Tool { is_error: true, output: Some(o), .. } => {
            o.lines().find(|l| !l.trim().is_empty()).map(|l| bashcut_agent::clip(l.trim(), 300))
        }
        _ => None,
    })
}

/// A problem card: the sentence, an optional reason, Copy.
pub(super) fn problem_card(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, headline: &str, reason: Option<&str>) {
    let t = app.tokens;
    let full = match reason {
        Some(r) => format!("{headline}\n{r}"),
        None => headline.to_string(),
    };
    let frame = card(app, ui, Tone::Problem, |app, ui| {
        title_row(app, ui, crate::icons::Icon::Info, t.danger, headline.to_string());
        if let Some(reason) = reason {
            let r = ui.add(egui::Label::new(egui::RichText::new(reason).size(NOTE).color(t.text_dim)).selectable(true).wrap());
            app.auto.add_response(&format!("{id}.reason"), &r, reason);
        }
        copy_button(app, ui, &format!("{id}.copy"), &full, 1.0);
    });
    app.auto.add(id, frame.response.rect, headline);
}

pub(super) fn gate_card(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, g: &Gate, running: bool) {
    let t = app.tokens;
    let frame = card(app, ui, Tone::Needs, |app, ui| {
        title_row(app, ui, crate::icons::Icon::Info, t.accent, f("Gate: {stage}", &[("stage", &g.stage)]));
        if !g.summary.is_empty() {
            ui.label(egui::RichText::new(&g.summary).size(11.5).color(t.text));
        }
        for line in g.evidence_lines() {
            note(ui, format!("• {line}"), NOTE, t.text_dim);
        }
        let r = ui.add(egui::TextEdit::multiline(&mut v.note).hint_text(s("Note (needed to ask for changes)")).desired_rows(2).desired_width(f32::INFINITY));
        app.auto.add_response("bashcut.gate.note", &r, "note");
        ui.horizontal_wrapped(|ui| {
            let note = v.note.trim().to_string();
            if action(app, ui, "bashcut.gate.approve", s("Approve"), "", Weight::Primary, !running) && call(app, ctx, v, "approve", json!({"note": note})) {
                v.note.clear();
                v.gate_checked = None;
            }
            if action(app, ui, "bashcut.gate.change", s("Ask for changes"), "", Weight::Secondary, !running && !note.is_empty())
                && call(app, ctx, v, "change", json!({"note": note}))
            {
                v.note.clear();
                v.gate_checked = None;
            }
        });
    });
    app.auto.add("bashcut.gate", frame.response.rect, &g.stage);
}

/// A permission request waiting for the user (AG8): what the agent wants to run, and the answers.
pub(super) fn permission_card(
    app: &mut FilmcraftApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    v: &mut View,
    i: usize,
    p: &bashcut_agent::session::PermissionView,
    agent_name: &str,
) {
    let t = app.tokens;
    let id = format!("bashcut.agent.permission.{i}");
    ui.add_space(6.0);
    let frame = card(app, ui, Tone::Needs, |app, ui| {
        let title = f("{agent} wants to use {tool}", &[("agent", agent_name), ("tool", &p.tool)]);
        title_row(app, ui, crate::icons::Icon::Info, t.accent, title);
        detail_frame(app, ui, &id, &p.detail);
        copy_button(app, ui, &format!("{id}.copy"), &p.detail, 1.0);
        ui.horizontal_wrapped(|ui| {
            let mut answer = None;
            if action(app, ui, &format!("{id}.once"), s("Allow once"), s("Run this call; ask again next time."), Weight::Primary, true) {
                answer = Some("once");
            }
            if p.always
                && action(
                    app,
                    ui,
                    &format!("{id}.always"),
                    s("Always for this project"),
                    s("Run it, and allow calls like it in this project's folder from now on (.claude/settings.local.json)."),
                    Weight::Secondary,
                    true,
                )
            {
                answer = Some("always");
            }
            if action(app, ui, &format!("{id}.deny"), s("Deny"), s("Do not run it; the agent is told you said no."), Weight::Danger, true) {
                answer = Some("deny");
            }
            if let Some(answer) = answer {
                call(app, ctx, v, "permission", json!({"index": i, "answer": answer}));
            }
        });
    });
    app.auto.add(&id, frame.response.rect, &p.tool);
}

/// The transcript line of an answered permission request.
pub(super) fn permission_line(tool: &str, answer: &str) -> String {
    let key = match answer {
        "allowed" => "{tool}: allowed once",
        "always" => "{tool}: allowed for this project",
        "denied" => "{tool}: denied",
        _ => "{tool}: not answered (the agent stopped waiting)",
    };
    f(key, &[("tool", tool)])
}
