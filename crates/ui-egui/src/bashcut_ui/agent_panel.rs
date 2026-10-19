//! The Agent section (`bashcut/in-app-ui.md` §6): the agent and conversation pickers, the pending
//! gate as a card (Approve / Change), the transcript with tool calls grouped by command, and the
//! message box (Enter sends, Shift+Enter starts a new line). Every action goes through the
//! `bashcut.agent.*` commands ([`agent::invoke`]) and registers an automation id.

use bashcut_agent::{Gate, Item, Status};
use serde_json::{Value, json};

use super::agent;
use super::plugins_panel::button;
use super::strings::{f, s};
use crate::FilmcraftApp;

/// The pending gate is re-read from `plan.json` this often (seconds), not every frame.
const GATE_EVERY: f64 = 1.0;
/// Height kept for the message box and its buttons under the transcript.
const INPUT_HEIGHT: f32 = 96.0;

/// What the section holds between frames (egui context data, not `UiState`).
#[derive(Clone, Default)]
struct View {
    input: String,
    note: String,
    status: Option<String>,
    gate: Option<Gate>,
    gate_checked: Option<f64>,
    /// The project the gate was read for (another project re-reads it at once).
    project: Option<std::path::PathBuf>,
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.agent.view")
}

fn input_id() -> egui::Id {
    egui::Id::new("bashcut.agent.input")
}

/// What one frame draws, taken under the lock.
struct Snapshot {
    items: Vec<Item>,
    status: Status,
    agents: Vec<(String, String)>,
    agent: (String, String),
    title: String,
    has_project: bool,
}

fn call(app: &mut FilmcraftApp, ctx: &egui::Context, v: &mut View, name: &str, params: Value) -> bool {
    match agent::invoke(app, ctx, name, &params) {
        Ok(_) => {
            v.status = None;
            true
        }
        Err(e) => {
            v.status = Some(e);
            false
        }
    }
}

pub fn draw(app: &mut FilmcraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let t = app.tokens;
    let mut v = ctx.data_mut(|d| d.get_temp_mut_or_default::<View>(key()).clone());
    let c = agent::context(app);
    let now = ctx.input(|i| i.time);
    if v.project != c.project {
        v.project = c.project.clone();
        v.gate_checked = None;
    }
    let check_gate = v.gate_checked.is_none_or(|at| now - at >= GATE_EVERY);
    let (snap, gate) = agent::with(app, &ctx, |a| {
        a.set_project(c.project.as_deref());
        let snap = Snapshot {
            items: a.conversation.transcript.items.clone(),
            status: a.status.clone(),
            agents: a.agents.iter().map(|s| (s.id.clone(), s.name.clone())).collect(),
            agent: (a.agent.id.clone(), a.agent.name.clone()),
            title: a.conversation.title(),
            has_project: a.sidecar().is_some(),
        };
        let gate = if check_gate && !a.is_running() { Some(a.gate()) } else { None };
        (snap, gate)
    });
    if let Some(g) = gate {
        v.gate = g;
        v.gate_checked = Some(now);
    }
    let running = matches!(snap.status, Status::Running | Status::Stopping);

    header(app, ui, &ctx, &mut v, &snap, running);
    if let Some(e) = &v.status {
        let r = ui.label(egui::RichText::new(e).size(11.0).color(egui::Color32::from_rgb(0xe0, 0x6c, 0x5f)));
        app.auto.add_response("bashcut.agent.status", &r, e);
    }
    if !snap.has_project {
        ui.label(egui::RichText::new(s("Save the project to keep conversations and gates next to it.")).size(11.0).color(t.text_dim));
    }
    if let Some(g) = v.gate.clone() {
        gate_card(app, ui, &ctx, &mut v, &g, running);
    }

    let height = (ui.available_height() - INPUT_HEIGHT).max(80.0);
    let out =
        egui::ScrollArea::vertical().id_salt("bashcut.agent.transcript").max_height(height).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            if snap.items.is_empty() {
                ui.label(egui::RichText::new(f("Ask {agent} to edit this project.", &[("agent", &snap.agent.1)])).size(11.0).color(t.text_dim));
            }
            transcript(app, ui, &snap.items);
            if running {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let label = if snap.status == Status::Stopping { s("Stopping…") } else { s("Working…") };
                    ui.label(egui::RichText::new(label).size(11.0).color(t.text_dim));
                });
            }
        });
    app.auto.add("bashcut.agent.transcript", out.inner_rect, &snap.title);

    ui.separator();
    input(app, ui, &ctx, &mut v, running);
    if running {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    ctx.data_mut(|d| d.insert_temp(key(), v));
}

