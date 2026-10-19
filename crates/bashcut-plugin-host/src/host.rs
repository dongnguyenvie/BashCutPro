//! The host the app keeps: installed plugins, the registry, and every job (plugin runs, installs,
//! registry fetches) with its progress, log and outcome. Long work runs on threads; the UI calls
//! [`Host::poll`] each frame. [`Host::command`] serves the `bashcut.plugins.*` command ids, so the
//! panels, shortcuts, the control channel and MCP all go through the same code.

use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use crate::install::{Installed, Installer};
use crate::manifest::{Connection, Manifest, version_key};
use crate::registry::{self, Registry};
use crate::runner::{End, Event, Outcome, RunSpec, Running};
use crate::settings::{self, SecretStore};
use crate::view::View;
use crate::{Error, Result, err};

/// Jobs kept in the list (oldest finished ones go first).
pub const MAX_JOBS: usize = 200;
/// Log lines kept per job.
pub const MAX_LOG: usize = 500;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum JobKind {
    Run { plugin: String, action: String },
    Install { plugin: String },
    Registry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
    HandOff,
    Cancelled,
    TimedOut,
    Crashed,
}

impl JobState {
    pub fn is_running(self) -> bool {
        self == JobState::Running
    }
}

impl From<End> for JobState {
    fn from(e: End) -> Self {
        match e {
            End::Succeeded => JobState::Succeeded,
            End::Failed => JobState::Failed,
            End::HandOff => JobState::HandOff,
            End::Cancelled => JobState::Cancelled,
            End::TimedOut => JobState::TimedOut,
            End::Crashed => JobState::Crashed,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    pub title: String,
    pub state: JobState,
    pub progress: Option<f32>,
    /// The latest progress line, then the outcome's summary.
    pub message: String,
    pub log: VecDeque<String>,
    pub outcome: Option<Outcome>,
    pub elapsed_ms: u64,
    /// For a hand-off: the action the agent runs to finish (`handoff.then`).
    pub then: Option<String>,
    /// The latest `bashcut-view/1` form the run printed (or put in its result as `data.view`).
    pub view: Option<View>,
    #[serde(skip)]
    started: Instant,
}

impl Job {
    fn new(id: u64, kind: JobKind, title: String) -> Self {
        Job {
            id,
            kind,
            title,
            state: JobState::Running,
            progress: None,
            message: String::new(),
            log: VecDeque::new(),
            outcome: None,
            elapsed_ms: 0,
            then: None,
            view: None,
            started: Instant::now(),
        }
    }

    fn push_log(&mut self, line: String) {
        if self.log.len() >= MAX_LOG {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    fn finish(&mut self, state: JobState, message: String) {
        self.state = state;
        if state == JobState::Succeeded {
            self.progress = Some(1.0);
        }
        self.message = message;
        self.elapsed_ms = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    }
}

enum Task {
    Registry(Result<Registry>),
    Install(Result<Installed>),
}

enum Live {
    Run(Running),
    Task(Receiver<Task>),
}

/// How to start an agent plugin (`Host::agents`).
#[derive(Clone, Debug, PartialEq)]
pub struct AgentLaunch {
    pub id: String,
    pub name: String,
    pub argv: Vec<String>,
    pub mcp_args: Vec<String>,
    pub resume_args: Vec<String>,
    pub events: String,
    pub env: Vec<(String, String)>,
}

/// What a run connects to, from the app.
#[derive(Clone, Debug, Default)]
pub struct RunContext {
    /// `127.0.0.1:<port>` of the app's control channel, when it serves one.
    pub bridge: Option<String>,
    /// `filmcraft-cli` next to the app, passed as `--cli`.
    pub cli: Option<String>,
    /// The running build's command ids (checked against `app.commands`); empty skips the check.
    pub known_commands: Vec<String>,
}

pub struct Host {
    installer: Option<Installer>,
    secrets: Arc<dyn SecretStore>,
    /// `repo` and `ref` of the registry.
    pub registry_location: (String, String),
    registry: Option<Registry>,
    installed: Vec<Installed>,
    jobs: Vec<Job>,
    live: Vec<(u64, Live)>,
    next_id: u64,
}

impl Host {
    /// A host installing into `<data_dir>/bashcut-plugins`; with no data dir nothing can be
    /// installed and the lists are empty. The registry comes from `BASHCUT_PLUGIN_REGISTRY`
    /// (`repo[@ref]` or a local `registry.json`), else the default repository.
    pub fn new(data_dir: Option<PathBuf>, secrets: Arc<dyn SecretStore>) -> Self {
        let loc = std::env::var("BASHCUT_PLUGIN_REGISTRY").ok().filter(|s| !s.trim().is_empty());
        let registry_location =
            loc.map_or_else(|| (registry::DEFAULT_REPO.to_string(), registry::DEFAULT_REF.to_string()), |l| registry::split_location(l.trim()));
        let installer = data_dir.as_deref().map(Installer::new);
        let mut h = Host { installer, secrets, registry_location, registry: None, installed: Vec::new(), jobs: Vec::new(), live: Vec::new(), next_id: 1 };
        h.refresh();
        h
    }

    /// Re-read the installed plugins from disk.
    pub fn refresh(&mut self) {
        self.installed = self.installer.as_ref().map(Installer::list).unwrap_or_default();
    }

    pub fn installed(&self) -> &[Installed] {
        &self.installed
    }

    pub fn plugin(&self, id: &str) -> Option<&Installed> {
        self.installed.iter().find(|p| p.id == id)
    }

    fn manifest(&self, id: &str) -> Result<&Manifest> {
        let p = self.plugin(id).ok_or_else(|| Error(format!("{id} is not installed")))?;
        p.manifest.as_ref().ok_or_else(|| Error(format!("{id}: {}", p.error.clone().unwrap_or_default())))
    }

    pub fn registry(&self) -> Option<&Registry> {
        self.registry.as_ref()
    }

    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    pub fn job(&self, id: u64) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    pub fn any_running(&self) -> bool {
        !self.live.is_empty()
    }

    /// A newer version in the registry than the installed one.
    pub fn update_for(&self, id: &str) -> Option<String> {
        let have = self.plugin(id)?.manifest.as_ref()?.version.clone();
        let r = self.registry.as_ref()?.find(id)?;
        (version_key(&r.version) > version_key(&have)).then(|| r.version.clone())
    }

    /// The installed agent plugins that run here, as the Agent section starts them: the entry (a
    /// relative program resolved in the plugin folder) plus `agent.args`, and the settings' env.
    pub fn agents(&self) -> Vec<AgentLaunch> {
        let Ok(dir) = self.plugins_dir() else { return Vec::new() };
        self.installed
            .iter()
            .filter_map(|p| {
                let m = p.manifest.as_ref().filter(|m| m.is_agent() && m.supports(crate::os()))?;
                let a = m.agent.as_ref()?;
                let mut argv = m.entry_for(crate::os()).to_vec();
                if let Some(first) = argv.first_mut()
                    && (first.starts_with("./") || first.starts_with(".\\"))
                {
                    *first = p.dir.join(first.as_str()).to_string_lossy().into_owned();
                }
                argv.extend(a.args.iter().cloned());
                let values = settings::load(&dir, m);
                let mut env = settings::env_for(m, &values, self.secrets.as_ref());
                env.push(("BASHCUT_PLUGIN_HOST".into(), "1".into()));
                Some(AgentLaunch {
                    id: m.id.clone(),
                    name: if m.name.is_empty() { m.id.clone() } else { m.name.clone() },
                    argv,
                    mcp_args: a.mcp_config.clone(),
                    resume_args: a.resume.clone(),
                    events: if a.events.is_empty() { crate::manifest::AGENT_EVENTS.to_string() } else { a.events.clone() },
                    env,
                })
            })
            .collect()
    }

    fn plugins_dir(&self) -> Result<PathBuf> {
        self.installer.as_ref().map(|i| i.root.clone()).ok_or_else(|| Error("no data folder: plugins cannot be installed in this session".into()))
    }

    fn add_job(&mut self, kind: JobKind, title: String) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        while self.jobs.len() >= MAX_JOBS {
            match self.jobs.iter().position(|j| !j.state.is_running()) {
                Some(i) => {
                    self.jobs.remove(i);
                }
                None => break,
            }
        }
        self.jobs.push(Job::new(id, kind, title));
        id
    }

    fn job_mut(&mut self, id: u64) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    /// Fetch the registry in the background.
    pub fn fetch_registry(&mut self) -> Result<u64> {
        let installer = self.installer.clone().ok_or_else(|| Error("no data folder".into()))?;
        let (repo, git_ref) = self.registry_location.clone();
        let id = self.add_job(JobKind::Registry, format!("Registry {repo}@{git_ref}"));
        let rx = spawn_task(move || Task::Registry(installer.fetch_registry(&repo, &git_ref)))?;
        self.live.push((id, Live::Task(rx)));
        Ok(id)
    }

    /// Install or update `plugin` from the loaded registry in the background.
    pub fn install(&mut self, plugin: &str) -> Result<u64> {
        let installer = self.installer.clone().ok_or_else(|| Error("no data folder".into()))?;
        let Some(reg) = &self.registry else { return err("load the registry first (bashcut.plugins.registry)") };
        let entry = reg.find(plugin).cloned().ok_or_else(|| Error(format!("{plugin} is not in the registry")))?;
        if !entry.supports(crate::os()) {
            return err(format!("{plugin} does not run on {}", crate::os()));
        }
        if self.live_install(plugin) {
            return err(format!("{plugin} is being installed"));
        }
        let verb = if installer.is_installed(plugin) { "Update" } else { "Install" };
        let id = self.add_job(JobKind::Install { plugin: plugin.to_string() }, format!("{verb} {} {}", entry.name, entry.version));
        let rx = spawn_task(move || Task::Install(installer.install(&entry)))?;
        self.live.push((id, Live::Task(rx)));
        Ok(id)
    }

    fn live_install(&self, plugin: &str) -> bool {
        self.live.iter().any(|(id, _)| self.job(*id).is_some_and(|j| j.kind == JobKind::Install { plugin: plugin.to_string() }))
    }

    pub fn remove(&mut self, plugin: &str) -> Result<()> {
        if self
            .live
            .iter()
            .any(|(id, _)| self.job(*id).is_some_and(|j| matches!(&j.kind, JobKind::Run { plugin: p, .. } | JobKind::Install { plugin: p } if p == plugin)))
        {
            return err(format!("{plugin} is running; cancel it first"));
        }
        let installer = self.installer.as_ref().ok_or_else(|| Error("no data folder".into()))?;
        if let Ok(m) = self.manifest(plugin) {
            for s in m.settings.iter().filter(|s| s.is_secret()) {
                let _ = self.secrets.delete(plugin, &s.id);
            }
        }
        installer.remove(plugin)?;
        self.refresh();
        Ok(())
    }

    pub fn settings(&self, plugin: &str) -> Result<settings::Values> {
        Ok(settings::load(&self.plugins_dir()?, self.manifest(plugin)?))
    }

    pub fn set_settings(&self, plugin: &str, values: &settings::Values) -> Result<settings::Values> {
        settings::save(&self.plugins_dir()?, self.manifest(plugin)?, values)
    }

    /// Which secrets are stored (never their values).
    pub fn secrets_set(&self, plugin: &str) -> Result<BTreeMap<String, bool>> {
        let m = self.manifest(plugin)?;
        Ok(m.settings.iter().filter(|s| s.is_secret()).map(|s| (s.id.clone(), self.secrets.get(plugin, &s.id).ok().flatten().is_some())).collect())
    }

    /// Store (`Some`) or delete (`None`) a secret setting.
    pub fn set_secret(&self, plugin: &str, setting: &str, value: Option<&str>) -> Result<()> {
        let m = self.manifest(plugin)?;
        if !m.setting(setting).is_some_and(|s| s.is_secret()) {
            return err(format!("{plugin} has no secret `{setting}`"));
        }
        match value.filter(|v| !v.is_empty()) {
            Some(v) => self.secrets.set(plugin, setting, v),
            None => self.secrets.delete(plugin, setting),
        }
    }

    /// Start an action. Params are checked against the manifest before anything runs.
    pub fn run(&mut self, plugin: &str, action: &str, params: &BTreeMap<String, Value>, ctx: &RunContext, timeout: Option<Duration>) -> Result<u64> {
        let p = self.plugin(plugin).ok_or_else(|| Error(format!("{plugin} is not installed")))?;
        let dir = p.dir.clone();
        let m = self.manifest(plugin)?.clone();
        if !m.supports(crate::os()) {
            return err(format!("{plugin} does not run on {}", crate::os()));
        }
        let missing = if ctx.known_commands.is_empty() { Vec::new() } else { m.missing_commands(&ctx.known_commands) };
        if !missing.is_empty() {
            return err(format!("{plugin} needs commands this build lacks: {}", missing.join(", ")));
        }
        let a = m.action(action).ok_or_else(|| Error(format!("{plugin} has no action `{action}`")))?.clone();
        let conn = match &ctx.bridge {
            Some(addr) => Connection::Bridge(addr.clone()),
            None => Connection::None,
        };
        let argv = m.command_line(crate::os(), action, &conn, ctx.cli.as_deref(), params)?;
        let values = settings::load(&self.plugins_dir()?, &m);
        let mut spec = RunSpec::new(dir, argv);
        spec.env = settings::env_for(&m, &values, self.secrets.as_ref());
        spec.env.push(("BASHCUT_PLUGIN_HOST".into(), "1".into()));
        if let Some(t) = timeout {
            spec.timeout = t;
        }
        if let Some(h) = &a.handoff {
            spec.handoff_exit = h.exit;
        }
        let running = Running::start(spec)?;
        let title = format!("{} ▸ {}", if m.name.is_empty() { &m.id } else { &m.name }, a.title.trim_end_matches('…'));
        let id = self.add_job(JobKind::Run { plugin: plugin.to_string(), action: action.to_string() }, title);
        if let Some(j) = self.job_mut(id) {
            j.then = a.handoff.as_ref().map(|h| h.then.clone()).filter(|t| !t.is_empty());
        }
        self.live.push((id, Live::Run(running)));
        Ok(id)
    }

    /// The newest form `plugin` showed, and the run that showed it.
    pub fn view(&self, plugin: &str) -> Option<(u64, &View)> {
        self.jobs.iter().rev().find_map(|j| match (&j.kind, &j.view) {
            (JobKind::Run { plugin: p, .. }, Some(v)) if p == plugin => Some((j.id, v)),
            _ => None,
        })
    }

    /// Press a button of `plugin`'s form: the button row `button`, or with `item = (item, button)`
    /// that item's button in the list `button`. The action it names starts with the form's
    /// `values` (those the action takes) and the button's params on top.
    pub fn click(&mut self, plugin: &str, button: &str, item: Option<(&str, &str)>, values: &BTreeMap<String, Value>, ctx: &RunContext) -> Result<u64> {
        let (_, view) = self.view(plugin).ok_or_else(|| Error(format!("{plugin} shows no form")))?;
        let (action, extra) = view.button(button, item).ok_or_else(|| Error(format!("{plugin}'s form has no button `{button}`")))?;
        let m = self.manifest(plugin)?;
        let a = m.action(&action).ok_or_else(|| Error(format!("{plugin} has no action `{action}`")))?;
        let mut params: BTreeMap<String, Value> =
            values.iter().filter(|(k, _)| a.params.iter().any(|p| &p.id == *k)).map(|(k, v)| (k.clone(), v.clone())).collect();
        params.extend(extra);
        self.run(plugin, &action, &params, ctx, None)
    }

    pub fn cancel(&mut self, job: u64) -> Result<()> {
        match self.live.iter().find(|(id, _)| *id == job) {
            Some((_, Live::Run(r))) => {
                r.cancel();
                Ok(())
            }
            Some((_, Live::Task(_))) => err("installs and registry fetches cannot be cancelled"),
            None => err(format!("job {job} is not running")),
        }
    }

    /// Drop finished jobs from the list.
    pub fn clear_finished(&mut self) {
        self.jobs.retain(|j| j.state.is_running());
    }

    /// Collect events from running jobs; returns true while anything still runs (ask for a repaint).
    pub fn poll(&mut self) -> bool {
        let mut live = std::mem::take(&mut self.live);
        let mut refresh = false;
        live.retain_mut(|(id, l)| {
            let id = *id;
            match l {
                Live::Run(r) => {
                    for e in r.poll() {
                        let Some(j) = self.job_mut(id) else { continue };
                        match e {
                            Event::Progress { fraction, message } => {
                                if fraction.is_some() {
                                    j.progress = fraction;
                                }
                                j.message = message.clone();
                                j.push_log(message);
                            }
                            Event::View(v) => j.view = Some(v),
                            Event::Done(o) => {
                                if let Some(v) = o.result.as_ref().and_then(|r| r.data.get("view")).and_then(View::from_value) {
                                    j.view = Some(v);
                                }
                                let msg = o.summary();
                                j.finish(o.end.into(), msg);
                                j.outcome = Some(o);
                            }
                        }
                    }
                    !r.is_finished()
                }
                Live::Task(rx) => {
                    let done = match rx.try_recv() {
                        Ok(t) => Some(t),
                        Err(TryRecvError::Empty) => None,
                        Err(TryRecvError::Disconnected) => Some(Task::Install(err("the task ended without a result"))),
                    };
                    let Some(t) = done else { return true };
                    let (state, msg) = match t {
                        Task::Registry(Ok(r)) => {
                            let msg = format!("{} plugins", r.plugins.len());
                            self.registry = Some(r);
                            (JobState::Succeeded, msg)
                        }
                        Task::Install(Ok(p)) => {
                            refresh = true;
                            match (&p.manifest, &p.error) {
                                (Some(m), _) => (JobState::Succeeded, format!("{} {} installed", m.name, m.version)),
                                (None, e) => (JobState::Failed, e.clone().unwrap_or_default()),
                            }
                        }
                        Task::Registry(Err(e)) | Task::Install(Err(e)) => (JobState::Failed, e.0),
                    };
                    if let Some(j) = self.job_mut(id) {
                        j.push_log(msg.clone());
                        j.finish(state, msg);
                    }
                    false
                }
            }
        });
        // anything started while polling (nothing today) stays
        live.append(&mut self.live);
        self.live = live;
        if refresh {
            self.refresh();
        }
        for j in self.jobs.iter_mut().filter(|j| j.state.is_running()) {
            j.elapsed_ms = j.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        }
        !self.live.is_empty()
    }

    /// Serve `bashcut.plugins.<name>`. `ctx` is only read by `run`.
    pub fn command(&mut self, name: &str, params: &Value, ctx: &RunContext) -> Result<Value> {
        let s = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_string);
        let need = |k: &str| s(k).ok_or_else(|| Error(format!("`{k}` is required")));
        let job_param = || params.get("job").and_then(Value::as_u64).ok_or_else(|| Error("`job` (number) is required".into()));
        match name {
            "list" => Ok(self.list_json()),
            "registry" => {
                if let Some(loc) = s("location").filter(|l| !l.trim().is_empty()) {
                    self.registry_location =
                        if std::path::Path::new(loc.trim()).is_file() { (loc.trim().to_string(), String::new()) } else { registry::split_location(loc.trim()) };
                }
                Ok(json!({"job": self.fetch_registry()?}))
            }
            "install" | "update" => Ok(json!({"job": self.install(&need("plugin")?)?})),
            "remove" => {
                self.remove(&need("plugin")?)?;
                Ok(Value::Null)
            }
            "settings" => {
                let plugin = need("plugin")?;
                let values = match params.get("values") {
                    Some(Value::Object(v)) => self.set_settings(&plugin, &v.clone().into_iter().collect())?,
                    Some(_) => return err("`values` must be an object"),
                    None => self.settings(&plugin)?,
                };
                Ok(json!({"values": values, "secrets": self.secrets_set(&plugin)?}))
            }
            "setSecret" => {
                let plugin = need("plugin")?;
                let setting = need("setting")?;
                self.set_secret(&plugin, &setting, params.get("value").and_then(Value::as_str))?;
                Ok(json!({"secrets": self.secrets_set(&plugin)?}))
            }
            "run" => {
                let plugin = need("plugin")?;
                let action = need("action")?;
                let p: BTreeMap<String, Value> = match params.get("params") {
                    Some(Value::Object(m)) => m.clone().into_iter().collect(),
                    None | Some(Value::Null) => BTreeMap::new(),
                    Some(_) => return err("`params` must be an object"),
                };
                let timeout = params
                    .get("timeoutSeconds")
                    .and_then(Value::as_f64)
                    .filter(|t| t.is_finite() && *t > 0.0)
                    .map(|t| Duration::from_secs_f64(t.min(86_400.0)));
                Ok(json!({"job": self.run(&plugin, &action, &p, ctx, timeout)?}))
            }
            "view" => {
                let plugin = need("plugin")?;
                Ok(self.view(&plugin).map_or(Value::Null, |(job, v)| json!({"job": job, "view": v})))
            }
            "click" => {
                let plugin = need("plugin")?;
                let button = need("button")?;
                let item = s("item");
                let item_button = s("itemButton");
                let item = match (&item, &item_button) {
                    (Some(i), Some(b)) => Some((i.as_str(), b.as_str())),
                    (None, None) => None,
                    _ => return err("`item` and `itemButton` go together"),
                };
                let values: BTreeMap<String, Value> = match params.get("values") {
                    Some(Value::Object(m)) => m.clone().into_iter().collect(),
                    None | Some(Value::Null) => BTreeMap::new(),
                    Some(_) => return err("`values` must be an object"),
                };
                Ok(json!({"job": self.click(&plugin, &button, item, &values, ctx)?}))
            }
            "runs" => Ok(json!(self.jobs.iter().map(job_summary).collect::<Vec<_>>())),
            "job" => {
                let id = job_param()?;
                self.job(id).map(|j| serde_json::to_value(j).unwrap_or(Value::Null)).ok_or_else(|| Error(format!("no job {id}")))
            }
            "cancel" => {
                self.cancel(job_param()?)?;
                Ok(Value::Null)
            }
            "clear" => {
                self.clear_finished();
                Ok(Value::Null)
            }
            _ => err(format!("unknown command bashcut.plugins.{name}")),
        }
    }

    fn list_json(&self) -> Value {
        let installed: Vec<Value> = self
            .installed
            .iter()
            .map(|p| match &p.manifest {
                Some(m) => json!({
                    "id": p.id, "name": m.name, "version": m.version, "summary": m.summary, "category": m.category,
                    "actions": m.actions.iter().map(|a| json!({"id": a.id, "title": a.title, "needs": a.needs, "edits": a.edits, "params": a.params})).collect::<Vec<_>>(),
                    "settings": m.settings, "update": self.update_for(&p.id), "origin": p.origin,
                }),
                None => json!({"id": p.id, "error": p.error}),
            })
            .collect();
        let registry = self.registry.as_ref().map(|r| {
            r.plugins
                .iter()
                .map(|e| {
                    let have = self.plugin(&e.id).and_then(|p| p.manifest.as_ref()).map(|m| m.version.clone());
                    json!({"id": e.id, "name": e.name, "version": e.version, "summary": e.summary, "category": e.category, "installed": have, "supported": e.supports(crate::os())})
                })
                .collect::<Vec<_>>()
        });
        json!({
            "installed": installed,
            "registry": registry,
            "location": format!("{}@{}", self.registry_location.0, self.registry_location.1),
            "canInstall": self.installer.is_some(),
        })
    }
}

/// A job without its log, for lists.
pub fn job_summary(j: &Job) -> Value {
    json!({"id": j.id, "kind": j.kind, "title": j.title, "state": j.state, "progress": j.progress, "message": j.message, "elapsedMs": j.elapsed_ms, "then": j.then, "view": j.view.is_some()})
}

fn spawn_task(f: impl FnOnce() -> Task + Send + 'static) -> Result<Receiver<Task>> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("bashcut-plugin-task".into())
        .spawn(move || {
            let t = catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Task::Install(err("the task failed")));
            let _ = tx.send(t);
        })
        .map_err(|e| Error(format!("cannot start a task thread: {e}")))?;
    Ok(rx)
}
