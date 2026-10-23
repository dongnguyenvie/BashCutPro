//! The Agent section (`bashcut/in-app-ui.md` §6): the agent and conversation pickers, the pending
//! gate as a card (Approve / Change), the transcript with tool calls grouped by command, and the
//! message box (Enter sends, Shift+Enter starts a new line). Every action goes through the
//! `bashcut.agent.*` commands ([`agent::invoke`]) and registers an automation id.
//!
//! Under the header, one readiness line per missing item (UX7): the agent's program, the control
//! channel, a saved project, a Claude Code too old for a live session. Agents whose program is
//! missing are greyed out in the picker and Send is disabled with the reason.
//!
//! A live agent (Claude Code, AG2) takes messages while it works: Send stays enabled, and a message
//! sent mid-turn shows under the transcript as queued until the agent takes it; one it never read
//! (it failed or was stopped) goes back into the message box.
//!
//! AG4: queued messages are numbered in the order the agent gets them, each with ✕
//! (`bashcut.agent.queued.<n>.cancel`, `bashcut.agent.cancel`): the agent never answers a removed one.
//!
//! AG5: Open in Terminal continues the conversation in the agent's own terminal UI, Sign in opens
//! `claude` for `/login`, and a missing Claude Code gets an Install button ([`super::terminal`]).
//!
//! AG6: when the latest turn ended on a sign-in or account problem ([`bashcut_agent::Item::Auth`],
//! decoded from the event stream only), a readiness line says so with Sign in, and the agent's own
//! message stays selectable under it.
//!
//! AG9: when the live session's `system/init` lists no `bcp` plugin, a readiness line says the
//! BashCutPro skills are not installed, with the kit's install commands and Install kit (a terminal
//! the user starts; never automatic), and Open in Terminal passes the app's MCP server instead.
//!
//! AG12: typing a `/` word in the message box lists the live session's slash commands and skills
//! (from its `system/init`, [`bashcut_agent::stream_json::SlashCommand`]) above the box: prefix
//! matches first, ↑/↓ pick, Enter/Tab insert `/name `, Esc closes, a click inserts too (rows
//! `bashcut.agent.slash.<i>`). Before the first init it says a message loads them; Codex has no list.
//!
//! AG8: a tool call Claude Code needs approval for shows as a card at the end of the transcript (tool,
//! the command or file): Allow once / Always for this project / Deny (`bashcut.agent.permission.<n>.*`,
//! `bashcut.agent.permission`); the answer stays in the transcript as one line.

use bashcut_agent::stream_json::SlashCommand;
use bashcut_agent::{Gate, Item, Status};
use serde_json::{Value, json};

use super::agent;
use super::hints::{self, HOW_TO_TURN_ON};
use super::strings::{f, s};
use super::terminal;
use super::widgets::{CODE, META, NOTE, button, close_button, code_note, code_text, note, scroll_clear_of_bar, strong_note};
use crate::FilmcraftApp;

/// The pending gate is re-read from `plan.json` this often (seconds), not every frame.
const GATE_EVERY: f64 = 1.0;
/// After this long (seconds) with no event from a running agent, a hint says where it may be
/// stuck (AG1: a macOS permission prompt holds `claude` before its first line). Not a stop.
const QUIET_HINT: f64 = 30.0;
/// Height kept for the message box and its buttons under the transcript.
const INPUT_HEIGHT: f32 = 96.0;
/// Rows the `/` list shows at most (AG12).
const SLASH_ROWS: usize = 8;

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
    /// While a turn runs: the agent's event count and when it last changed (egui time).
    heard: Option<(u64, f64)>,
    /// The `/` list (AG12): the picked row, the word it was picked for, the word Esc closed it on,
    /// and where it was drawn last frame (a click on it keeps it open while the box loses focus).
    slash_sel: usize,
    slash_word: String,
    slash_closed: Option<String>,
    slash_rect: Option<egui::Rect>,
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
    /// [`bashcut_agent::Agent::events_seen`].
    events: u64,
    /// The agent's program with its resume arguments (Claude Code, Codex), for Open in Terminal.
    resume: Option<Vec<String>>,
    /// The open project, for the folder turns run in ([`Snapshot::cwd`]).
    project: Option<std::path::PathBuf>,
    /// [`bashcut_agent::Agent::is_live`]: Send stays enabled while it works.
    live: bool,
    /// [`bashcut_agent::Agent::queued_items`].
    queued: Vec<bashcut_agent::session::Queued>,
    /// [`bashcut_agent::Agent::outdated`].
    outdated: bool,
    /// The sign-in or account problem the latest turn ended on (category, the agent's message).
    auth: Option<(String, String)>,
    /// [`agent::kit_missing`].
    kit_missing: bool,
    /// [`bashcut_agent::Agent::permissions`].
    permissions: Vec<bashcut_agent::session::PermissionView>,
    /// The agent's events are Claude Code's stream-json, whose init lists slash commands (AG12).
    slash: bool,
    /// [`bashcut_agent::Agent::commands`], read only while the message box holds a `/`.
    commands: Option<Vec<SlashCommand>>,
}

