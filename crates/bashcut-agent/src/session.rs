//! The agent the app keeps: the chosen agent, the open conversation, its agent process, and the
//! project's gates. The UI calls [`Agent::poll`] each frame; [`Agent::command`] serves the
//! `bashcut.agent.*` command ids, so the panel, shortcuts, the control channel and MCP share it.
//!
//! A live agent (Claude Code, AG2) keeps one process per conversation: a message sent while it
//! works is written at once and shown as queued until the agent takes it; the turn ends at its
//! `result`. The process is closed when the conversation, agent or project changes, after
//! [`IDLE_CLOSE`] idle, when the panel is hidden while idle, and at app exit; the next message starts
//! it again with `--resume`. Codex keeps one process per message.
//!
//! Stop (AG3) asks a live agent that announces `interrupt_receipt_v1` to end its turn with the
//! stream-json interrupt request and keeps the process for the next message; without the
//! capability, or when the agent does not answer in time, it gets SIGINT (the process ends, the next
//! message resumes).
//!
//! Queued messages (AG4) each have ✕: an agent that announces `interrupt_cancel_queued_v1` gets every
//! message with a `uuid` and ✕ withdraws a written one with `cancel_async_message` (dropped when the
//! agent confirms; one it took already stays). Any other agent gets a queued message only when the
//! turn before it ends (it is held in the app, so ✕ just drops it, and each runs as its own turn).
//!
//! Permission requests (AG8): a live agent asks before a tool call that is not pre-allowed
//! ([`Event::Permission`]); the request waits in [`Agent::permissions`] until the user answers it
//! ([`Agent::answer_permission`]: allow once, always for this project, deny). Stop, the end of the
//! turn or of the process deny or drop what still waits, so nothing hangs on an answer.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use crate::plan::{self, Gate};
use crate::process::{AgentSpec, Live, Turn, TurnEnd, TurnEvent, mcp_config_path, write_mcp_config};
use crate::store::{self, Conversation};
use crate::stream_json::{self, Answer, Event, Permission, SlashCommand};
use crate::transcript::Item;
use crate::{Error, Result, err};

/// Longest message sent.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// A live agent idle this long is closed (its next message resumes the session in a new process).
pub const IDLE_CLOSE: Duration = Duration::from_secs(30 * 60);
/// At app exit, how long to wait for a live agent to end after its stdin closed.
const EXIT_WAIT: Duration = Duration::from_secs(3);
/// After the interrupt request, how long the agent has to acknowledge it, then SIGINT.
const ACK_WAIT: Duration = Duration::from_secs(3);
/// After the acknowledgement, how long it has to end the turn with its `result`, then SIGINT.
const RESULT_WAIT: Duration = Duration::from_secs(5);
/// `system/init` capabilities: the interrupt control request is answered (AG3), and it can also
/// drop messages written but not taken yet (`cancel_queued`).
pub const INTERRUPT_RECEIPT: &str = "interrupt_receipt_v1";
pub const INTERRUPT_CANCEL_QUEUED: &str = "interrupt_cancel_queued_v1";
/// Permission requests kept waiting at once; more are denied at once.
const MAX_PERMISSIONS: usize = 16;
/// What a denied tool call tells the agent.
const DENIED: &str = "The user denied this in BashCutPro.";
/// What a request denied because the turn stopped tells the agent.
const STOPPED: &str = "The user stopped the turn in BashCutPro.";

/// An interrupt request written to the live agent and not finished yet.
struct Interrupt {
    id: String,
    sent: Instant,
    acked: bool,
}

/// A message for the live agent not taken yet.
struct Pending {
    /// As written (with the context line), to write again after a restart.
    message: String,
    /// What the user typed, when it was sent while the agent worked (shown as queued, added to the
    /// transcript when the agent takes it); `None`: already in the transcript.
    queued: Option<String>,
    /// The `uuid` it is written with ([`INTERRUPT_CANCEL_QUEUED`]), for ✕; empty: written without one.
    uuid: String,
    /// Kept in the app, not written yet (AG4): written when the agent has nothing else to answer.
    held: bool,
    /// The id of the `cancel_async_message` request in flight for it.
    cancelling: Option<String>,
}

/// One queued message as the panel shows it, oldest first.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Queued {
    pub text: String,
    /// ✕ was pressed and the agent has not answered yet.
    pub cancelling: bool,
}

/// Where and how the live agent runs, to restart it.
#[derive(Clone, PartialEq)]
struct Launch {
    cwd: PathBuf,
    env: Vec<(String, String)>,
    /// The MCP config it was given: another one (new engine or bridge) restarts an idle agent.
    mcp: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Status {
    Idle,
    Running,
    Stopping,
    /// The last turn failed (the transcript has the details).
    Failed {
        message: String,
    },
}

/// What a turn is sent with, from the app.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// The open project file (`None`: unsaved; conversations are then not kept).
    pub project: Option<PathBuf>,
    /// One line about the project, sequence, selection and playhead, sent before the message.
    pub line: String,
    /// The engine next to the app (`bashcutpro-cli`, else `bcpro` or `filmcraft-cli`).
    pub cli: Option<String>,
    /// `127.0.0.1:<control port>` when the app serves one.
    pub bridge: Option<String>,
    /// The control channel's session token, only when `bridge` is the app's own; the agent gets it as
    /// `FILMCRAFT_CONTROL_TOKEN` (SY7). Never written to the MCP config.
    pub control_token: Option<String>,
}

