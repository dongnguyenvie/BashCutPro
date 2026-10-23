//! The agent the app keeps: the chosen agent, the open conversation, the running turn, and the
//! project's gates. The UI calls [`Agent::poll`] each frame; [`Agent::command`] serves the
//! `bashcut.agent.*` command ids, so the panel, shortcuts, the control channel and MCP share it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::plan::{self, Gate};
use crate::process::{AgentSpec, Turn, TurnEnd, TurnEvent, mcp_config_path, write_mcp_config};
use crate::store::{self, Conversation};
use crate::stream_json::Event;
use crate::transcript::Item;
use crate::{Error, Result, err};

/// Longest message sent.
pub const MAX_MESSAGE: usize = 64 * 1024;

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
}

impl Agent {
    pub fn new(agents: Vec<AgentSpec>) -> Self {
        let agents = if agents.is_empty() { vec![AgentSpec::claude_code(None)] } else { agents };
        let agent = agents.first().cloned().unwrap_or_else(|| AgentSpec::claude_code(None));
        let conversation = Conversation::new(&agent.id);
        Agent { agents, agent, conversation, status: Status::Idle, sidecar: None, turn: None, mcp_config: None, events: 0 }
    }

    pub fn is_running(&self) -> bool {
        self.turn.is_some()
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

    /// Start a turn with `text`.
    pub fn send(&mut self, text: &str, ctx: &Context) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return err("nothing to send");
        }
        if text.len() > MAX_MESSAGE {
            return err("message too long");
        }
        if self.is_running() {
            return err("the agent is still working: wait or stop it");
        }
        self.set_project(ctx.project.as_deref());
        let argv = self.command_line(ctx)?;
        let message = if ctx.line.is_empty() { text.to_string() } else { format!("[{}]\n\n{text}", ctx.line) };
        let cwd = ctx.project.as_deref().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(unsaved_cwd);
        let mut env = self.agent.env.clone();
        env.push(("BASHCUT_AGENT_HOST".into(), "1".into()));
        if let Some(side) = ctx.project.as_deref().map(crate::sidecar) {
            env.push(("BASHCUT_PROJECT_SIDECAR".into(), side.to_string_lossy().into_owned()));
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

    pub fn stop(&mut self) {
        if let Some(t) = &self.turn {
            t.stop();
            self.status = Status::Stopping;
        }
    }

    /// Take the running turn's events; `true` while a turn runs.
    pub fn poll(&mut self) -> bool {
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

    /// Remove the temporary MCP config (app exit).
    pub fn shutdown(&mut self) {
        self.stop();
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

/// Where an unsaved project's turn runs: `agent` in the app's data folder (else the temp dir), never
/// the app's own folder or a checkout (whose `.mcp.json` and `CLAUDE.md` the agent would load).
fn unsaved_cwd() -> PathBuf {
    let dir = bashcut_dirs::data_dir().map(|d| d.join("agent"));
    dir.filter(|d| std::fs::create_dir_all(d).is_ok()).unwrap_or_else(std::env::temp_dir)
}