impl Snapshot {
    /// The folder turns run in ([`bashcut_agent::session::turn_dir`]; it may create it, so only on a click).
    fn cwd(&self) -> std::path::PathBuf {
        bashcut_agent::session::turn_dir(self.project.as_deref())
    }
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
    let has_slash = v.input.contains('/');
    let (snap, gate, unsent) = agent::with(app, &ctx, |a| {
        a.set_project(c.project.as_deref());
        let snap = Snapshot {
            items: a.conversation.transcript.items.clone(),
            status: a.status.clone(),
            agents: a.agents.iter().map(|s| (s.id.clone(), s.name.clone(), hints::agent_found(s))).collect(),
            agent: (a.agent.id.clone(), a.agent.name.clone()),
            missing: (!hints::agent_found(&a.agent)).then(|| missing_line(&a.agent)),
            title: a.conversation.title(),
            has_project: a.sidecar().is_some(),
            events: a.events_seen(),
            resume: agent::resume(a),
            project: c.project.clone(),
            live: a.is_live(),
            queued: a.queued_items(),
            outdated: a.outdated(),
            // FX28: a program that is not found wins over an earlier sign-in problem
            auth: a.conversation.transcript.auth_problem().filter(|_| hints::agent_found(&a.agent)).map(|(c, m)| (c.to_string(), m.to_string())),
            kit_missing: agent::kit_missing(a),
            permissions: a.permissions(),
            slash: a.agent.events == bashcut_agent::stream_json::FORMAT,
            commands: if has_slash { a.commands.clone() } else { None },
        };
        let gate = if check_gate && !a.is_running() { Some(a.gate()) } else { None };
        (snap, gate, a.take_unsent())
    });
    if !unsent.is_empty() {
        // what the agent never read goes back into the message box, before anything typed since
        let mut parts = unsent;
        parts.extend(Some(v.input.trim().to_string()).filter(|t| !t.is_empty()));
        v.input = parts.join("\n\n");
    }
    if let Some(g) = gate {
        v.gate = g;
        v.gate_checked = Some(now);
    }
    let running = matches!(snap.status, Status::Running | Status::Stopping);
    v.heard = match v.heard {
        _ if !running => None,
        Some((n, at)) if n == snap.events => Some((n, at)),
        _ => Some((snap.events, now)),
    };
    let quiet = v.heard.is_some_and(|(_, at)| now - at >= QUIET_HINT);

    header(app, ui, &ctx, &mut v, &snap, running);
    if readiness(app, ui, &ctx, &snap) {
        report(app, agent::open_in_terminal(app, &ctx).map(drop));
    }
    hand_off(app, ui, &ctx);
    // the agent crate's messages are English; ours show translated (strings::agent, VI4)
    let status = v.status.as_deref().map(super::strings::agent);
    super::toast::status_line(app, ui, "bashcut.agent.status", status.as_deref(), t.danger);
    if let Some(g) = v.gate.clone() {
        gate_card(app, ui, &ctx, &mut v, &g, running);
    }

