//! Header, readiness lines and the terminal row (UX7, AG5, AG6, AG9, AG16).

use serde_json::json;

use super::super::agent;
use super::super::hints::{self, HOW_TO_TURN_ON};
use super::super::strings::{f, s};
use super::super::terminal;
use super::super::widgets::{NOTE, button, code_text, note};
use super::{Snapshot, View, call, report};
use crate::FilmcraftApp;

/// One line per missing item, before the user presses Send; `true` when Open in Terminal was clicked.
pub(super) fn readiness(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, snap: &Snapshot) -> bool {
    let t = app.tokens;
    if let Some(line) = &snap.missing {
        ui.horizontal_wrapped(|ui| {
            let r = note(ui, line, NOTE, t.danger);
            app.auto.add_response("bashcut.agent.ready.agent", &r, line);
            if snap.agent.0 == "claude-code"
                && hover_button(app, ui, "bashcut.agent.install", s("Install"), s("Open a terminal with the official Claude Code installer."))
            {
                report(app, agent::install(snap.project.as_deref()));
            }
        });
    }
    if snap.outdated {
        let line = s(super::OUTDATED_LINE);
        let r = note(ui, line, NOTE, t.danger);
        app.auto.add_response("bashcut.agent.ready.update", &r, line);
    }
    if let Some((category, message)) = &snap.auth {
        auth_line(app, ui, snap, category, message);
    }
    if snap.kit_missing {
        kit_line(app, ui, snap);
    }
    let handoff = terminal_row(app, ui, snap);
    if !hints::control_on() {
        let line = s("Control channel off: the agent cannot edit the open project.");
        let r = note(ui, line, NOTE, t.text_dim).on_hover_text(s(HOW_TO_TURN_ON));
        app.auto.add_response("bashcut.agent.ready.control", &r, line);
    }
    super::super::control::setting(app, ui);
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
    handoff
}

/// "Claude Code is not signed in" or "… account problem: billing" (AG6).
pub(super) fn auth_headline(agent: &str, category: &str) -> String {
    let key = match category {
        bashcut_agent::stream_json::NOT_SIGNED_IN => "{agent} is not signed in",
        "billing_error" => "{agent} account problem: billing",
        "oauth_org_not_allowed" => "{agent} account problem: this organization is not allowed",
        _ => "{agent} account problem: {category}",
    };
    f(key, &[("agent", agent), ("category", category)])
}

/// The sign-in readiness line: headline, Sign in (Claude Code), and the agent's message, selectable.
pub(super) fn auth_line(app: &mut FilmcraftApp, ui: &mut egui::Ui, snap: &Snapshot, category: &str, message: &str) {
    let t = app.tokens;
    let line = auth_headline(&snap.agent.1, category);
    ui.horizontal_wrapped(|ui| {
        let r = note(ui, &line, NOTE, t.danger);
        app.auto.add_response("bashcut.agent.ready.auth", &r, &line);
        if snap.agent.0 == "claude-code"
            && let Some(program) = snap.resume.as_ref().and_then(|a| a.first())
        {
            let program = program.clone();
            sign_in_button(app, ui, &program, snap.project.as_deref());
        }
    });
    if !message.is_empty() {
        let r = ui.add(egui::Label::new(egui::RichText::new(message).size(NOTE).color(t.text_dim)).selectable(true).wrap());
        app.auto.add_response("bashcut.agent.ready.auth.message", &r, message);
    }
}

/// The optional global install, as the agent-kit README gives it (shown selectable; the app never runs it).
pub(super) fn kit_commands() -> String {
    format!("claude plugin marketplace add {} && claude plugin install {}", terminal::KIT_MARKETPLACE, terminal::KIT_INSTALL)
}

/// "BashCutPro skills are not downloaded" (AG9, AG16): Download kit… (git), or the zip URL without git, and the
/// global install command to run by hand.
pub(super) fn kit_line(app: &mut FilmcraftApp, ui: &mut egui::Ui, snap: &Snapshot) {
    let t = app.tokens;
    let git = agent::git_found();
    ui.horizontal_wrapped(|ui| {
        let line = s("BashCutPro skills are not downloaded");
        let r = note(ui, line, NOTE, t.danger).on_hover_text(s("The BashCutPro kit (bcp plugin for Claude Code) brings the /bcp: recipes and skills."));
        app.auto.add_response("bashcut.agent.ready.kit", &r, line);
        if git
            && snap.resume.is_some()
            && hover_button(
                app,
                ui,
                "bashcut.agent.installKit",
                s("Download kit…"),
                s("Open a terminal that downloads the BashCutPro kit (git) into BashCutPro's data folder. Your next message loads it."),
            )
        {
            report(app, agent::install_kit(&snap.cwd()));
        }
    });
    if git {
        note(ui, s("When the download finishes, your next message loads the kit."), NOTE, t.text_dim);
    } else {
        let folder = agent::kit_target().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        let text = f("Git was not found. Download {url} and unpack it into {folder}.", &[("url", terminal::KIT_ZIP_URL), ("folder", &folder)]);
        let r = ui.add(egui::Label::new(egui::RichText::new(&text).size(NOTE).color(t.text_dim)).selectable(true).wrap());
        app.auto.add_response("bashcut.agent.ready.kit.zip", &r, &text);
    }
    note(ui, s("Want the skills in every Claude Code session? Run:"), NOTE, t.text_dim);
    let cmds = kit_commands();
    let r = ui.add(egui::Label::new(code_text(&cmds, NOTE, t.text_dim)).selectable(true).wrap());
    app.auto.add_response("bashcut.agent.ready.kit.commands", &r, &cmds);
}

pub(super) fn hover_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, hover: &str) -> bool {
    let r = ui.add(egui::Button::new(label).small()).on_hover_text(hover);
    app.auto.add_response(id, &r, label);
    r.clicked()
}

/// Open in Terminal and Sign in, for an agent whose program was found (Claude Code, Codex); `true`
/// when Open in Terminal was clicked.
pub(super) fn terminal_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, snap: &Snapshot) -> bool {
    let Some(argv) = snap.resume.as_ref() else { return false };
    ui.horizontal_wrapped(|ui| {
        let tip = f("Continue this conversation with {agent} in a terminal window.", &[("agent", &snap.agent.1)]);
        let clicked = hover_button(app, ui, "bashcut.agent.terminal", s("Open in Terminal"), &tip);
        // the sign-in line has its own Sign in (AG6)
        if snap.agent.0 == "claude-code" && snap.auth.is_none() {
            let program = argv.first().cloned().unwrap_or_default();
            sign_in_button(app, ui, &program, snap.project.as_deref());
        }
        clicked
    })
    .inner
}

/// Sign in ([`agent::sign_in`], AG6 shows it when auth fails).
pub fn sign_in_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, program: &str, project: Option<&std::path::Path>) {
    let tip = s("Open Claude Code in a terminal to sign in (type /login). BashCutPro never sees your login.");
    if hover_button(app, ui, "bashcut.agent.signIn", s("Sign in…"), tip) {
        report(app, agent::sign_in(program, project));
    }
}

pub(super) fn header(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, snap: &Snapshot, running: bool) {
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
