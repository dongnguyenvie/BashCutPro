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
//! AG9/AG16: when no kit folder is found and the live session's `system/init` lists no `bcp` plugin, a
//! readiness line says the BashCutPro skills are not downloaded, with Download kit (a terminal the user
//! starts that clones the kit into the data folder; never automatic; without git the zip URL instead) and
//! the optional global install command, and Open in Terminal passes the app's MCP server instead.
//!
//! AG12: typing a `/` word in the message box lists the live session's slash commands and skills
//! (from its `system/init`, [`bashcut_agent::stream_json::SlashCommand`]) above the box: prefix
//! matches first, ↑/↓ pick, Enter/Tab insert `/name `, Esc closes, a click inserts too (rows
//! `bashcut.agent.slash.<i>`). Before the first init it says a message loads them; Codex has no list.
//!
//! AG8: a tool call Claude Code needs approval for shows as a card at the end of the transcript (tool,
//! the command or file): Allow once / Always for this project / Deny (`bashcut.agent.permission.<n>.*`,
//! `bashcut.agent.permission`); the answer stays in the transcript as one line.

mod cards;
mod empty;
mod input;
mod reveal;
mod setup;
mod tool_summary;
mod tools;
mod transcript;
mod working;

use bashcut_agent::stream_json::SlashCommand;
use bashcut_agent::{Gate, Item, Status};
use serde_json::{Value, json};

use super::agent;
use super::hints;
use super::strings::{f, s};
use super::widgets::{META, NOTE, close_button, note, scroll_clear_of_bar, strong_note};
use crate::FilmcraftApp;

pub use setup::sign_in_button;

/// The pending gate is re-read from `plan.json` this often (seconds), not every frame.
const GATE_EVERY: f64 = 1.0;
/// After this long (seconds) with no event from a running agent, a hint says where it may be
/// stuck (AG1: a macOS permission prompt holds `claude` before its first line). Not a stop.
const QUIET_HINT: f64 = 30.0;
/// Scrolled up further than this (px) from the bottom, new items show the jump pill.
const JUMP_AWAY: f32 = 40.0;
/// Height kept for the composer before its first frame.
const INPUT_HEIGHT: f32 = 60.0;

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
    /// When the running turn began (egui time), for the working row's elapsed time.
    turn_start: Option<f64>,
    /// The `/` list (AG12): the picked row, the word it was picked for, the word Esc closed it on,
    /// and where it was drawn last frame (a click on it keeps it open while the box loses focus).
    slash_sel: usize,
    slash_word: String,
    slash_closed: Option<String>,
    slash_rect: Option<egui::Rect>,
    /// Jump to latest (§3.10): the transcript's content height last frame, whether content arrived
    /// while it was scrolled up, and a click waiting to scroll to the bottom.
    content_h: f32,
    new_below: bool,
    jump: bool,
    /// The composer height last frame: the transcript gets the rest.
    composer_h: f32,
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
    /// [`agent::kit_gap`]: commands the kit names that this build lacks, and the skills using them.
    kit_gap: Option<(usize, Vec<String>)>,
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
            kit_gap: agent::kit_gap(&ctx),
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
    v.turn_start = if running { v.turn_start.or(Some(now)) } else { None };
    let quiet = v.heard.is_some_and(|(_, at)| now - at >= QUIET_HINT);

    setup::header(app, ui, &ctx, &mut v, &snap, running);
    if setup::readiness(app, ui, &ctx, &snap) {
        report(app, agent::open_in_terminal(app, &ctx).map(drop));
    }
    hand_off(app, ui, &ctx);
    // the agent crate's messages are English; ours show translated (strings::agent, VI4)
    let status = v.status.as_deref().map(super::strings::agent);
    super::toast::status_line(app, ui, "bashcut.agent.status", status.as_deref(), t.danger);
    if let Some(g) = v.gate.clone() {
        cards::gate_card(app, ui, &ctx, &mut v, &g, running);
    }

    let composer = if v.composer_h > 0.0 { v.composer_h } else { INPUT_HEIGHT };
    let height = (ui.available_height() - composer - ui.spacing().item_spacing.y * 2.0 - 4.0).max(80.0);
    let area = egui::ScrollArea::vertical().id_salt("bashcut.agent.transcript").max_height(height).auto_shrink([false, false]).stick_to_bottom(true);
    let out = scroll_clear_of_bar(ui, area, egui::Id::new("bashcut.agent.transcript.bar"), |ui| {
        // before anything can widen the content: a line wider than the area (a long tool title,
        // an unbreakable word) moves egui's right edge with it, and a ✕ aligned to it (FX25)
        let width = ui.available_width();
        if snap.items.is_empty()
            && let Some(prompt) = empty::empty(app, ui, &snap.agent.1)
        {
            v.input = prompt;
            ctx.memory_mut(|m| m.request_focus(input_id()));
        }
        transcript::transcript(app, ui, &snap.items, &snap.agent.1);
        for (i, p) in snap.permissions.iter().enumerate() {
            cards::permission_card(app, ui, &ctx, &mut v, i, p, &snap.agent.1);
        }
        if running {
            let hint = (quiet && snap.status == Status::Running && snap.permissions.is_empty()).then(quiet_line);
            working::row(app, ui, &snap.items, &snap.status, v.turn_start.unwrap_or(now), hint);
        }
        for (i, q) in snap.queued.iter().enumerate() {
            ui.add_space(6.0);
            let id = format!("bashcut.agent.queued.{i}");
            row_within(ui, width, |ui| {
                strong_note(ui, f("Queued {n}", &[("n", &(i + 1).to_string())]), META, t.text_dim);
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
            let r =
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| ui.scope(|ui| transcript::user_bubble(app, ui, &q.text, true)).inner).inner;
            app.auto.add_response(&id, &r, &q.text);
        }
        if std::mem::take(&mut v.jump) {
            ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
        }
    });
    app.auto.add("bashcut.agent.transcript", out.inner_rect, &snap.title);
    jump_pill(app, &ctx, &mut v, &out);

    ui.separator();
    // a live agent takes messages while it works; a stopping one is about to exit
    let busy = running && (!snap.live || snap.status == Status::Stopping);
    let slash = snap.slash.then_some(snap.commands.as_deref());
    input::input(app, ui, &ctx, &mut v, busy, running, snap.missing.as_deref(), slash, &snap.agent.1, &c.line);
    super::shared::put_temp(&ctx, key(), v);
}