fn header(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, snap: &Snapshot, running: bool) {
    ui.horizontal(|ui| {
        let mut chosen = None;
        let combo = egui::ComboBox::from_id_salt("bashcut.agent.picker").selected_text(snap.agent.1.as_str()).width(120.0).show_ui(ui, |ui| {
            for (id, name) in &snap.agents {
                if ui.selectable_label(*id == snap.agent.0, name.as_str()).clicked() {
                    chosen = Some(id.clone());
                }
            }
        });
        app.auto.add_response("bashcut.agent.picker", &combo.response, &snap.agent.1);
        if combo.response.clicked() {
            // pick up agent plugins installed since
            call(app, ctx, v, "agents", json!({}));
        }
        if let Some(id) = chosen.filter(|id| *id != snap.agent.0) {
            call(app, ctx, v, "use", json!({"agent": id}));
        }

        let mut open = None;
        let title = if snap.title.is_empty() { s("New conversation").to_string() } else { snap.title.clone() };
        let combo =
            egui::ComboBox::from_id_salt("bashcut.agent.conversations").selected_text(title.as_str()).width(ui.available_width() - 60.0).show_ui(ui, |ui| {
                // read only while the list is open
                let list = agent::with(app, ctx, |a| a.conversations());
                if list.is_empty() {
                    ui.label(egui::RichText::new(s("No saved conversations.")).size(11.0));
                }
                for c in list {
                    let label = if c.title.is_empty() { c.id.clone() } else { c.title.clone() };
                    if ui.selectable_label(false, label).clicked() {
                        open = Some(c.id);
                    }
                }
            });
        app.auto.add_response("bashcut.agent.conversations", &combo.response, &title);
        if let Some(id) = open {
            call(app, ctx, v, "open", json!({"id": id}));
        }
        if button(app, ui, "bashcut.agent.new", s("New"), !running && !snap.items.is_empty()) {
            call(app, ctx, v, "new", json!({}));
        }
    });
}

