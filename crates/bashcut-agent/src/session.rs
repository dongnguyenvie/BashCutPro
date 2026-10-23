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

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use crate::plan::{self, Gate};
use crate::process::{AgentSpec, Live, Turn, TurnEnd, TurnEvent, mcp_config_path, write_mcp_config};
use crate::store::{self, Conversation};
use crate::stream_json::{self, Event};
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

/// An interrupt request written to the live agent and not finished yet.
struct Interrupt {
    id: String,
    sent: Instant,
    acked: bool,
}

/// A message written to the live agent and not taken yet.
struct Pending {
    /// As written (with the context line), to write again after a restart.
    message: String,
    /// What the user typed, when it was sent while the agent worked (shown as queued, added to the
    /// transcript when the agent takes it); `None`: already in the transcript.
    queued: Option<String>,
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
    /// Messages written to the live agent and not taken yet, oldest first.
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
    /// Claude Code refused one of our flags (`unknown option`): too old for a live session.
    outdated: bool,
    /// The Stop in flight as an interrupt request (AG3).
    interrupting: Option<Interrupt>,
    /// Interrupt requests written so far (their ids).
    requests: u64,
    /// A live agent that was closed and has not exited yet (Open in Terminal waits for it).
    closing: Option<Live>,
    /// A client other than the panel (control channel, MCP, a plugin) drives the agent while the
    /// Agent section is hidden: the hidden panel does not close it after each turn (idle close still does).
    pub driven: bool,
    /// The agent reported a sign-in or account problem this turn (AG6): its process is not kept for
    /// the next message (the user may sign in meanwhile), and its exit adds no generic error.
    auth_failed: bool,
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
            outdated: false,
            interrupting: None,
            requests: 0,
            closing: None,
            driven: false,
            auth_failed: false,
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

    /// Queued messages the agent never read since the last call (for the message box).
    pub fn take_unsent(&mut self) -> Vec<String> {
        std::mem::take(&mut self.unsent)
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
        self.agent.command_line(mcp.as_deref(), session)
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
        let Some(live) = &self.live else { return err("the agent did not start") };
        live.send(&message)?;
        if busy {
            self.pending.push_back(Pending { message, queued: Some(text.to_string()) });
        } else {
            self.conversation.transcript.push(Item::User { text: text.to_string() });
            self.pending.push_back(Pending { message, queued: None });
            self.busy_since = Some(Instant::now());
            self.idle_since = None;
            self.timed_out = false;
            self.status = Status::Running;
        }
        Ok(())
    }

    fn start_live(&mut self, argv: &[String], launch: &Launch) -> Result<()> {
        let live = Live::start(argv, &launch.cwd, &launch.env, &self.agent.events)?;
        self.live = Some(live);
        self.in_turn = false;
        self.auth_failed = false;
        Ok(())
    }

    /// Start the live agent again (resuming its session) and write what it had not taken.
    fn restart_live(&mut self) -> Result<()> {
        let launch = self.launch.clone().ok_or_else(|| Error("the agent cannot be restarted".into()))?;
        let session = Some(self.conversation.session.as_str()).filter(|s| !s.is_empty());
        let argv = self.agent.command_line(launch.mcp.as_deref(), session)?;
        self.start_live(&argv, &launch)?;
        let Some(live) = &self.live else { return err("the agent did not start") };
        for p in &self.pending {
            live.send(&p.message)?;
        }
        Ok(())
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
        if !cancel && self.pending.len() > usize::from(!self.in_turn) {
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
            Event::Init { session, capabilities, .. } => {
                if !session.is_empty() {
                    self.conversation.session = session.clone();
                }
                self.capabilities = capabilities.clone();
            }
            Event::Auth { .. } => self.auth_failed = true,
            Event::Control { request, ok } => {
                let Some(i) = self.interrupting.as_mut().filter(|i| i.id == *request) else { return };
                if !*ok {
                    return self.interrupt_fallback("refused");
                }
                // the turn's `result` ends the stop (the `isReplay` echo can come later than this
                // receipt, so a turn may run although none was seen taking the message)
                i.acked = true;
                return;
            }
            Event::Picked => {
                self.take_next();
                self.in_turn = true;
                self.restarts = 0;
            }
            Event::Result { ok, session, .. } => {
                if !session.is_empty() {
                    self.conversation.session = session.clone();
                }
                // an agent without the echo: the result answers the oldest message
                if !self.in_turn {
                    self.take_next();
                }
                self.in_turn = false;
                self.restarts = 0;
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

    /// The oldest pending message was taken: a queued one joins the transcript.
    fn take_next(&mut self) {
        if let Some(Pending { queued: Some(text), .. }) = self.pending.pop_front() {
            self.conversation.transcript.push(Item::User { text });
        }
    }

    fn live_ended(&mut self, end: TurnEnd) {
        self.live = None;
        self.interrupting = None;
        self.in_turn = false;
        self.idle_since = None;
        let busy = !self.pending.is_empty();
        let stopping = self.status == Status::Stopping;
        if busy && !stopping && !self.timed_out && end == TurnEnd::Done && self.restarts == 0 {
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
            "capabilities": self.capabilities,
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

/// Where a turn runs: the project's folder, else [`unsaved_cwd`]. A terminal resuming the session
/// starts here too (the agent keys its sessions by folder).
pub fn turn_dir(project: Option<&Path>) -> PathBuf {
    project.and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(unsaved_cwd)
}

/// Where an unsaved project's turn runs: `agent` in the app's data folder (else the temp dir), never
/// the app's own folder or a checkout (whose `.mcp.json` and `CLAUDE.md` the agent would load).
fn unsaved_cwd() -> PathBuf {
    let dir = bashcut_dirs::data_dir().map(|d| d.join("agent"));
    dir.filter(|d| std::fs::create_dir_all(d).is_ok()).unwrap_or_else(std::env::temp_dir)
}