    let height = (ui.available_height() - INPUT_HEIGHT).max(80.0);
    let area = egui::ScrollArea::vertical().id_salt("bashcut.agent.transcript").max_height(height).auto_shrink([false, false]).stick_to_bottom(true);
    let out = scroll_clear_of_bar(ui, area, egui::Id::new("bashcut.agent.transcript.bar"), |ui| {
        // before anything can widen the content: a line wider than the area (a long tool title,
        // an unbreakable word) moves egui's right edge with it, and a ✕ aligned to it (FX25)
        let width = ui.available_width();
        if snap.items.is_empty() {
            note(ui, f("Ask {agent} to edit this project.", &[("agent", &snap.agent.1)]), NOTE, t.text_dim);
            if let Some(prompt) = examples(app, ui) {
                v.input = prompt;
                ctx.memory_mut(|m| m.request_focus(input_id()));
            }
            note(ui, s("The agent stops at each stage for your approval."), NOTE, t.text_dim);
        }
        transcript(app, ui, &snap.items);
        for (i, p) in snap.permissions.iter().enumerate() {
            permission_card(app, ui, &ctx, &mut v, i, p, &snap.agent.1);
        }
        for (i, q) in snap.queued.iter().enumerate() {
            ui.add_space(6.0);
            let id = format!("bashcut.agent.queued.{i}");
            row_within(ui, width, |ui| {
                strong_note(ui, f("You · queued {n}", &[("n", &(i + 1).to_string())]), META, t.text_dim);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if q.cancelling {
                        note(ui, s("Removing…"), META, t.text_dim);
                        return;
                    }
                    if close_button(app, ui, &format!("{id}.cancel"), s("Remove this message from the queue")) {
                        call(app, &ctx, &mut v, "cancel", json!({"index": i}));
                    }
                });
            });
            let r = ui.add(egui::Label::new(egui::RichText::new(&q.text).color(t.text_dim)).selectable(true).wrap());
            app.auto.add_response(&id, &r, &q.text);
        }
        if running {
            ui.horizontal(|ui| {
                ui.spinner();
                let label = if snap.status == Status::Stopping { s("Stopping…") } else { s("Working…") };
                note(ui, label, NOTE, t.text_dim);
            });
            // a waiting permission card is the reason for the silence
            if quiet && snap.status == Status::Running && snap.permissions.is_empty() {
                let line = s(quiet_line());
                let r = note(ui, line, NOTE, t.text);
                app.auto.add_response("bashcut.agent.quiet", &r, line);
            }
        }
    });
    app.auto.add("bashcut.agent.transcript", out.inner_rect, &snap.title);

    ui.separator();
    // a live agent takes messages while it works; a stopping one is about to exit
    let busy = running && (!snap.live || snap.status == Status::Stopping);
    let slash = snap.slash.then_some(snap.commands.as_deref());
    input(app, ui, &ctx, &mut v, busy, running, snap.missing.as_deref(), slash);
    if running {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    super::shared::put_temp(&ctx, key(), v);
}

/// A left-to-right row exactly `width` wide from the left edge, so its right-aligned part (a ✕)
/// stays inside even when an earlier line widened `ui` past its area (FX25).
fn row_within(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let at = egui::pos2(ui.max_rect().left(), ui.cursor().top());
    let rect = egui::Rect::from_min_size(at, egui::vec2(width.max(0.0), ui.spacing().interact_size.y));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), add);
}

/// The hints after [`QUIET_HINT`] with no output, macOS first (the strings test reads both).
pub(super) const QUIET_LINES: [&str; 2] = [
    "No output for 30 s: check for a macOS permission prompt (it can be behind this window).",
    "No output for 30 s: the agent may be waiting for a sign-in or a prompt outside BashCutPro.",
];

/// The hint after [`QUIET_HINT`] with no output: where a blocked agent usually waits.
fn quiet_line() -> &'static str {
    if cfg!(target_os = "macos") { QUIET_LINES[0] } else { QUIET_LINES[1] }
}