fn gate_card(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, g: &Gate, running: bool) {
    let t = app.tokens;
    let frame = egui::Frame::group(ui.style()).stroke(egui::Stroke::new(1.0, t.accent)).show(ui, |ui| {
        ui.label(egui::RichText::new(f("Gate: {stage}", &[("stage", &g.stage)])).strong().color(t.text));
        if !g.summary.is_empty() {
            ui.label(egui::RichText::new(&g.summary).size(11.5).color(t.text));
        }
        for line in g.evidence_lines() {
            ui.label(egui::RichText::new(format!("• {line}")).size(11.0).color(t.text_dim));
        }
        let r = ui.add(egui::TextEdit::multiline(&mut v.note).hint_text(s("Note (needed to ask for changes)")).desired_rows(2).desired_width(f32::INFINITY));
        app.auto.add_response("bashcut.gate.note", &r, "note");
        ui.horizontal(|ui| {
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

/// The transcript; consecutive tool calls with the same key fold into one row.
fn transcript(app: &mut FilmcraftApp, ui: &mut egui::Ui, items: &[Item]) {
    let t = app.tokens;
    let mut i = 0;
    while let Some(item) = items.get(i) {
        match item {
            Item::User { text } => {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(s("You")).size(10.5).strong().color(t.text_dim));
                ui.add(egui::Label::new(egui::RichText::new(text).color(t.text)).selectable(true).wrap());
            }
            Item::Assistant { text } => {
                ui.add_space(4.0);
                ui.add(egui::Label::new(egui::RichText::new(text).color(t.text)).selectable(true).wrap());
            }
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
            Item::Gate { stage, state, note } => {
                let what = if state == bashcut_agent::plan::APPROVED { s("approved") } else { s("changes requested") };
                let mut line = f("Gate {stage}: {state}", &[("stage", stage), ("state", what)]);
                if !note.is_empty() {
                    line.push_str(&format!(" — {note}"));
                }
                ui.label(egui::RichText::new(line).size(11.0).color(t.accent));
            }
            Item::Error { message } => {
                ui.label(egui::RichText::new(message).size(11.0).color(egui::Color32::from_rgb(0xe0, 0x6c, 0x5f)));
            }
            Item::Notice { message } => {
                ui.label(egui::RichText::new(message).size(10.5).color(t.text_dim));
            }
        }
        i += 1;
    }
}

fn tool_group(app: &mut FilmcraftApp, ui: &mut egui::Ui, at: usize, key: &str, group: &[Item]) {
    let t = app.tokens;
    let failed = group.iter().any(|g| matches!(g, Item::Tool { is_error: true, .. }));
    let waiting = group.iter().any(|g| matches!(g, Item::Tool { output: None, .. }));
    let mark = if failed {
        "✗"
    } else if waiting {
        "…"
    } else {
        "✓"
    };
    let title = if group.len() > 1 { format!("{mark} {key} ×{}", group.len()) } else { format!("{mark} {key}") };
    let head =
        egui::CollapsingHeader::new(egui::RichText::new(&title).monospace().size(10.5).color(t.text_dim)).id_salt(("bashcut.agent.tool", at)).show(ui, |ui| {
            for g in group {
                let Item::Tool { input, output, .. } = g else { continue };
                ui.label(egui::RichText::new(bashcut_agent::clip(&input.to_string(), 2000)).monospace().size(10.0).color(t.text_dim));
                if let Some(o) = output {
                    ui.label(egui::RichText::new(bashcut_agent::clip(o, 4000)).monospace().size(10.0).color(t.text));
                }
            }
            // an engine command or plugin action the agent ran once can run on many files (#46)
            let first = group.iter().find_map(|g| match g {
                Item::Tool { input, .. } => Some(input.get("params").cloned().unwrap_or(json!({}))),
                _ => None,
            });
            if let Some(prefill) = first.and_then(|p| super::batch::prefill_params(key, &p))
                && button(app, ui, &format!("bashcut.agent.tool.{at}.batch"), s("Run on many files…"), true)
            {
                let ctx = ui.ctx().clone();
                if let Err(e) = super::batch::invoke(app, &ctx, "prefill", &prefill) {
                    app.ui.status = e;
                }
            }
        });
    app.auto.add_response(&format!("bashcut.agent.tool.{at}"), &head.header_response, &title);
}

fn input(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, running: bool) {
    // Enter sends, Shift+Enter is a new line: take the Enter press away before the text box sees it
    let focused = ctx.memory(|m| m.has_focus(input_id()));
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
            .desired_width(f32::INFINITY),
    );
    app.auto.add_response("bashcut.agent.input", &r, "message");
    ui.horizontal(|ui| {
        let can_send = !running && !v.input.trim().is_empty();
        let clicked = button(app, ui, "bashcut.agent.send", s("Send"), can_send);
        let text = v.input.trim().to_string();
        if (clicked || (enter && can_send)) && call(app, ctx, v, "send", json!({"text": text})) {
            v.input.clear();
        }
        if button(app, ui, "bashcut.agent.stop", s("Stop"), running) {
            call(app, ctx, v, "stop", json!({}));
        }
    });
}
