//! The Agent section (`bashcut/in-app-ui.md` §6): the agent and conversation pickers, the pending
//! gate as a card (Approve / Change), the transcript with tool calls grouped by command, and the
//! message box (Enter sends, Shift+Enter starts a new line). Every action goes through the
//! `bashcut.agent.*` commands ([`agent::invoke`]) and registers an automation id.
//!
//! Under the header, one readiness line per missing item (UX7): the agent's program, the control
//! channel, a saved project. Agents whose program is missing are greyed out in the picker and
//! Send is disabled with the reason.

use bashcut_agent::{Gate, Item, Status};
use serde_json::{Value, json};

use super::agent;
use super::hints::{self, HOW_TO_TURN_ON};
use super::strings::{f, s};
use super::widgets::{META, NOTE, button, note};
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
    /// id, name, program found ([`hints::agent_found`])
    agents: Vec<(String, String, bool)>,
    agent: (String, String),
    /// The current agent's program is missing: why Send is disabled.
    missing: Option<String>,
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
    let mut v = super::shared::temp::<View>(&ctx, key());
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
            agents: a.agents.iter().map(|s| (s.id.clone(), s.name.clone(), hints::agent_found(s))).collect(),
            agent: (a.agent.id.clone(), a.agent.name.clone()),
            missing: (!hints::agent_found(&a.agent)).then(|| missing_line(&a.agent)),
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
    readiness(app, ui, &ctx, &snap);
    super::toast::status_line(app, ui, "bashcut.agent.status", v.status.as_deref(), t.danger);
    if let Some(g) = v.gate.clone() {
        gate_card(app, ui, &ctx, &mut v, &g, running);
    }

    let height = (ui.available_height() - INPUT_HEIGHT).max(80.0);
    let out =
        egui::ScrollArea::vertical().id_salt("bashcut.agent.transcript").max_height(height).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            if snap.items.is_empty() {
                note(ui, f("Ask {agent} to edit this project.", &[("agent", &snap.agent.1)]), NOTE, t.text_dim);
                if let Some(prompt) = examples(app, ui) {
                    v.input = prompt;
                    ctx.memory_mut(|m| m.request_focus(input_id()));
                }
                note(ui, s("The agent stops at each stage for your approval."), NOTE, t.text_dim);
            }
            transcript(app, ui, &snap.items);
            if running {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let label = if snap.status == Status::Stopping { s("Stopping…") } else { s("Working…") };
                    note(ui, label, NOTE, t.text_dim);
                });
            }
        });
    app.auto.add("bashcut.agent.transcript", out.inner_rect, &snap.title);

    ui.separator();
    input(app, ui, &ctx, &mut v, running, snap.missing.as_deref());
    if running {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    super::shared::put_temp(&ctx, key(), v);
}

/// "Claude Code not found: install it, or set BASHCUT_CLAUDE" (an agent plugin: install its program).
fn missing_line(spec: &bashcut_agent::AgentSpec) -> String {
    let var = match spec.id.as_str() {
        "claude-code" => Some("BASHCUT_CLAUDE"),
        "codex" => Some("BASHCUT_CODEX"),
        _ => None,
    };
    match var {
        Some(var) => f("{agent} not found: install it, or set {var}", &[("agent", &spec.name), ("var", var)]),
        None => f("{agent} not found: install {program}", &[("agent", &spec.name), ("program", spec.argv.first().map_or("", String::as_str))]),
    }
}

/// One line per missing item, before the user presses Send.
fn readiness(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, snap: &Snapshot) {
    let t = app.tokens;
    if let Some(line) = &snap.missing {
        let r = note(ui, line, NOTE, t.danger);
        app.auto.add_response("bashcut.agent.ready.agent", &r, line);
    }
    if !hints::control_on() {
        let line = s("Control channel off: the agent cannot edit the open project.");
        let r = note(ui, line, NOTE, t.text_dim).on_hover_text(s(HOW_TO_TURN_ON));
        app.auto.add_response("bashcut.agent.ready.control", &r, line);
    }
    super::control::setting(app, ui);
    if !snap.has_project {
        ui.horizontal_wrapped(|ui| {
            let line = s("Project not saved: conversations will not be kept");
            let r = note(ui, line, NOTE, t.text_dim);
            app.auto.add_response("bashcut.agent.ready.project", &r, line);
            if button(app, ui, "bashcut.agent.save", s("Save…"), true)
                && let Err(e) = crate::menus::invoke(app, ctx, "file.save", json!({}))
            {
                app.ui.status = e;
            }
        });
    }
}

