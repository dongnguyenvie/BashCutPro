//! Header with its overflow menu, the Setup strip and the terminal status line (UX7, AG5, AG6, AG9, AG16, AGU7).

use serde_json::json;

use super::super::agent;
use super::super::hints::{self, HOW_TO_TURN_ON};
use super::super::strings::{f, s};
use super::super::terminal;
use super::super::widgets::{NOTE, button, code_text, icon_button, icon_button_response, note};
use super::{Snapshot, View, call, report};
use crate::FilmcraftApp;
use crate::icons::Icon;

/// The Setup strip (AGU7): nothing when all is ready, the one line when one item is missing, "n things to set up ▸"
/// expanding to the lines for more. Under it the terminal session's state. `true` when the menu's Open in
/// Terminal was clicked (see [`header`]).
pub(super) fn readiness(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, snap: &Snapshot) -> bool {
    let control_off = !hints::control_on();
    let issues = usize::from(snap.missing.is_some())
        + usize::from(snap.outdated)
        + usize::from(snap.auth.is_some())
        + usize::from(snap.kit_missing)
        + usize::from(control_off)
        + usize::from(!snap.has_project);
    match issues {
        0 => {}
        1 => lines(app, ui, ctx, snap, control_off),
        n => {
            let id = ui.make_persistent_id("bashcut.agent.setup");
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, false);
            let title = f("{n} things to set up", &[("n", &n.to_string())]);
            let head = ui.add(
                egui::Button::new(egui::RichText::new(format!("{} {title}", if state.is_open() { "▾" } else { "▸" })).size(NOTE).color(app.tokens.danger))
                    .frame(false),
            );
            app.auto.add_response("bashcut.agent.setup", &head, &title);
            if head.clicked() {
                state.toggle(ui);
            }
            state.show_body_unindented(ui, |ui| lines(app, ui, ctx, snap, control_off));
        }
    }
    status_row(app, ui, snap);
    take_menu_click(ctx)
}

/// One line per missing item (ids `bashcut.agent.ready.*`); the control checkbox sits under its line while the channel is off.
fn lines(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, snap: &Snapshot, control_off: bool) {
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
    if control_off {
        let line = s("Control channel off: the agent cannot edit the open project.");
        let r = note(ui, line, NOTE, t.text_dim).on_hover_text(s(HOW_TO_TURN_ON));
        app.auto.add_response("bashcut.agent.ready.control", &r, line);
        super::super::control::setting(app, ui);
    }
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

/// The terminal session's state from its hooks (AGT1), under the header.
fn status_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, snap: &Snapshot) {
    if snap.resume.is_none() {
        return;
    }
    if let Some(phase) = agent::terminal_phase(ui.ctx()) {
        let (line, color) = match phase {
            terminal::Phase::Opened => (s("Terminal: opened"), app.tokens.text_dim),
            terminal::Phase::Working => (s("Terminal: working…"), app.tokens.text_dim),
            terminal::Phase::Waiting => (s("Terminal: waiting for your permission"), app.tokens.danger),
            terminal::Phase::Done => (s("Terminal: done"), app.tokens.text_dim),
            terminal::Phase::Failed => (s("Terminal: stopped with an error"), app.tokens.danger),
            terminal::Phase::Closed => (s("Terminal: closed"), app.tokens.text_dim),
        };
        let r = note(ui, line, NOTE, color);
        app.auto.add_response("bashcut.agent.terminal.status", &r, line);
    }
}

/// The header's menu clicked Open in Terminal this frame; [`readiness`] hands it to the panel.
fn menu_click_id() -> egui::Id {
    egui::Id::new("bashcut.agent.menu.terminal")
}

fn take_menu_click(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.remove_temp::<bool>(menu_click_id())).unwrap_or(false)
}

/// The overflow menu: Open in Terminal, Sign in… (unless the auth line has it) and the control checkbox (inline in
/// the Setup strip instead while the channel is off).
fn overflow_menu(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, snap: &Snapshot) {
    let r = icon_button_response(app, ui, "bashcut.agent.menu", Icon::Hamburger, s("More"), true);
    egui::Popup::menu(&r).show(|ui| {
        ui.set_min_width(180.0);
        if let Some(argv) = snap.resume.as_ref() {
            let tip = f("Continue this conversation with {agent} in a terminal window.", &[("agent", &snap.agent.1)]);
            if hover_button(app, ui, "bashcut.agent.terminal", s("Open in Terminal"), &tip) {
                ctx.data_mut(|d| d.insert_temp(menu_click_id(), true));
                ui.close();
            }
            // the sign-in line has its own Sign in (AG6)
            if snap.agent.0 == "claude-code" && snap.auth.is_none() {
                let program = argv.first().cloned().unwrap_or_default();
                sign_in_button(app, ui, &program, snap.project.as_deref());
            }
        }
        if hints::control_on() {
            super::super::control::setting(app, ui);
        }
    });
}

/// Sign in ([`agent::sign_in`], AG6 shows it when auth fails).
pub fn sign_in_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, program: &str, project: Option<&std::path::Path>) {
    let tip = s("Open Claude Code in a terminal to sign in (type /login). BashCutPro never sees your login.");
    if hover_button(app, ui, "bashcut.agent.signIn", s("Sign in…"), tip) {
        report(app, agent::sign_in(program, project));
    }
}

/// Below this the conversation picker gets its own row (240 px panel).
const NARROW: f32 = 300.0;

pub(super) fn header(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, snap: &Snapshot, running: bool) {
    let narrow = ui.available_width() < NARROW;
    ui.horizontal(|ui| {
        let mut chosen = None;
        // sized to its text, not less than 90 px
        let text = egui::WidgetText::from(snap.agent.1.as_str()).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
        let width = (text.size().x + 28.0).clamp(90.0, 150.0);
        let combo = egui::ComboBox::from_id_salt("bashcut.agent.picker").selected_text(snap.agent.1.as_str()).width(width).show_ui(ui, |ui| {
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

        // icons first from the right, so a long conversation title truncates instead of pushing them out
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            overflow_menu(app, ui, ctx, snap);
            new_button(app, ui, ctx, v, snap, running);
            if !narrow {
                conversation_picker(app, ui, ctx, v, snap);
            }
        });
    });
    if narrow {
        conversation_picker(app, ui, ctx, v, snap);
    }
}

fn new_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, snap: &Snapshot, running: bool) {
    if icon_button(app, ui, "bashcut.agent.new", Icon::NewItem, s("New"), !running && !snap.items.is_empty()) {
        call(app, ctx, v, "new", json!({}));
    }
}

fn conversation_picker(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, snap: &Snapshot) {
    let mut open = None;
    let title = if snap.title.is_empty() { s("New conversation").to_string() } else { snap.title.clone() };
    let combo =
        egui::ComboBox::from_id_salt("bashcut.agent.conversations").selected_text(title.as_str()).width(ui.available_width()).truncate().show_ui(ui, |ui| {
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
}