/// The readiness line when Claude Code refused a flag of the live session (too old).
pub(super) const OUTDATED_LINE: &str = "Update Claude Code: run `claude update`";

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

/// Open in Terminal waits until the in-app agent of this conversation has ended (stopping its turn,
/// closing its process; [`agent::open_in_terminal`]): a line says so meanwhile.
fn hand_off(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    if let Some(e) = agent::take_hand_off_error(ctx) {
        app.ui.status = e;
    }
    if !agent::handing_off(ctx) {
        return;
    }
    let line = s("Ending the in-app session before the terminal opens…");
    let r = note(ui, line, NOTE, app.tokens.text_dim);
    app.auto.add_response("bashcut.agent.handoff", &r, line);
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}

/// One line per missing item, before the user presses Send; `true` when Open in Terminal was clicked.
fn readiness(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, snap: &Snapshot) -> bool {
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
        let line = s(OUTDATED_LINE);
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
    handoff
}

/// "Claude Code is not signed in" or "… account problem: billing" (AG6).
fn auth_headline(agent: &str, category: &str) -> String {
    let key = match category {
        bashcut_agent::stream_json::NOT_SIGNED_IN => "{agent} is not signed in",
        "billing_error" => "{agent} account problem: billing",
        "oauth_org_not_allowed" => "{agent} account problem: this organization is not allowed",
        _ => "{agent} account problem: {category}",
    };
    f(key, &[("agent", agent), ("category", category)])
}

/// The sign-in readiness line: headline, Sign in (Claude Code), and the agent's message, selectable.
fn auth_line(app: &mut FilmcraftApp, ui: &mut egui::Ui, snap: &Snapshot, category: &str, message: &str) {
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

/// The agent kit's install commands, as the agent-kit README gives them (shown selectable).
fn kit_commands() -> String {
    format!("claude plugin marketplace add {}\nclaude plugin install {}", terminal::KIT_MARKETPLACE, terminal::KIT_INSTALL)
}

/// "BashCutPro skills are not installed" (AG9): Install kit…, and the commands to run by hand.
fn kit_line(app: &mut FilmcraftApp, ui: &mut egui::Ui, snap: &Snapshot) {
    let t = app.tokens;
    ui.horizontal_wrapped(|ui| {
        let line = s("BashCutPro skills are not installed");
        let r = note(ui, line, NOTE, t.danger).on_hover_text(s("The BashCutPro kit (bcp plugin for Claude Code) brings the /bcp: recipes and skills."));
        app.auto.add_response("bashcut.agent.ready.kit", &r, line);
        let program = snap.resume.as_ref().and_then(|a| a.first()).cloned();
        if let Some(program) = program
            && hover_button(
                app,
                ui,
                "bashcut.agent.installKit",
                s("Install kit…"),
                s("Open a terminal that installs the BashCutPro kit (bcp plugin) for Claude Code."),
            )
        {
            report(app, agent::install_kit(&program, &snap.cwd()));
        }
    });
    let cmds = kit_commands();
    let r = ui.add(egui::Label::new(code_text(&cmds, NOTE, t.text_dim)).selectable(true).wrap());
    app.auto.add_response("bashcut.agent.ready.kit.commands", &r, &cmds);
}

fn hover_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, id: &str, label: &str, hover: &str) -> bool {
    let r = ui.add(egui::Button::new(label).small()).on_hover_text(hover);
    app.auto.add_response(id, &r, label);
    r.clicked()
}