/// "↓ New messages" over the bottom of the transcript (§3.10): shown when it is scrolled up more than
/// [`JUMP_AWAY`] and content arrived since; a click scrolls to the bottom (and sticks there again).
fn jump_pill<R>(app: &mut FilmcraftApp, ctx: &egui::Context, v: &mut View, out: &egui::scroll_area::ScrollAreaOutput<R>) {
    let t = app.tokens;
    let away = out.content_size.y - out.inner_rect.height() - out.state.offset.y;
    if away <= JUMP_AWAY {
        v.new_below = false;
    } else if out.content_size.y > v.content_h + 0.5 {
        v.new_below = true;
    }
    v.content_h = out.content_size.y;
    let show = v.new_below && !v.jump;
    let a = if super::motion::on(ctx) { ctx.animate_bool_with_time(egui::Id::new("bashcut.agent.jump.fade"), show, 0.12) } else { f32::from(u8::from(show)) };
    if a <= 0.0 {
        return;
    }
    let at = egui::pos2(out.inner_rect.center().x, out.inner_rect.bottom() - 8.0);
    let label = s("New messages");
    let clicked = egui::Area::new(egui::Id::new("bashcut.agent.jump.area"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .show(ctx, |ui| {
            ui.multiply_opacity(a);
            egui::Frame::new()
                .fill(t.field_bg)
                .stroke(egui::Stroke::new(1.0, t.field_border))
                .corner_radius(t.radius)
                .inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        crate::icons::paint(ui.painter(), rect, crate::icons::Icon::ChevronDown, t.text);
                        ui.add(egui::Label::new(super::widgets::note_text(label, NOTE, t.text)).selectable(false));
                    });
                })
                .response
                .interact(egui::Sense::click())
        });
    let r = clicked.inner;
    app.auto.add_response("bashcut.agent.jump", &r, label);
    if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        v.jump = true;
        v.new_below = false;
        ctx.request_repaint();
    }
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

/// A terminal action's error in the status bar.
fn report(app: &mut FilmcraftApp, r: Result<(), String>) {
    if let Err(e) = r {
        app.ui.status = e;
    }
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
            "Queued {n}",
            "New messages",
            "Remove this message from the queue",
            "Removing…",
            "{agent} wants to use {tool}",
            "Failed",
            "Stopped",
            "Copy",
            "Copied",
            "Show all",
            "Show",
            "Working",
            "Thinking…",
            "Running {tool}…",
            "Writing…",
            "The turn ended with an error.",
            "The turn stopped at its turn limit.",
            "The turn stopped at its spending limit.",
            "1 step",
            "{n} steps",
            "{n} s",
            "{m} min {s} s",
            "Allow once",
            "Always for this project",
            "Deny",
            "Copy",
            "Copied",
            "Start a message to load skills",
            "skill",
            "command",
            "↑/↓ to pick, Enter or Tab to insert, Esc to close",
            "More",
            "{n} things to set up",
            "Message {agent}…",
            "Send (Enter sends, Shift+Enter for a new line)",
            "No sequence open",
        ] {
            assert_ne!(s(k), k);
        }
        set_language(None);
    }
}