pub struct Agent {
    /// The agents to choose from (Claude Code first, then agent plugins).
    pub agents: Vec<AgentSpec>,
    pub agent: AgentSpec,
    pub conversation: Conversation,
    pub status: Status,
    /// The sidecar of the project the conversation belongs to.
    sidecar: Option<PathBuf>,
    turn: Option<Turn>,
    mcp_config: Option<PathBuf>,
    /// Events received from the agent so far (all turns): the panel notes when it last changed
    /// and hints at a blocked agent after a long silence (AG1).
    events: u64,
    /// The live agent's process, kept between messages.
    live: Option<Live>,
    launch: Option<Launch>,
    /// Messages for the live agent not taken yet, oldest first (held ones after the written ones).
    pending: VecDeque<Pending>,
    /// The live agent took a message and has not sent its `result` yet.
    in_turn: bool,
    /// When the current turn started (the timeout runs from here).
    busy_since: Option<Instant>,
    /// When the live agent became idle ([`IDLE_CLOSE`]).
    idle_since: Option<Instant>,
    timed_out: bool,
    /// Automatic restarts since the agent last answered (at most one: an agent that exits before
    /// reading its message is started again with `--resume` and the message written again).
    restarts: u8,
    /// Queued messages the agent never read (it failed or was stopped), for the message box.
    unsent: Vec<String>,
    /// `capabilities` of the live agent's last `system/init` (AG3/AG4 feature-detect from them).
    pub capabilities: Vec<String>,
    /// Plugin names of the live agent's last `system/init` (`None` before one arrived; AG9 reads `bcp`).
    pub plugins: Option<Vec<String>>,
    /// Slash commands of the agent's last `system/init` (AG12, the message box's `/` list; `None`
    /// before one arrived).
    pub commands: Option<Vec<SlashCommand>>,
    /// Claude Code refused one of our flags (`unknown option`): too old for a live session.
    outdated: bool,
    /// The Stop in flight as an interrupt request (AG3).
    interrupting: Option<Interrupt>,
    /// Control requests (interrupt, cancel) written so far (their ids).
    requests: u64,
    /// A live agent that was closed and has not exited yet (Open in Terminal waits for it).
    closing: Option<Live>,
    /// A client other than the panel (control channel, MCP, a plugin) drives the agent while the
    /// Agent section is hidden: the hidden panel does not close it after each turn (idle close still does).
    pub driven: bool,
    /// The agent reported a sign-in or account problem this turn (AG6): its process is not kept for
    /// the next message (the user may sign in meanwhile), and its exit adds no generic error.
    auth_failed: bool,
    /// Permission requests waiting for the user, oldest first (AG8).
    permissions: Vec<Permission>,
}

/// A waiting permission request as the panel and `bashcut.agent.state` show it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionView {
    pub tool: String,
    pub detail: String,
    /// "Always for this project" is offered: the agent suggested a rule and the project is saved.
    pub always: bool,
}

impl Agent {
    pub fn new(agents: Vec<AgentSpec>) -> Self {
        let agents = if agents.is_empty() { vec![AgentSpec::claude_code(None)] } else { agents };
        let agent = agents.first().cloned().unwrap_or_else(|| AgentSpec::claude_code(None));
        let conversation = Conversation::new(&agent.id);
        Agent {
            agents,
            agent,
            conversation,
            status: Status::Idle,
            sidecar: None,
            turn: None,
            mcp_config: None,
            events: 0,
            live: None,
            launch: None,
            pending: VecDeque::new(),
            in_turn: false,
            busy_since: None,
            idle_since: None,
            timed_out: false,
            restarts: 0,
            unsent: Vec::new(),
            capabilities: Vec::new(),
            plugins: None,
            commands: None,
            outdated: false,
            interrupting: None,
            requests: 0,
            closing: None,
            driven: false,
            auth_failed: false,
            permissions: Vec::new(),
        }
    }

    /// A turn runs (or a live agent has messages it has not answered).
    pub fn is_running(&self) -> bool {
        self.turn.is_some() || self.busy()
    }

    /// The live agent has a message it has not answered.
    fn busy(&self) -> bool {
        self.in_turn || !self.pending.is_empty()
    }

    /// The current agent keeps one process per conversation, so Send works while it runs.
    pub fn is_live(&self) -> bool {
        crate::format::find(&self.agent.events).is_ok_and(|f| f.live)
    }

    /// Messages sent while the agent worked that it has not taken yet, oldest first.
    pub fn queued(&self) -> Vec<String> {
        self.pending.iter().filter_map(|p| p.queued.clone()).collect()
    }

    /// [`Agent::queued`] with each one's ✕ state.
    pub fn queued_items(&self) -> Vec<Queued> {
        self.pending.iter().filter_map(|p| p.queued.as_ref().map(|text| Queued { text: text.clone(), cancelling: p.cancelling.is_some() })).collect()
    }

    /// The live agent can withdraw a written message (`cancel_async_message`, AG4).
    fn can_withdraw(&self) -> bool {
        self.capabilities.iter().any(|c| c == INTERRUPT_CANCEL_QUEUED)
    }

    /// ✕ on the queued message at `index` of [`Agent::queued`]: a held one is dropped at once; a
    /// written one is withdrawn with `cancel_async_message` and dropped when the agent confirms (one
    /// the agent took already stays and is answered).
    pub fn cancel(&mut self, index: usize) -> Result<()> {
        self.poll();
        let at = self.pending.iter().enumerate().filter(|(_, p)| p.queued.is_some()).nth(index).map(|(i, _)| i);
        let Some((i, p)) = at.and_then(|i| self.pending.get(i).map(|p| (i, p))) else { return err("that message is not queued any more") };
        if p.held {
            self.pending.remove(i);
            return Ok(());
        }
        if p.cancelling.is_some() {
            return Ok(());
        }
        if p.uuid.is_empty() {
            return err("the agent has this message already: stop it instead");
        }
        let Some(l) = &self.live else { return err("the agent has ended") };
        self.requests = self.requests.wrapping_add(1);
        let id = format!("bashcut-cancel-{}", self.requests);
        l.control(&stream_json::cancel_request(&id, &p.uuid))?;
        if let Some(p) = self.pending.get_mut(i) {
            p.cancelling = Some(id);
        }
        Ok(())
    }