fn header(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, snap: &Snapshot, running: bool) {
    ui.horizontal(|ui| {
        let mut chosen = None;
        let combo = egui::ComboBox::from_id_salt("bashcut.agent.picker").selected_text(snap.agent.1.as_str()).width(120.0).show_ui(ui, |ui| {
            for (id, name, found) in &snap.agents {
                let label = if *found { name.clone() } else { f("{agent} (not installed)", &[("agent", name)]) };
                let r = ui.add_enabled(*found || *id == snap.agent.0, egui::Button::selectable(*id == snap.agent.0, label.as_str()));
                app.auto.add_response(&format!("bashcut.agent.picker.{id}"), &r, &label);
                if r.clicked() {
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

        // New first from the right, so a long conversation title truncates instead of pushing it out
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(app, ui, "bashcut.agent.new", s("New"), !running && !snap.items.is_empty()) {
                call(app, ctx, v, "new", json!({}));
            }
            let mut open = None;
            let title = if snap.title.is_empty() { s("New conversation").to_string() } else { snap.title.clone() };
            let combo = egui::ComboBox::from_id_salt("bashcut.agent.conversations")
                .selected_text(title.as_str())
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
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
            let combo_response = combo.response.on_hover_text(&title);
            app.auto.add_response("bashcut.agent.conversations", &combo_response, &title);
            if let Some(id) = open {
                call(app, ctx, v, "open", json!({"id": id}));
            }
        });
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
            note(ui, format!("• {line}"), NOTE, t.text_dim);
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
/// Kit recipes ([`super::palette::RECIPES`]) offered as example prompts on an empty transcript.
const EXAMPLES: &[&str] = &["vlog-plan", "vlog-hook-script", "vlog-publish"];

/// One button per [`EXAMPLES`] recipe; the clicked one's prompt fills the message box (not sent).
fn examples(app: &mut FilmcraftApp, ui: &mut egui::Ui) -> Option<String> {
    let mut picked = None;
    ui.add_space(4.0);
    for (id, name, _) in super::palette::RECIPES.iter().filter(|(id, ..)| EXAMPLES.contains(id)) {
        if button(app, ui, &format!("bashcut.agent.example.{id}"), s(name), true) {
            picked = Some(format!("Use the bcp:{id} recipe on this project."));
        }
    }
    ui.add_space(4.0);
    picked
}

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
                ui.add_space(6.0);
                ui.label(egui::RichText::new(s("Agent")).size(10.5).strong().color(t.text_dim));
                super::markdown::show(ui, text, t.text, t.text_dim);
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
                note(ui, message, NOTE, t.danger);
            }
            Item::Notice { message } => {
                note(ui, message, META, t.text_dim);
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

fn input(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, running: bool, missing: Option<&str>) {
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
        let can_send = !running && !v.input.trim().is_empty() && missing.is_none();
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

#[cfg(test)]
mod tests {
    use super::super::strings::set_language;
    use super::*;

    #[test]
    fn missing_agent_line_names_the_override() {
        set_language(Some("en"));
        let fake = bashcut_agent::AgentSpec::claude_code(Some("bashcut-no-such-agent-ux7".into()));
        assert!(!hints::agent_found(&fake));
        assert_eq!(missing_line(&fake), "Claude Code not found: install it, or set BASHCUT_CLAUDE");
        let codex = bashcut_agent::AgentSpec::codex(Some("bashcut-no-such-codex-ux7".into()));
        assert_eq!(missing_line(&codex), "Codex not found: install it, or set BASHCUT_CODEX");
        assert_eq!(f("{agent} (not installed)", &[("agent", "Codex")]), "Codex (not installed)");
        let plugin = bashcut_agent::AgentSpec { id: "x".into(), name: "X".into(), argv: vec!["xagent".into()], ..Default::default() };
        assert_eq!(missing_line(&plugin), "X not found: install xagent");
        set_language(Some("vi"));
        for line in [missing_line(&fake), missing_line(&plugin), f("{agent} (not installed)", &[("agent", "Codex")])] {
            assert!(!line.contains("not found") && !line.contains("not installed"), "{line}");
        }
        for k in ["Control channel off: the agent cannot edit the open project.", "Project not saved: conversations will not be kept", "Save…"] {
            assert_ne!(s(k), k);
        }
        set_language(None);
    }
}