/// Open in Terminal and Sign in, for an agent whose program was found (Claude Code, Codex); `true`
/// when Open in Terminal was clicked.
fn terminal_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, snap: &Snapshot) -> bool {
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

/// A terminal action's error in the status bar.
fn report(app: &mut FilmcraftApp, r: Result<(), String>) {
    if let Err(e) = r {
        app.ui.status = e;
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

/// A permission request waiting for the user (AG8): what the agent wants to run, and the answers.
fn permission_card(
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
fn permission_line(tool: &str, answer: &str) -> String {
    let key = match answer {
        "allowed" => "{tool}: allowed once",
        "always" => "{tool}: allowed for this project",
        "denied" => "{tool}: denied",
        _ => "{tool}: not answered (the agent stopped waiting)",
    };
    f(key, &[("tool", tool)])
}

/// The transcript; consecutive tool calls with the same key fold into one row.
/// Kit recipes ([`super::palette::RECIPES`]) offered as example prompts on an empty transcript.
const EXAMPLES: &[&str] = &["vlog-plan", "vlog-hook-script", "vlog-publish"];

/// One button per [`EXAMPLES`] recipe; the clicked one's prompt fills the message box (not sent).
fn examples(app: &mut FilmcraftApp, ui: &mut egui::Ui) -> Option<String> {
    let mut picked = None;
    ui.add_space(4.0);
    for (id, name, _) in super::palette::RECIPES.iter().filter(|(id, ..)| EXAMPLES.contains(id)) {
        let r = ui.button(s(name)).on_hover_text(s("Needs the BashCutPro kit (bcp plugin) for Claude Code."));
        app.auto.add_response(&format!("bashcut.agent.example.{id}"), &r, s(name));
        if r.clicked() {
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
                strong_note(ui, s("You"), META, t.text_dim);
                ui.add(egui::Label::new(egui::RichText::new(text).color(t.text)).selectable(true).wrap());
            }
            Item::Assistant { text } => {
                ui.add_space(6.0);
                strong_note(ui, s("Agent"), META, t.text_dim);
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
                super::widgets::note(ui, line, NOTE, t.accent);
            }
            Item::Error { message } => {
                note(ui, super::strings::agent(message), NOTE, t.danger);
            }
            Item::Notice { message } => {
                note(ui, super::strings::agent(message), META, t.text_dim);
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
        }
        i += 1;
    }
}

fn tool_group(app: &mut FilmcraftApp, ui: &mut egui::Ui, at: usize, key: &str, group: &[Item]) {
    let t = app.tokens;
    let failed = group.iter().any(|g| matches!(g, Item::Tool { is_error: true, .. }));
    let waiting = group.iter().any(|g| matches!(g, Item::Tool { output: None, .. }));
    let mark = if failed {
        "×"
    } else if waiting {
        "…"
    } else {
        "•"
    };
    let title = if group.len() > 1 { format!("{mark} {key} ×{}", group.len()) } else { format!("{mark} {key}") };
    let head = egui::CollapsingHeader::new(code_text(&title, META, t.text_dim)).id_salt(("bashcut.agent.tool", at)).show(ui, |ui| {
        for g in group {
            let Item::Tool { input, output, .. } = g else { continue };
            code_note(ui, bashcut_agent::clip(&input.to_string(), 2000), CODE, t.text_dim);
            if let Some(o) = output {
                code_note(ui, bashcut_agent::clip(o, 4000), CODE, t.text);
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
fn caret(ctx: &egui::Context, text: &str) -> usize {
    let chars = egui::text_edit::TextEditState::load(ctx, input_id()).and_then(|st| st.cursor.char_range()).map(|r| usize::from(r.primary.index));
    chars.map_or(text.len(), |n| text.char_indices().nth(n).map_or(text.len(), |(i, _)| i))
}

/// Replace the `/` word `start..caret` with `/name ` and put the caret after it (AG12).
fn insert_slash(ctx: &egui::Context, v: &mut View, start: usize, caret: usize, name: &str) {
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
fn input(
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
fn slash_list(
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
        for k in [
            "Control channel off: the agent cannot edit the open project.",
            "Project not saved: conversations will not be kept",
            "Save…",
            OUTDATED_LINE,
            "You · queued {n}",
            "Remove this message from the queue",
            "Removing…",
            "{agent} wants to use {tool}",
            "Allow once",
            "Always for this project",
            "Deny",
            "Start a message to load skills",
            "skill",
            "command",
            "↑/↓ to pick, Enter or Tab to insert, Esc to close",
        ] {
            assert_ne!(s(k), k);
        }
        set_language(None);
    }

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
