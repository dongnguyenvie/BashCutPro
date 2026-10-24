//! The gate card and the permission card (AG8).

use bashcut_agent::Gate;
use serde_json::json;

use super::super::strings::{f, s};
use super::super::widgets::{CODE, NOTE, button, code_text, note};
use super::setup::hover_button;
use super::{View, call};
use crate::FilmcraftApp;

pub(super) fn gate_card(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, g: &Gate, running: bool) {
    let t = app.tokens;
    let frame = egui::Frame::group(ui.style()).stroke(egui::Stroke::new(1.0, t.accent)).show(ui, |ui| {
        ui.label(egui::RichText::new(f("Gate: {stage}", &[("stage", &g.stage)])).strong().color(t.text));
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
            if button(app, ui, "bashcut.gate.approve", s("Approve"), !running) && call(app, ctx, v, "approve", json!({"note": note})) {
                v.note.clear();
                v.gate_checked = None;
            }
            if button(app, ui, "bashcut.gate.change", s("Ask for changes"), !running && !note.is_empty()) && call(app, ctx, v, "change", json!({"note": note}))
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
    let frame = egui::Frame::group(ui.style()).stroke(egui::Stroke::new(1.0, t.accent)).show(ui, |ui| {
        let title = f("{agent} wants to use {tool}", &[("agent", agent_name), ("tool", &p.tool)]);
        ui.label(egui::RichText::new(title).strong().color(t.text));
        let r = ui.add(egui::Label::new(code_text(bashcut_agent::clip(&p.detail, 2000), CODE, t.text)).selectable(true).wrap());
        app.auto.add_response(&format!("{id}.detail"), &r, &p.detail);
        ui.horizontal_wrapped(|ui| {
            let mut answer = None;
            if hover_button(app, ui, &format!("{id}.once"), s("Allow once"), s("Run this call; ask again next time.")) {
                answer = Some("once");
            }
            if p.always
                && hover_button(
                    app,
                    ui,
                    &format!("{id}.always"),
                    s("Always for this project"),
                    s("Run it, and allow calls like it in this project's folder from now on (.claude/settings.local.json)."),
                )
            {
                answer = Some("always");
            }
            if hover_button(app, ui, &format!("{id}.deny"), s("Deny"), s("Do not run it; the agent is told you said no.")) {
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