    /// Queued messages the agent never read since the last call (for the message box).
    pub fn take_unsent(&mut self) -> Vec<String> {
        std::mem::take(&mut self.unsent)
    }

    /// Permission requests waiting for the user, oldest first.
    pub fn permissions(&self) -> Vec<PermissionView> {
        self.permissions
            .iter()
            .map(|p| PermissionView { tool: p.tool.clone(), detail: p.detail.clone(), always: !p.rules.is_empty() && self.sidecar.is_some() })
            .collect()
    }

    /// Answer the waiting permission request at `index` (AG8). [`Answer::Always`] needs a saved
    /// project (the rule goes into its folder's `.claude/settings.local.json`) and a suggested rule.
    pub fn answer_permission(&mut self, index: usize, answer: Answer) -> Result<()> {
        self.poll();
        let Some(p) = self.permissions.get(index) else { return err("that permission request is not waiting any more") };
        if answer == Answer::Always && (p.rules.is_empty() || self.sidecar.is_none()) {
            return err("this request cannot be allowed for the whole project: allow it once");
        }
        let Some(l) = &self.live else { return err("the agent has ended") };
        l.control(&stream_json::permission_response(p, answer, DENIED))?;
        let p = self.permissions.remove(index);
        let answer = match answer {
            Answer::Once => "allowed",
            Answer::Always => "always",
            Answer::Deny => "denied",
        };
        self.conversation.transcript.push(Item::Permission { tool: p.tool, detail: p.detail, answer: answer.into() });
        Ok(())
    }

    /// Deny every waiting permission request with `message` (Stop): the agent stops waiting.
    fn deny_permissions(&mut self, message: &str) {
        for p in std::mem::take(&mut self.permissions) {
            if let Some(l) = &self.live {
                let _ = l.control(&stream_json::permission_response(&p, Answer::Deny, message));
            }
            self.conversation.transcript.push(Item::Permission { tool: p.tool, detail: p.detail, answer: "denied".into() });
        }
    }

    /// Requests nobody can answer any more (the turn or the process ended) leave the panel.
    fn drop_permissions(&mut self) {
        for p in std::mem::take(&mut self.permissions) {
            self.conversation.transcript.push(Item::Permission { tool: p.tool, detail: p.detail, answer: "withdrawn".into() });
        }
    }

    /// Claude Code is too old for a live session (it refused one of our flags).
    pub fn outdated(&self) -> bool {
        self.outdated
    }

    /// Close the live agent if it is idle (panel hidden, project or conversation changed).
    pub fn end_session(&mut self) {
        if self.busy() {
            return;
        }
        if let Some(mut l) = self.live.take() {
            l.close();
            self.closing = Some(l);
        }
        self.idle_since = None;
        // the next process lists its own plugins and commands (AG15: no stale kit line from the ended one)
        self.plugins = None;
        self.commands = None;
    }

    /// Open in Terminal (AG3): the terminal resumes this conversation's session, which must not have
    /// two writers. Stops a running turn and closes the agent; `true` once no agent process of it
    /// runs (call again each frame until then).
    pub fn release(&mut self) -> bool {
        self.poll();
        if self.is_running() {
            if self.status != Status::Stopping {
                self.stop();
            }
            return false;
        }
        self.end_session();
        self.live.is_none() && self.closing.is_none()
    }

    /// The conversation went on in a terminal (after [`Agent::release`]).
    pub fn continued_in_terminal(&mut self) {
        if self.conversation.session.is_empty() {
            // nothing to continue: the terminal starts a new session
            return;
        }
        self.conversation.transcript.push(Item::Notice { message: "Continued in Terminal.".into() });
        self.save();
    }

    /// How many events the agent has sent so far; unchanged while it is silent.
    pub fn events_seen(&self) -> u64 {
        self.events
    }

    pub fn sidecar(&self) -> Option<&Path> {
        self.sidecar.as_deref()
    }

    /// Follow the open project: a different project saves this conversation and opens that
    /// project's latest one (or a new one).
    pub fn set_project(&mut self, project: Option<&Path>) {
        let side = project.map(crate::sidecar);
        if side == self.sidecar {
            return;
        }
        if self.is_running() {
            // never move a running turn's conversation; switch once it ends
            return;
        }
        self.save();
        self.end_session();
        self.sidecar = side;
        let latest = self.sidecar.as_deref().and_then(|s| store::list(s).first().and_then(|c| store::load(s, &c.id).ok()));
        self.conversation = latest.unwrap_or_else(|| Conversation::new(&self.agent.id));
        self.status = Status::Idle;
    }

    /// Use the agent `id` for the next conversation.
    pub fn use_agent(&mut self, id: &str) -> Result<()> {
        let a = self.agents.iter().find(|a| a.id == id).cloned().ok_or_else(|| Error(format!("no agent `{id}`")))?;
        if self.is_running() {
            return err("stop the running turn first");
        }
        if a.id != self.agent.id {
            self.agent = a;
            // the plugins were the other agent's (AG9's kit line); unknown until this one's init
            self.plugins = None;
            self.commands = None;
            self.new_conversation()?;
        }
        Ok(())
    }

    pub fn new_conversation(&mut self) -> Result<()> {
        if self.is_running() {
            return err("stop the running turn first");
        }
        self.save();
        self.end_session();
        self.conversation = Conversation::new(&self.agent.id);
        self.status = Status::Idle;
        Ok(())
    }

    pub fn open(&mut self, id: &str) -> Result<()> {
        if self.is_running() {
            return err("stop the running turn first");
        }
        let side = self.sidecar.clone().ok_or_else(|| Error("save the project first: conversations are kept next to it".into()))?;
        let c = store::load(&side, id)?;
        self.save();
        self.end_session();
        if let Some(a) = self.agents.iter().find(|a| a.id == c.agent) {
            self.agent = a.clone();
        }
        self.conversation = c;
        self.status = Status::Idle;
        Ok(())
    }

