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

use bashcut_agent::{Gate, Item, Status};
use serde_json::{Value, json};

use super::agent;
use super::hints::{self, HOW_TO_TURN_ON};
use super::strings::{f, s};
use super::terminal::{self, Job};
use super::widgets::{CODE, META, NOTE, button, code_note, code_text, note, strong_note};
use crate::FilmcraftApp;

/// The pending gate is re-read from `plan.json` this often (seconds), not every frame.
const GATE_EVERY: f64 = 1.0;
/// After this long (seconds) with no event from a running agent, a hint says where it may be
/// stuck (AG1: a macOS permission prompt holds `claude` before its first line). Not a stop.
const QUIET_HINT: f64 = 30.0;
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
    /// While a turn runs: the agent's event count and when it last changed (egui time).
    heard: Option<(u64, f64)>,
    /// Open in Terminal was clicked: the terminal's command and folder, opened once the in-app
    /// agent has ended (AG3: one writer per session).
    handoff: Option<(Vec<String>, std::path::PathBuf)>,
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
    /// Claude Code's last `system/init` listed no [`terminal::KIT_PLUGIN`] (unknown before one: `false`).
    kit_missing: bool,
    /// The engine and bridge address, for the terminal's MCP config when the kit is missing.
    mcp: Option<(String, String)>,
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
            resume: (matches!(a.agent.id.as_str(), "claude-code" | "codex") && hints::agent_found(&a.agent))
                .then(|| a.agent.argv.first().map(|p| terminal::resume_argv(p, &a.agent.resume_args, &a.conversation.session)))
                .flatten(),
            project: c.project.clone(),
            live: a.is_live(),
            queued: a.queued_items(),
            outdated: a.outdated(),
            auth: a.conversation.transcript.auth_problem().map(|(c, m)| (c.to_string(), m.to_string())),
            kit_missing: a.agent.id == "claude-code" && a.plugins.as_ref().is_some_and(|p| !p.iter().any(|n| n == terminal::KIT_PLUGIN)),
            mcp: c.cli.clone().zip(c.bridge.clone()),
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
    if let Some(job) = readiness(app, ui, &ctx, &snap) {
        v.handoff = Some(job);
    }
    hand_off(app, ui, &ctx, &mut v);
    // the agent crate's messages are English; those with a catalog row show translated
    super::toast::status_line(app, ui, "bashcut.agent.status", v.status.as_deref().map(s), t.danger);
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
            for (i, q) in snap.queued.iter().enumerate() {
                ui.add_space(6.0);
                let id = format!("bashcut.agent.queued.{i}");
                ui.horizontal(|ui| {
                    strong_note(ui, f("You · queued {n}", &[("n", &(i + 1).to_string())]), META, t.text_dim);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if q.cancelling {
                            note(ui, s("Removing…"), META, t.text_dim);
                            return;
                        }
                        let r = ui.add(egui::Button::new("✕").small().frame(false)).on_hover_text(s("Remove this message from the queue"));
                        app.auto.add_response(&format!("{id}.cancel"), &r, "✕");
                        if r.clicked() {
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
                if quiet && snap.status == Status::Running {
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
    input(app, ui, &ctx, &mut v, busy, running, snap.missing.as_deref());
    if running {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    super::shared::put_temp(&ctx, key(), v);
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

/// Open in Terminal, once the in-app agent of this conversation has ended (stopping its turn, closing
/// its process): the terminal then resumes the session alone.
fn hand_off(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View) {
    let Some((argv, cwd)) = v.handoff.clone() else { return };
    if agent::with(app, ctx, bashcut_agent::Agent::release) {
        v.handoff = None;
        agent::with(app, ctx, bashcut_agent::Agent::continued_in_terminal);
        open_terminal(app, &cwd, &Job::Program(argv), None);
        return;
    }
    let line = s("Ending the in-app session before the terminal opens…");
    let r = note(ui, line, NOTE, app.tokens.text_dim);
    app.auto.add_response("bashcut.agent.handoff", &r, line);
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}

/// One line per missing item, before the user presses Send; the Open in Terminal job when clicked.
fn readiness(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, snap: &Snapshot) -> Option<(Vec<String>, std::path::PathBuf)> {
    let t = app.tokens;
    if let Some(line) = &snap.missing {
        ui.horizontal_wrapped(|ui| {
            let r = note(ui, line, NOTE, t.danger);
            app.auto.add_response("bashcut.agent.ready.agent", &r, line);
            if snap.agent.0 == "claude-code"
                && hover_button(app, ui, "bashcut.agent.install", s("Install"), s("Open a terminal with the official Claude Code installer."))
            {
                let hint = s("Installing Claude Code with the official installer. When it finishes, run claude and type /login, then go back to BashCutPro.");
                open_terminal(app, &snap.cwd(), &terminal::install_job(terminal::Os::current()), Some(hint));
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
            let hint = s("Installing the BashCutPro kit for Claude Code. When it finishes, close this window and start a New conversation in BashCutPro.");
            open_terminal(app, &snap.cwd(), &terminal::kit_job(terminal::Os::current(), &program), Some(hint));
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

/// Open in Terminal and Sign in, for an agent whose program was found (Claude Code, Codex); the
/// terminal's command and folder when Open in Terminal was clicked ([`hand_off`] opens it).
fn terminal_row(app: &mut FilmcraftApp, ui: &mut egui::Ui, snap: &Snapshot) -> Option<(Vec<String>, std::path::PathBuf)> {
    let argv = snap.resume.as_ref()?;
    ui.horizontal_wrapped(|ui| {
        let tip = f("Continue this conversation with {agent} in a terminal window.", &[("agent", &snap.agent.1)]);
        let clicked = hover_button(app, ui, "bashcut.agent.terminal", s("Open in Terminal"), &tip);
        // the sign-in line has its own Sign in (AG6)
        if snap.agent.0 == "claude-code" && snap.auth.is_none() {
            let program = argv.first().cloned().unwrap_or_default();
            sign_in_button(app, ui, &program, snap.project.as_deref());
        }
        clicked.then(|| (terminal_argv(argv, snap), snap.cwd()))
    })
    .inner
}

/// `argv` plus, while the kit is missing (no `bashcutpro` server in the terminal), the app's own (AG9).
fn terminal_argv(argv: &[String], snap: &Snapshot) -> Vec<String> {
    let mut argv = argv.to_vec();
    if snap.kit_missing
        && let Some((cli, bridge)) = &snap.mcp
    {
        match terminal::terminal_mcp_config(cli, bridge) {
            Ok(path) => argv.extend(["--mcp-config".to_string(), path.to_string_lossy().into_owned()]),
            Err(e) => log::warn!("bashcut terminal MCP config: {e}"),
        }
    }
    argv
}

/// Sign in: `claude` in a terminal with a hint to type `/login` (AG6 shows it when auth fails).
/// The login happens in Claude Code's own flow; the app never reads or stores tokens.
pub fn sign_in_button(app: &mut FilmcraftApp, ui: &mut egui::Ui, program: &str, project: Option<&std::path::Path>) {
    let tip = s("Open Claude Code in a terminal to sign in (type /login). BashCutPro never sees your login.");
    if hover_button(app, ui, "bashcut.agent.signIn", s("Sign in…"), tip) {
        let hint = s("Type /login and follow the steps in your browser, then close this window.");
        let cwd = bashcut_agent::session::turn_dir(project);
        open_terminal(app, &cwd, &Job::Program(vec![program.to_string()]), Some(hint));
    }
}

fn open_terminal(app: &mut FilmcraftApp, cwd: &std::path::Path, job: &Job, hint: Option<&str>) {
    if let Err(e) = terminal::open(cwd, job, hint) {
        app.ui.status = f("Could not open a terminal: {error}", &[("error", &e)]);
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
                note(ui, s(message), NOTE, t.danger);
            }
            Item::Notice { message } => {
                note(ui, s(message), META, t.text_dim);
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
        "✗"
    } else if waiting {
        "…"
    } else {
        "✓"
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

/// The message box; `busy`: Send is off (a turn of a per-message agent, or a stopping one), `running`:
/// Stop is on.
fn input(app: &mut FilmcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, v: &mut View, busy: bool, running: bool, missing: Option<&str>) {
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
        ] {
            assert_ne!(s(k), k);
        }
        set_language(None);
    }
}