    pub fn conversations(&self) -> Vec<store::Summary> {
        self.sidecar.as_deref().map(store::list).unwrap_or_default()
    }

    /// Save the conversation next to the project (if it has one and anything was said).
    pub fn save(&mut self) {
        let Some(side) = &self.sidecar else { return };
        if self.conversation.transcript.items.is_empty() {
            return;
        }
        self.conversation.updated = store::now();
        if let Err(e) = store::save(side, &self.conversation) {
            log::warn!("bashcut agent: {e}");
        }
    }

    /// Send `text`: a live agent gets it at once (queued while it works), else a turn starts.
    pub fn send(&mut self, text: &str, ctx: &Context) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return err("nothing to send");
        }
        if text.len() > MAX_MESSAGE {
            return err("message too long");
        }
        // an end already waiting is taken first, so the message never goes to a process that exited
        self.poll();
        let live = self.is_live();
        if self.turn.is_some() || (!live && self.is_running()) {
            return err("the agent is still working: wait or stop it");
        }
        if self.status == Status::Stopping {
            return err("the agent is stopping: wait a moment");
        }
        self.set_project(ctx.project.as_deref());
        let argv = self.command_line(ctx)?;
        let message = if ctx.line.is_empty() { text.to_string() } else { format!("[{}]\n\n{text}", ctx.line) };
        let cwd = turn_dir(ctx.project.as_deref());
        let mut env = self.agent.env.clone();
        env.push(("BASHCUT_AGENT_HOST".into(), "1".into()));
        // the kit's MCP launcher steps aside when it sees this: the app passes its own server (AK3)
        env.push(("BASHCUT_IN_APP_AGENT".into(), "1".into()));
        if let Some(t) = ctx.control_token.as_deref().filter(|_| ctx.bridge.is_some()) {
            env.push(("FILMCRAFT_CONTROL_TOKEN".into(), t.to_string()));
        }
        if let Some(side) = ctx.project.as_deref().map(crate::sidecar) {
            env.push(("BASHCUT_PROJECT_SIDECAR".into(), side.to_string_lossy().into_owned()));
        }
        if live {
            let launch = Launch { cwd, env, mcp: self.mcp_config.clone() };
            return self.send_live(text, message, &argv, launch);
        }
        let turn = Turn::start(&argv, &cwd, &env, &message, &self.agent.events, Duration::from_secs(self.agent.timeout_secs.max(1)))?;
        self.conversation.transcript.push(Item::User { text: text.to_string() });
        self.turn = Some(turn);
        self.status = Status::Running;
        Ok(())
    }

    /// The next turn's command line (writes the MCP config the first time and whenever the engine or
    /// bridge changed; the stale file is removed).
    fn command_line(&mut self, ctx: &Context) -> Result<Vec<String>> {
        let mcp = match (&ctx.cli, &ctx.bridge) {
            _ if self.agent.mcp_args.is_empty() => None,
            (Some(cli), Some(bridge)) => Some(match &self.mcp_config {
                Some(p) if *p == mcp_config_path(cli, bridge) && p.is_file() => p.clone(),
                old => {
                    let path = write_mcp_config(cli, bridge)?;
                    if let Some(old) = old.as_ref().filter(|o| **o != path) {
                        let _ = std::fs::remove_file(old);
                    }
                    path
                }
            }),
            (None, _) => return err("the BashCutPro engine (bashcutpro-cli) was not found next to the app: install BashCutPro again"),
            (_, None) => return err("this agent needs the app's control channel: start BashCutPro with --control <port>"),
        };
        self.mcp_config = mcp.clone();
        let session = Some(self.conversation.session.as_str()).filter(|s| !s.is_empty());
        self.agent.command_line(mcp.as_deref(), session, bashcut_dirs::kit::kit_dir().as_deref())
    }

    /// Write `message` to the live agent, starting it (with `argv`) when there is none.
    fn send_live(&mut self, text: &str, message: String, argv: &[String], launch: Launch) -> Result<()> {
        let busy = self.busy();
        if !busy && self.launch.as_ref().is_some_and(|l| *l != launch) {
            // another engine, bridge or folder: a fresh process (resuming the session)
            self.end_session();
        }
        if self.live.as_ref().is_none_or(Live::is_finished) {
            self.start_live(argv, &launch)?;
        }
        self.launch = Some(launch);
        // an agent that cannot withdraw a written message gets a queued one when its turn ends (AG4)
        let held = busy && (!self.can_withdraw() || self.pending.iter().any(|p| p.held));
        let uuid = if self.can_withdraw() { new_uuid() } else { String::new() };
        if !held {
            self.write(&message, &uuid)?;
        }
        let pending = Pending { message, queued: None, uuid, held, cancelling: None };
        if busy {
            self.pending.push_back(Pending { queued: Some(text.to_string()), ..pending });
        } else {
            self.conversation.transcript.push(Item::User { text: text.to_string() });
            self.pending.push_back(pending);
            self.busy_since = Some(Instant::now());
            self.idle_since = None;
            self.timed_out = false;
            self.status = Status::Running;
        }
        Ok(())
    }

    /// Write one message to the live agent, with its `uuid` when it has one.
    fn write(&self, message: &str, uuid: &str) -> Result<()> {
        let Some(live) = &self.live else { return err("the agent has ended") };
        if uuid.is_empty() { live.send(message) } else { live.control(&stream_json::user_message_with_uuid(message, uuid)) }
    }

    /// Write the oldest held message once no turn runs and nothing written waits.
    fn write_held(&mut self) -> Result<()> {
        if self.in_turn || self.pending.iter().any(|p| !p.held) {
            return Ok(());
        }
        let Some(p) = self.pending.front_mut() else { return Ok(()) };
        p.held = false;
        let (message, uuid) = (p.message.clone(), p.uuid.clone());
        self.write(&message, &uuid)
    }

    fn start_live(&mut self, argv: &[String], launch: &Launch) -> Result<()> {
        let live = Live::start(argv, &launch.cwd, &launch.env, &self.agent.events)?;
        self.live = Some(live);
        // its own system/init says again what it supports
        self.capabilities.clear();
        self.in_turn = false;
        self.auth_failed = false;
        Ok(())
    }

    /// Start the live agent again (resuming its session) and write what it had not taken.
    fn restart_live(&mut self) -> Result<()> {
        let launch = self.launch.clone().ok_or_else(|| Error("the agent cannot be restarted".into()))?;
        let session = Some(self.conversation.session.as_str()).filter(|s| !s.is_empty());
        let argv = self.agent.command_line(launch.mcp.as_deref(), session, bashcut_dirs::kit::kit_dir().as_deref())?;
        self.start_live(&argv, &launch)?;
        for p in self.pending.iter().filter(|p| !p.held) {
            self.write(&p.message, &p.uuid)?;
        }
        self.write_held()
    }

    /// Stop the running turn: a per-message turn ends its process; a live agent gets the interrupt
    /// request when it can take it (else, or on a second Stop, SIGINT).
    pub fn stop(&mut self) {
        if let Some(t) = &self.turn {
            t.stop();
            self.status = Status::Stopping;
            return;
        }
        if !self.busy() {
            return;
        }
        if self.live.is_none() {
            return;
        }
        self.deny_permissions(STOPPED);
        let Some(l) = &self.live else { return };
        if self.status == Status::Stopping {
            // stopping already: the user insists
            self.interrupting = None;
            l.interrupt();
            return;
        }
        self.status = Status::Stopping;
        if let Err(e) = self.send_interrupt() {
            log::info!("bashcut agent: stop with SIGINT ({})", e.0);
            if let Some(l) = &self.live {
                l.interrupt();
            }
        }
    }

    /// Write the interrupt request: only to an agent that announced it, and (without `cancel_queued`)
    /// only when no message waits behind the turn (it would still run).
    fn send_interrupt(&mut self) -> Result<()> {
        let has = |c: &str| self.capabilities.iter().any(|x| x == c);
        if !has(INTERRUPT_RECEIPT) {
            return err("no interrupt request");
        }
        let cancel = has(INTERRUPT_CANCEL_QUEUED);
        if !cancel && self.pending.iter().filter(|p| !p.held).count() > usize::from(!self.in_turn) {
            return err("queued messages it cannot drop");
        }
        let Some(l) = &self.live else { return err("no agent") };
        self.requests = self.requests.wrapping_add(1);
        let id = format!("bashcut-stop-{}", self.requests);
        l.control(&stream_json::interrupt_request(&id, cancel))?;
        self.interrupting = Some(Interrupt { id, sent: Instant::now(), acked: false });
        Ok(())
    }

    /// The interrupt request ended the turn and the agent keeps running: what it never took goes
    /// back to the message box.
    fn stopped_in_place(&mut self) {
        self.interrupting = None;
        self.in_turn = false;
        self.unsent.extend(self.pending.drain(..).filter_map(|p| p.queued));
        self.busy_since = None;
        self.idle_since = Some(Instant::now());
        self.timed_out = false;
        self.status = Status::Idle;
        self.conversation.transcript.push(Item::Notice { message: "Stopped.".into() });
        self.save();
    }

    /// SIGINT when the interrupt request was refused or not answered in time.
    fn interrupt_fallback(&mut self, why: &str) {
        if self.interrupting.take().is_some() {
            log::info!("bashcut agent: interrupt request {why}: SIGINT");
            if let Some(l) = &self.live {
                l.interrupt();
            }
        }
    }

    /// Take the agent's events; `true` while a turn runs (or a live agent has unanswered messages).
    pub fn poll(&mut self) -> bool {
        if let Some(c) = &mut self.closing {
            // its late events belong to no turn
            c.poll();
            if c.is_finished() {
                self.closing = None;
            }
        }
        if self.live.is_some() || self.busy() {
            return self.poll_live();
        }
        let Some(turn) = &mut self.turn else { return false };
        let events = turn.poll();
        let mut ended = None;
        for e in events {
            self.events = self.events.wrapping_add(1);
            match e {
                TurnEvent::Agent(ev) => {
                    match &ev {
                        Event::Init { session, .. } | Event::Result { session, .. } if !session.is_empty() => {
                            self.conversation.session = session.clone();
                        }
                        _ => {}
                    }
                    if let Event::Init { commands, .. } = &ev
                        && !commands.is_empty()
                    {
                        self.commands = Some(commands.clone());
                    }
                    self.conversation.transcript.apply(&ev);
                }
                TurnEvent::Ended(end) => ended = Some(end),
            }
        }
        let Some(end) = ended else { return true };
        self.turn = None;
        let stopped = end == TurnEnd::Stopped;
        self.status = match end {
            TurnEnd::Done | TurnEnd::Stopped => Status::Idle,
            TurnEnd::TimedOut => {
                let message = "the agent took too long and was stopped".to_string();
                self.conversation.transcript.push(Item::Error { message: message.clone() });
                Status::Failed { message }
            }
            TurnEnd::Failed(message) => {
                self.conversation.transcript.push(Item::Error { message: message.clone() });
                Status::Failed { message }
            }
        };
        if stopped {
            self.conversation.transcript.push(Item::Notice { message: "Stopped.".into() });
        }
        self.save();
        false
    }

    fn poll_live(&mut self) -> bool {
        let events = self.live.as_mut().map(Live::poll).unwrap_or_default();
        let mut ended = None;
        for e in events {
            self.events = self.events.wrapping_add(1);
            match e {
                TurnEvent::Agent(ev) => self.live_event(ev),
                TurnEvent::Ended(end) => ended = Some(end),
            }
        }
        if let Some(end) = ended {
            self.live_ended(end);
        } else if self.live.is_none() && self.busy() {
            // nothing will answer (cannot happen; never leave the panel busy)
            self.live_ended(TurnEnd::Failed("the agent ended unexpectedly".into()));
        }
        let late = self.interrupting.as_ref().map(|i| (i.acked, i.sent.elapsed())).filter(|(acked, t)| *t > if *acked { RESULT_WAIT } else { ACK_WAIT });
        if let Some((acked, _)) = late {
            self.interrupt_fallback(if acked { "got no result" } else { "got no answer" });
        }
        let timeout = Duration::from_secs(self.agent.timeout_secs.max(1));
        if self.busy() && !self.timed_out && self.busy_since.is_some_and(|t| t.elapsed() > timeout) {
            self.timed_out = true;
            if let Some(l) = &self.live {
                l.interrupt();
            }
        }
        if !self.busy() && self.idle_since.is_some_and(|t| t.elapsed() > IDLE_CLOSE) {
            self.end_session();
        }
        self.busy()
    }

    fn live_event(&mut self, ev: Event) {
        match &ev {
            Event::Init { session, capabilities, plugins, commands, .. } => {
                if !session.is_empty() {
                    self.conversation.session = session.clone();
                }
                self.capabilities = capabilities.clone();
                self.plugins = Some(plugins.clone());
                self.commands = Some(commands.clone());
            }
            Event::Auth { .. } => self.auth_failed = true,
            Event::Permission(p) => {
                if p.oversized || p.request.is_empty() || self.permissions.len() >= MAX_PERMISSIONS {
                    if let Some(l) = &self.live {
                        let _ = l.control(&stream_json::permission_response(p, Answer::Deny, "BashCutPro could not show this request."));
                    }
                    self.conversation.transcript.push(Item::Permission { tool: p.tool.clone(), detail: p.detail.clone(), answer: "denied".into() });
                } else {
                    self.permissions.push(p.clone());
                }
                return;
            }
            Event::PermissionWithdrawn { request } => {
                if let Some(at) = self.permissions.iter().position(|p| p.request == *request) {
                    let p = self.permissions.remove(at);
                    self.conversation.transcript.push(Item::Permission { tool: p.tool, detail: p.detail, answer: "withdrawn".into() });
                }
                return;
            }
            Event::Control { request, ok, cancelled } => {
                if let Some(at) = self.pending.iter().position(|p| p.cancelling.as_deref() == Some(request.as_str())) {
                    if *ok && *cancelled {
                        self.pending.remove(at);
                        if let Err(e) = self.write_held() {
                            return self.live_failed(e.0);
                        }
                        self.after_cancel();
                    } else if let Some(p) = self.pending.get_mut(at) {
                        // taken already (its echo may still come): it stays and is answered
                        p.cancelling = None;
                    }
                    return;
                }
                let Some(i) = self.interrupting.as_mut().filter(|i| i.id == *request) else { return };
                if !*ok {
                    return self.interrupt_fallback("refused");
                }
                // the turn's `result` ends the stop (the `isReplay` echo can come later than this
                // receipt, so a turn may run although none was seen taking the message)
                i.acked = true;
                return;
            }
            Event::Picked { uuid } => {
                self.take_next(uuid);
                self.in_turn = true;
                self.restarts = 0;
            }
            Event::Result { ok, session, .. } => {
                if !session.is_empty() {
                    self.conversation.session = session.clone();
                }
                // an agent without the echo: the result answers the oldest message
                if !self.in_turn {
                    self.take_next("");
                }
                self.in_turn = false;
                self.restarts = 0;
                // the turn is over: nothing waits on an answer any more
                self.drop_permissions();
                if self.interrupting.is_some() {
                    // the interrupted turn's end (its error result is not news); a turn that
                    // finished first keeps its answer
                    if *ok {
                        self.conversation.transcript.apply(&ev);
                    }
                    return self.stopped_in_place();
                }
                // the interrupt's own error result is not news: "Stopped." follows
                if !*ok && self.status == Status::Stopping {
                    return;
                }
                self.conversation.transcript.apply(&ev);
                if let Err(e) = self.write_held() {
                    return self.live_failed(e.0);
                }
                if self.busy() {
                    self.busy_since = Some(Instant::now());
                } else {
                    if self.status == Status::Running {
                        self.status = Status::Idle;
                    }
                    self.busy_since = None;
                    self.idle_since = Some(Instant::now());
                    self.save();
                    if std::mem::take(&mut self.auth_failed) {
                        // the next message starts a fresh process, with the login as it is then
                        self.end_session();
                    }
                }
                return;
            }
            _ => {}
        }
        self.conversation.transcript.apply(&ev);
    }

    /// A pending message was taken (the one written with `uuid`, else the oldest written one): a
    /// queued one joins the transcript.
    fn take_next(&mut self, uuid: &str) {
        let by_uuid = self.pending.iter().position(|p| !uuid.is_empty() && p.uuid == uuid);
        let at = by_uuid.or_else(|| self.pending.front().filter(|p| !p.held).map(|_| 0));
        if let Some(Pending { queued: Some(text), .. }) = at.and_then(|i| self.pending.remove(i)) {
            self.conversation.transcript.push(Item::User { text });
        }
    }

    /// A queued message was withdrawn: when it was the last thing waiting and no turn runs (the turn
    /// ended before the confirmation), the agent is idle.
    fn after_cancel(&mut self) {
        if self.busy() || self.status != Status::Running {
            return;
        }
        self.status = Status::Idle;
        self.busy_since = None;
        self.idle_since = Some(Instant::now());
        self.save();
    }

    fn live_ended(&mut self, end: TurnEnd) {
        self.live = None;
        self.drop_permissions();
        self.interrupting = None;
        self.idle_since = None;
        // a turn it took and never ended counts too (else the panel stays Running with no error)
        let busy = std::mem::take(&mut self.in_turn) || !self.pending.is_empty();
        let stopping = self.status == Status::Stopping;
        if !self.pending.is_empty() && !stopping && !self.timed_out && end == TurnEnd::Done && self.restarts == 0 {
            // it exited before reading what was written (or between turns): again, resuming
            self.restarts = 1;
            match self.restart_live() {
                Ok(()) => return,
                Err(e) => return self.live_failed(e.0),
            }
        }
        let timed_out = std::mem::take(&mut self.timed_out);
        if !busy && !stopping && !timed_out {
            // an idle agent that exited: the next message starts it again
            if let TurnEnd::Failed(m) = end {
                log::warn!("bashcut agent ended between turns: {m}");
            }
            return;
        }
        if stopping || (end == TurnEnd::Stopped && !timed_out) {
            self.unsent.extend(self.pending.drain(..).filter_map(|p| p.queued));
            self.busy_since = None;
            self.status = Status::Idle;
            self.conversation.transcript.push(Item::Notice { message: "Stopped.".into() });
            self.save();
            return;
        }
        let message = match end {
            _ if timed_out => "the agent took too long and was stopped".to_string(),
            TurnEnd::Failed(m) => m,
            _ => "the agent ended before answering".to_string(),
        };
        self.live_failed(message);
    }

    /// The live agent failed with messages unanswered: show why, give back what it never read.
    fn live_failed(&mut self, message: String) {
        if self.agent.id == "claude-code" && message.contains("unknown option") {
            self.outdated = true;
        }
        self.live = None;
        self.in_turn = false;
        self.drop_permissions();
        self.unsent.extend(self.pending.drain(..).filter_map(|p| p.queued));
        self.busy_since = None;
        // after a sign-in problem the transcript's sign-in line says it already
        if !std::mem::take(&mut self.auth_failed) {
            self.conversation.transcript.push(Item::Error { message: message.clone() });
        }
        self.status = Status::Failed { message };
        self.save();
    }

    /// The gate waiting for the user, if the project has a plan.
    pub fn gate(&self) -> Option<Gate> {
        self.sidecar.as_deref().and_then(plan::pending)
    }

    /// Answer the pending gate and send the answer as the next message.
    pub fn answer_gate(&mut self, approve: bool, note: &str, ctx: &Context) -> Result<()> {
        if self.is_running() {
            return err("the agent is still working: wait or stop it");
        }
        self.set_project(ctx.project.as_deref());
        let side = self.sidecar.clone().ok_or_else(|| Error("no project plan".into()))?;
        let g = plan::pending(&side).ok_or_else(|| Error("no gate is waiting".into()))?;
        // the answer is written only when it can also be sent
        self.command_line(ctx)?;
        let message = plan::answer(&side, g.index, approve, note)?;
        let state = if approve { plan::APPROVED } else { plan::CHANGES };
        self.conversation.transcript.push(Item::Gate { stage: g.stage.clone(), state: state.into(), note: note.trim().to_string() });
        self.send(&message, ctx)
    }

    /// End the agent and remove the temporary MCP config (app exit): a live agent's stdin is closed and
    /// it gets [`EXIT_WAIT`] to end (SIGINT, then kill, past that).
    pub fn shutdown(&mut self) {
        self.stop();
        if let Some(mut l) = self.live.take() {
            l.close_and_wait(EXIT_WAIT);
        }
        if let Some(mut c) = self.closing.take() {
            c.close_and_wait(EXIT_WAIT);
        }
        self.pending.clear();
        self.permissions.clear();
        self.in_turn = false;
        self.save();
        if let Some(p) = self.mcp_config.take() {
            let _ = std::fs::remove_file(p);
        }
    }

    pub fn state_json(&self) -> Value {
        json!({
            "agent": {"id": self.agent.id, "name": self.agent.name},
            "agents": self.agents.iter().map(|a| json!({"id": a.id, "name": a.name})).collect::<Vec<_>>(),
            "status": self.status,
            "live": self.live.is_some(),
            "queued": self.queued(),
            "queuedItems": self.queued_items(),
            "capabilities": self.capabilities,
            "plugins": self.plugins,
            "slashCommands": self.commands,
            "permissions": self.permissions(),
            "conversation": {"id": self.conversation.id, "title": self.conversation.title(), "items": self.conversation.transcript.items.len(), "session": self.conversation.session},
            "gate": self.gate().map(|g| json!({"stage": g.stage, "summary": g.summary, "evidence": g.evidence_lines()})),
            "project": self.sidecar.is_some(),
        })
    }

    /// `bashcut.agent.<name>`.
    pub fn command(&mut self, name: &str, params: &Value, ctx: &Context) -> Result<Value> {
        let s = |k: &str| params.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        self.set_project(ctx.project.as_deref());
        match name {
            "state" => Ok(self.state_json()),
            "send" => {
                self.send(&s("text"), ctx)?;
                Ok(self.state_json())
            }
            "stop" => {
                self.stop();
                Ok(self.state_json())
            }
            "cancel" => {
                let index = params.get("index").and_then(Value::as_u64).ok_or_else(|| Error("which queued message? (index)".into()))?;
                self.cancel(usize::try_from(index).unwrap_or(usize::MAX))?;
                Ok(self.state_json())
            }
            "permission" => {
                let answer = match s("answer").as_str() {
                    "once" | "allow" => Answer::Once,
                    "always" => Answer::Always,
                    "deny" => Answer::Deny,
                    _ => return err("answer: once, always or deny"),
                };
                let index = params.get("index").and_then(Value::as_u64).map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
                self.answer_permission(index, answer)?;
                Ok(self.state_json())
            }
            "transcript" => {
                let from = params.get("from").and_then(Value::as_u64).map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
                let items = self.conversation.transcript.items.get(from..).unwrap_or_default();
                Ok(json!({"from": from, "items": items, "status": self.status}))
            }
            "conversations" => Ok(json!(self.conversations())),
            "open" => {
                self.open(&s("id"))?;
                Ok(self.state_json())
            }
            "new" => {
                self.new_conversation()?;
                Ok(self.state_json())
            }
            "use" => {
                self.use_agent(&s("agent"))?;
                Ok(self.state_json())
            }
            "gate" => Ok(json!(self.gate().map(|g| json!({"index": g.index, "stage": g.stage, "summary": g.summary, "evidence": g.evidence_lines()})))),
            "approve" => {
                self.answer_gate(true, &s("note"), ctx)?;
                Ok(self.state_json())
            }
            "change" => {
                self.answer_gate(false, &s("note"), ctx)?;
                Ok(self.state_json())
            }
            _ => err(format!("unknown command bashcut.agent.{name}")),
        }
    }
}

/// A random version-4 UUID for a message written to the live agent (std only: two randomly keyed
/// hashes of the time and a counter).
fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let half = || {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_u64(n);
        h.finish()
    };
    let v = (u128::from(half()) << 64 | u128::from(half())) & !(0xf000 << 64) & !(0xc << 60) | (0x4000 << 64) | (0x8 << 60);
    let x = format!("{v:032x}");
    let part = |a: usize, b: usize| x.get(a..b).unwrap_or_default();
    format!("{}-{}-{}-{}-{}", part(0, 8), part(8, 12), part(12, 16), part(16, 20), part(20, 32))
}

/// Where a turn runs: the project's folder, else [`unsaved_cwd`] (also for a bare file name, whose
/// parent is empty: no process starts in ""). A terminal resuming the session starts here too (the
/// agent keys its sessions by folder).
pub fn turn_dir(project: Option<&Path>) -> PathBuf {
    project.and_then(Path::parent).filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(unsaved_cwd)
}

/// Where an unsaved project's turn runs: `agent` in the app's data folder (else the temp dir), never
/// the app's own folder or a checkout (whose `.mcp.json` and `CLAUDE.md` the agent would load).
fn unsaved_cwd() -> PathBuf {
    let dir = bashcut_dirs::data_dir().map(|d| d.join("agent"));
    dir.filter(|d| std::fs::create_dir_all(d).is_ok()).unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_session_clears_plugins() {
        let mut a = Agent::new(Vec::new());
        a.plugins = Some(vec!["bcp".into()]);
        a.commands = Some(Vec::new());
        a.end_session();
        assert_eq!((a.plugins.as_ref(), a.commands.as_ref()), (None, None), "the next process lists its own");
    }

    #[test]
    fn a_saved_project_runs_in_its_folder() {
        assert_eq!(turn_dir(Some(Path::new("/films/My Film.fcproj"))), Path::new("/films"));
    }

    /// A waiting `Bash` request, with a suggested rule when `rule`.
    fn waiting(rule: bool) -> Permission {
        Permission {
            request: "perm-1".into(),
            tool: "Bash".into(),
            detail: "rm take.mov".into(),
            input: json!({"command": "rm take.mov"}),
            rules: if rule { vec![json!({"toolName": "Bash", "ruleContent": "rm:*"})] } else { Vec::new() },
            oversized: false,
        }
    }

    #[test]
    fn always_needs_a_saved_project_and_a_rule() {
        let mut agent = Agent::new(Vec::new());
        agent.permissions.push(waiting(true));
        assert!(!agent.permissions()[0].always, "an unsaved project has no folder for the rule");
        let e = agent.answer_permission(0, Answer::Always).unwrap_err();
        assert!(e.0.contains("allow it once"), "{e}");
        assert_eq!(agent.permissions().len(), 1, "a refused answer leaves the request waiting");
        assert!(agent.conversation.transcript.items.is_empty());

        agent.sidecar = Some(PathBuf::from("/films/film.bashcut"));
        assert!(agent.permissions()[0].always);
        agent.permissions = vec![waiting(false)];
        assert!(!agent.permissions()[0].always, "no suggested rule: nothing to keep");
        assert!(agent.answer_permission(0, Answer::Always).unwrap_err().0.contains("allow it once"));
        // without a process the answer cannot be written: the request stays
        assert!(agent.answer_permission(0, Answer::Once).unwrap_err().0.contains("ended"));
        assert_eq!(agent.permissions().len(), 1);
        assert!(agent.command("permission", &json!({"answer": "maybe"}), &Context::default()).is_err());
    }

    #[test]
    fn message_uuids_are_random_v4() {
        let a = new_uuid();
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12], "{a}");
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{a}");
        assert!(parts[2].starts_with('4'), "version 4: {a}");
        assert!(parts[3].starts_with(['8', '9', 'a', 'b']), "RFC 4122 variant: {a}");
        let many: std::collections::HashSet<String> = (0..100).map(|_| new_uuid()).collect();
        assert_eq!(many.len(), 100);
    }

    #[test]
    fn a_new_agent_is_idle_with_nothing_queued() {
        let mut agent = Agent::new(Vec::new());
        assert!(agent.is_live(), "Claude Code keeps one process per conversation");
        assert!(!agent.poll() && !agent.is_running());
        assert!(agent.queued().is_empty() && agent.take_unsent().is_empty());
        assert!(agent.cancel(0).is_err());
        assert!(agent.answer_permission(0, Answer::Once).is_err());
        agent.stop();
        assert_eq!(agent.status, Status::Idle, "Stop with nothing running does nothing");
        assert!(agent.release());
        let state = agent.state_json();
        assert_eq!((state["live"].as_bool(), state["project"].as_bool()), (Some(false), Some(false)));
    }
}
