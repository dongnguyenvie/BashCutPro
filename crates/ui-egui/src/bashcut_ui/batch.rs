//! The Batch section's model (`bashcut/in-app-ui.md` §7, #46): the pipeline being built (the
//! draft), the job it runs and the `bashcut.batch.*` commands that the section, the control
//! channel and MCP share. The runner is the `bashcut-batch` library (`bashcut/batch.md`), the same
//! one the `bashcut-batch` command uses, on a background thread: every step is its own
//! `filmcraft-cli` process, so the open project is never touched. State lives in egui context
//! data; learned step times and recent jobs in `<data dir>/bashcut/batch.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use bashcut_batch::book::now_ms;
use bashcut_batch::pipeline::{self, AgentStep, OnFailure, Output, ProcessStep, Step};
use bashcut_batch::{Book, ItemState, Job, Options, Pipeline, SCHEMA, Status, Summary, presets};
use bashcut_plugin_host::manifest::Connection;
use bashcut_plugin_host::settings::{self, MemoryStore};
use filmcraft_engine::shortcuts::CommandInfo;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::palette::RECIPES;
use super::plugins;
use crate::FilmcraftApp;

pub type Shared = Arc<Mutex<Batch>>;
type Slot = Arc<Mutex<Option<Result<Summary, String>>>>;

/// How often the job book is read back while a job runs.
const READ_EVERY: Duration = Duration::from_millis(500);
const MAX_RECENT: usize = 20;
/// Largest `batch.json` read back.
const MAX_FILE: u64 = 256 * 1024;
/// Runs a learned step time averages over (newer runs weigh more after that).
const LEARN_RUNS: u32 = 20;
/// A step's expected time before any run of it was timed.
pub const DEFAULT_STEP_MS: u64 = 10_000;

/// Average time of a step, by label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Learned {
    pub ms: u64,
    pub runs: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    /// The job folder.
    pub job: String,
    pub name: String,
    pub at: u64,
}

/// What `batch.json` keeps between sessions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Memory {
    pub durations: BTreeMap<String, Learned>,
    /// Newest first.
    pub recent: Vec<Recent>,
}

impl Memory {
    /// Fold the finished steps of `book` (ended at or after `since`, epoch ms) into the averages.
    pub fn learn(&mut self, book: &Book, since: u64) {
        for step in book.items.iter().flat_map(|i| i.steps.iter()) {
            let (Status::Done, Some(ms), Some(end)) = (step.status, step.duration_ms, step.ended_at) else { continue };
            if end < since {
                continue;
            }
            let l = self.durations.entry(step.label.clone()).or_default();
            let n = u64::from(l.runs.min(LEARN_RUNS - 1));
            l.ms = l.ms.saturating_mul(n).saturating_add(ms) / n.saturating_add(1);
            l.runs = l.runs.saturating_add(1);
        }
    }

    fn remember(&mut self, job: &Job, name: &str) {
        let path = job.dir.to_string_lossy().into_owned();
        self.recent.retain(|r| r.job != path);
        self.recent.insert(0, Recent { job: path, name: name.to_string(), at: now_ms() });
        self.recent.truncate(MAX_RECENT);
    }

    fn expected(&self, label: &str) -> u64 {
        self.durations.get(label).map(|l| l.ms).filter(|ms| *ms > 0).unwrap_or(DEFAULT_STEP_MS)
    }
}

/// How far an item is, 0–1: finished steps count whole, the running one by its elapsed time
/// against its learned time (never past 95 % of it), each step weighted by its learned time.
pub fn item_progress(item: &ItemState, memory: &Memory, now: u64) -> f32 {
    if item.status.is_final() {
        return 1.0;
    }
    let mut total = 0u64;
    let mut done = 0u64;
    for s in &item.steps {
        let w = memory.expected(&s.label);
        total = total.saturating_add(w);
        done = done.saturating_add(match s.status {
            Status::Pending => 0,
            Status::Running => s.started_at.map(|t| now.saturating_sub(t)).unwrap_or(0).min(w.saturating_mul(95) / 100),
            _ => w,
        });
    }
    if total == 0 { 0.0 } else { (done as f64 / total as f64).clamp(0.0, 1.0) as f32 }
}

/// The job running on the background thread.
struct Running {
    slot: Slot,
    /// When it started (epoch ms): steps that ended earlier are not learned again.
    since: u64,
}

pub struct Batch {
    pub draft: Pipeline,
    /// The job shown in the queue (the last one run or opened).
    pub job: Option<Job>,
    pub book: Option<Book>,
    pub summary: Option<Summary>,
    pub error: Option<String>,
    pub memory: Memory,
    /// The `filmcraft-cli` of the last run (Retry uses it again).
    pub cli: Option<String>,
    /// Where the section drew its drop zone last frame.
    pub drop_zone: Option<egui::Rect>,
    running: Option<Running>,
    read_at: Option<Instant>,
    loaded: bool,
}

impl Default for Batch {
    fn default() -> Self {
        Batch {
            draft: empty_draft(),
            job: None,
            book: None,
            summary: None,
            error: None,
            memory: Memory::default(),
            cli: None,
            drop_zone: None,
            running: None,
            read_at: None,
            loaded: false,
        }
    }
}

pub fn empty_draft() -> Pipeline {
    Pipeline {
        schema: SCHEMA.into(),
        name: String::new(),
        description: String::new(),
        inputs: Vec::new(),
        template: None,
        steps: Vec::new(),
        outputs: Vec::new(),
        on_failure: OnFailure::Skip,
        concurrency: 2,
        output_name: None,
        projects_folder: String::new(),
        outputs_folder: None,
        extensions: Vec::new(),
        resources: Default::default(),
    }
}

impl Batch {
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    fn read(&mut self) {
        self.read_at = Some(Instant::now());
        if let Some(job) = &self.job {
            match job.book() {
                Ok(b) => self.book = Some(b),
                Err(e) => self.error = Some(e),
            }
        }
    }

    fn load_once(&mut self, dir: Option<&Path>) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(path) = dir.map(memory_file) else { return };
        let small = std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_FILE);
        if let Some(m) = small.then(|| std::fs::read_to_string(&path).ok()).flatten().and_then(|t| serde_json::from_str::<Memory>(&t).ok()) {
            self.memory = m;
        }
    }

    fn save(&self, dir: Option<&Path>) {
        let Some(path) = dir.map(memory_file) else { return };
        let text = match serde_json::to_string_pretty(&self.memory) {
            Ok(t) => t,
            Err(e) => {
                log::warn!("batch memory: {e}");
                return;
            }
        };
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent).and_then(|()| std::fs::write(&path, text))
        {
            log::warn!("{}: {e}", path.display());
        }
    }

    /// Add inputs (files, folders, projects), skipping ones already listed.
    pub fn add_inputs(&mut self, paths: impl IntoIterator<Item = String>) -> usize {
        let mut n = 0;
        for p in paths {
            let p = pipeline::absolute(Path::new(&p)).to_string_lossy().into_owned();
            if !p.is_empty() && !self.draft.inputs.contains(&p) {
                self.draft.inputs.push(p);
                n += 1;
            }
        }
        n
    }
}

pub fn memory_file(data_dir: &Path) -> PathBuf {
    data_dir.join("bashcut").join("batch.json")
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.batch")
}

/// This app's batch state, created on first use.
pub fn shared(ctx: &egui::Context) -> Shared {
    if let Some(b) = ctx.data(|d| d.get_temp::<Shared>(key())) {
        return b;
    }
    let b: Shared = Arc::new(Mutex::new(Batch::default()));
    ctx.data_mut(|d| d.insert_temp(key(), b.clone()));
    b
}

pub fn lock(b: &Shared) -> MutexGuard<'_, Batch> {
    b.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Collect a finished run and re-read the book of a running job (called by `layout` every frame).
pub fn poll(ctx: &egui::Context, dir: Option<&Path>) {
    let Some(b) = ctx.data(|d| d.get_temp::<Shared>(key())) else { return };
    let mut g = lock(&b);
    g.load_once(dir);
    let finished = g.running.as_ref().and_then(|r| r.slot.lock().unwrap_or_else(PoisonError::into_inner).take());
    if let Some(result) = finished {
        let since = g.running.take().map(|r| r.since).unwrap_or(0);
        g.read();
        match result {
            Ok(summary) => g.summary = Some(summary),
            Err(e) => g.error = Some(e),
        }
        if let Some(book) = g.book.clone() {
            g.memory.learn(&book, since);
        }
        g.save(dir);
        return;
    }
    // a job run by the `bashcut-batch` command (opened from Recent) is watched the same way
    let live = g.running.is_some() || g.book.as_ref().is_some_and(|b| b.status == Status::Running);
    if live {
        if g.read_at.is_none_or(|t| t.elapsed() >= READ_EVERY) {
            g.read();
        }
        ctx.request_repaint_after(READ_EVERY);
    }
}

/// Files dropped on the Batch section become inputs instead of being imported into the open
/// project (the hook before `handle_drops` in `lib.rs`).
pub fn take_drops(app: &FilmcraftApp, ctx: &egui::Context) {
    let Some(b) = ctx.data(|d| d.get_temp::<Shared>(key())) else { return };
    let Some(zone) = lock(&b).drop_zone.take() else { return };
    if ctx.input(|i| i.raw.dropped_files.is_empty()) {
        return;
    }
    let Some(at) = app.drop_pointer(ctx) else { return };
    if !zone.contains(at) {
        return;
    }
    let paths: Vec<String> = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files)).iter().map(|f| f.path().to_string_lossy().into_owned()).collect();
    lock(&b).add_inputs(paths);
}

/// The draft as a pipeline that can run: paths absolute, a name, and the projects folder
/// defaulting to `<first input's folder>/batch`.
pub fn prepared(draft: &Pipeline) -> Result<Pipeline, String> {
    let mut p = draft.clone();
    let first = p.inputs.first().map(PathBuf::from).ok_or("add at least one input (files, a folder or projects)")?;
    let base = if first.is_dir() { first } else { first.parent().map(Path::to_path_buf).unwrap_or_default() };
    if p.projects_folder.trim().is_empty() {
        p.projects_folder = base.join("batch").to_string_lossy().into_owned();
    }
    if p.name.trim().is_empty() {
        p.name = "batch".into();
    }
    let text = serde_json::to_string(&p).map_err(|e| e.to_string())?;
    Pipeline::parse(&text, &pipeline::absolute(&base))
}

fn start(ctx: &egui::Context, g: &mut Batch, job: Job, opts: Options) -> Result<(), String> {
    let slot: Slot = Arc::new(Mutex::new(None));
    let (job2, slot2, ctx2) = (job.clone(), slot.clone(), ctx.clone());
    std::thread::Builder::new()
        .name("bashcut-batch".into())
        .spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bashcut_batch::run(&job2, &opts)))
                .unwrap_or_else(|_| Err("the batch runner stopped unexpectedly".into()));
            *slot2.lock().unwrap_or_else(PoisonError::into_inner) = Some(r);
            ctx2.request_repaint();
        })
        .map_err(|e| format!("batch runner: {e}"))?;
    g.running = Some(Running { slot, since: now_ms() });
    let name = job.book().map(|b| b.name).unwrap_or_default();
    g.memory.remember(&job, &name);
    g.job = Some(job);
    g.summary = None;
    g.error = None;
    g.read();
    ctx.request_repaint_after(READ_EVERY);
    Ok(())
}

fn options(app: &FilmcraftApp, cli: &str) -> Options {
    let mut o = Options::new(cli);
    // profile outputs are compiled into the app's user export presets
    o.data_dir = super::data_dir(app);
    o
}

fn cli_param(params: &Value, last: Option<&str>) -> Result<String, String> {
    params
        .get("cli")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .or_else(|| last.map(str::to_string))
        .or_else(plugins::cli_path)
        .ok_or_else(|| "filmcraft-cli was not found next to the app; pass `cli`".into())
}

fn str_p<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

fn index_p(p: &Value, k: &str, len: usize) -> Result<usize, String> {
    let i = p.get(k).and_then(Value::as_u64).ok_or(format!("`{k}` (number) is required"))?;
    usize::try_from(i).ok().filter(|i| *i < len).ok_or(format!("`{k}` {i} is out of range (0–{})", len.saturating_sub(1)))
}

/// A step from command params: `{command, params}`, `{plugin, action, params}`, `{recipe, brief}`
/// or `{step}` (a pipeline step as JSON); `label` overrides the shown name.
pub fn step_from(app: &mut FilmcraftApp, ctx: &egui::Context, p: &Value) -> Result<Step, String> {
    let mut step = if let Some(raw) = p.get("step").filter(|s| s.is_object()) {
        serde_json::from_value::<Step>(raw.clone()).map_err(|e| format!("step: {e}"))?
    } else if let Some(id) = str_p(p, "command") {
        if filmcraft_engine::commands::find(id).is_none() {
            return Err(format!("{id} is not an engine command; only engine commands run in a batch"));
        }
        let params = p.get("params").filter(|v| v.is_object()).cloned();
        Step { command: Some(id.to_string()), params, ..Step::default() }
    } else if let Some(plugin) = str_p(p, "plugin") {
        let action = str_p(p, "action").ok_or("`action` is required with `plugin`")?;
        plugin_step(app, ctx, plugin, action, p.get("params"))?
    } else if let Some(recipe) = str_p(p, "recipe") {
        let (id, title, _) = RECIPES.iter().find(|(id, ..)| *id == recipe).ok_or(format!("no recipe {recipe}"))?;
        let brief = str_p(p, "brief").unwrap_or_default().to_string();
        Step { label: Some(title.to_string()), agent: Some(AgentStep { skill: id.to_string(), brief, argv: Vec::new() }), ..Step::default() }
    } else {
        return Err("give `command`, `plugin` + `action`, `recipe` or `step`".into());
    };
    if let Some(label) = str_p(p, "label") {
        step.label = Some(label.to_string());
    }
    step.kind()?;
    Ok(step)
}

/// A plugin action as a batch step: its command line connected to the item's project, program
/// files made absolute (the step runs in the item's folder) and the plugin's non-secret settings
/// as environment. Secrets stay in the OS store, never in a pipeline file.
fn plugin_step(app: &mut FilmcraftApp, ctx: &egui::Context, plugin: &str, action: &str, params: Option<&Value>) -> Result<Step, String> {
    let (installed, values) = plugins::with(app, ctx, |h| (h.plugin(plugin).cloned(), h.settings(plugin).unwrap_or_default()));
    let installed = installed.ok_or(format!("{plugin} is not installed"))?;
    let m = installed.manifest.as_ref().ok_or(format!("{plugin} has no readable manifest"))?;
    let a = m.action(action).ok_or(format!("{plugin} has no action `{action}`"))?;
    let params: BTreeMap<String, Value> =
        params.and_then(Value::as_object).map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    let cli = plugins::cli_path();
    let argv = m.command_line(bashcut_plugin_host::os(), action, &Connection::Project("{project}".into()), cli.as_deref(), &params).map_err(|e| e.0)?;
    let argv = argv
        .into_iter()
        .map(|a| {
            let local = installed.dir.join(&a);
            if !a.is_empty() && !Path::new(&a).is_absolute() && local.exists() { local.to_string_lossy().into_owned() } else { a }
        })
        .collect();
    let env = settings::env_for(m, &values, &MemoryStore::default()).into_iter().collect();
    let name = if m.name.is_empty() { m.id.as_str() } else { m.name.as_str() };
    let title = if a.title.is_empty() { a.id.as_str() } else { a.title.trim_end_matches('…') };
    Ok(Step { label: Some(format!("{name} ▸ {title}")), plugin: Some(ProcessStep { argv, env }), ..Step::default() })
}

/// An output from `{profile, name?}` (the runner exports it with the profile's loudness and caption
/// rules), `{preset, name?}` or `{output}` (a pipeline output as JSON).
fn output_from(app: &mut FilmcraftApp, p: &Value) -> Result<Output, String> {
    if let Some(raw) = p.get("output").filter(|o| o.is_object()) {
        return serde_json::from_value(raw.clone()).map_err(|e| format!("output: {e}"));
    }
    let name = str_p(p, "name").map(str::to_string);
    let blank = |name: String| Output { name, preset: None, format: None, settings: None, profile: None, timeout_seconds: None };
    if let Some(profile) = str_p(p, "profile") {
        let r = app.session.execute("bashcut.profiles.get", json!({"profile": profile})).map_err(|e| e.to_string())?;
        let id = r.get("id").and_then(Value::as_str).unwrap_or(profile).to_string();
        let ext = if r.get("codec").and_then(Value::as_str).is_some_and(|c| c.to_ascii_lowercase().contains("prores")) { "mov" } else { "mp4" };
        let name = name.unwrap_or_else(|| format!("{{name}}-{}.{ext}", pipeline::safe_name(&id)));
        return Ok(Output { profile: Some(id), ..blank(name) });
    }
    if let Some(preset) = str_p(p, "preset") {
        return Ok(Output { preset: Some(preset.to_string()), ..blank(name.unwrap_or_else(|| "{name}.mp4".into())) });
    }
    Err("give `profile`, `preset` or `output`".into())
}

fn item<'a>(book: Option<&'a Book>, p: &Value) -> Result<&'a ItemState, String> {
    let name = str_p(p, "item").ok_or("`item` (name) is required")?;
    book.and_then(|b| b.items.iter().find(|i| i.name == name)).ok_or(format!("no item {name} in this batch"))
}

fn state_json(g: &Batch) -> Value {
    let now = now_ms();
    let items: Vec<Value> = g
        .book
        .iter()
        .flat_map(|b| b.items.iter())
        .map(|i| {
            let current = i.steps.iter().find(|s| s.status == Status::Running).map(|s| s.label.clone());
            json!({"name": i.name, "status": i.status, "progress": item_progress(i, &g.memory, now), "step": current,
                   "error": i.error, "outputs": i.outputs(), "project": i.project})
        })
        .collect();
    json!({
        "draft": g.draft,
        "running": g.is_running(),
        "job": g.job.as_ref().map(|j| j.dir.to_string_lossy().into_owned()),
        "status": g.book.as_ref().map(|b| b.status),
        "counts": g.book.as_ref().map(Book::counts),
        "items": items,
        "summary": g.summary,
        "error": g.error,
    })
}

/// `bashcut.batch.<name>`.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, name: &str, p: &Value) -> Result<Value, String> {
    let b = shared(ctx);
    // steps and outputs may call into the plugin host or the session: build them unlocked
    let built_step = match name {
        "addStep" | "prefill" | "setStep" => Some(step_from(app, ctx, p)?),
        _ => None,
    };
    let built_output = if name == "addOutput" { Some(output_from(app, p)?) } else { None };
    let dir = super::data_dir(app);
    let mut g = lock(&b);
    g.load_once(dir.as_deref());
    let steps = g.draft.steps.len();
    match name {
        "state" => Ok(state_json(&g)),
        "draft" => {
            if let Some(d) = p.get("draft").filter(|d| d.is_object()) {
                let mut next: Pipeline = serde_json::from_value(d.clone()).map_err(|e| format!("draft: {e}"))?;
                next.schema = SCHEMA.into();
                g.draft = next;
            }
            Ok(json!(g.draft))
        }
        "clear" => {
            g.draft = empty_draft();
            Ok(json!(g.draft))
        }
        "addInput" => {
            let paths: Vec<String> = match p.get("paths").and_then(Value::as_array) {
                Some(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
                None => str_p(p, "path").map(|s| vec![s.to_string()]).ok_or("`paths` (array) is required")?,
            };
            let added = g.add_inputs(paths);
            Ok(json!({"added": added, "inputs": g.draft.inputs}))
        }
        "removeInput" => {
            let i = index_p(p, "index", g.draft.inputs.len())?;
            g.draft.inputs.remove(i);
            Ok(json!(g.draft.inputs))
        }
        "addStep" | "prefill" => {
            if let Some(s) = built_step {
                g.draft.steps.push(s);
            }
            if name == "prefill" {
                super::State::update(ctx, |st| st.show("batch"));
            }
            Ok(json!({"index": g.draft.steps.len().saturating_sub(1), "steps": g.draft.steps}))
        }
        "setStep" => {
            let i = index_p(p, "index", steps)?;
            if let (Some(s), Some(slot)) = (built_step, g.draft.steps.get_mut(i)) {
                *slot = s;
            }
            Ok(json!(g.draft.steps))
        }
        "removeStep" => {
            let i = index_p(p, "index", steps)?;
            g.draft.steps.remove(i);
            Ok(json!(g.draft.steps))
        }
        "moveStep" => {
            let i = index_p(p, "index", steps)?;
            let to = index_p(p, "to", steps)?;
            let s = g.draft.steps.remove(i);
            g.draft.steps.insert(to, s);
            Ok(json!(g.draft.steps))
        }
        "addOutput" => {
            if let Some(o) = built_output {
                g.draft.outputs.push(o);
            }
            Ok(json!(g.draft.outputs))
        }
        "removeOutput" => {
            let i = index_p(p, "index", g.draft.outputs.len())?;
            g.draft.outputs.remove(i);
            Ok(json!(g.draft.outputs))
        }
        "set" => {
            if let Some(n) = p.get("name").and_then(Value::as_str) {
                g.draft.name = n.to_string();
            }
            if let Some(c) = p.get("concurrency").and_then(Value::as_u64) {
                g.draft.concurrency = usize::try_from(c).unwrap_or(1).clamp(1, pipeline::MAX_CONCURRENCY);
            }
            match p.get("onFailure").and_then(Value::as_str) {
                Some("skip") => g.draft.on_failure = OnFailure::Skip,
                Some("stop") => g.draft.on_failure = OnFailure::Stop,
                Some(other) => return Err(format!("onFailure must be skip or stop (got {other})")),
                None => {}
            }
            if let Some(f) = p.get("projectsFolder").and_then(Value::as_str) {
                g.draft.projects_folder = f.to_string();
            }
            if let Some(f) = p.get("outputsFolder") {
                g.draft.outputs_folder = f.as_str().filter(|f| !f.trim().is_empty()).map(str::to_string);
            }
            if let Some(f) = p.get("outputName") {
                g.draft.output_name = f.as_str().filter(|f| !f.trim().is_empty()).map(str::to_string);
            }
            Ok(json!(g.draft))
        }
        "run" => {
            if g.is_running() {
                return Err("a batch is already running".into());
            }
            let cli = cli_param(p, None)?;
            let pipeline = prepared(&g.draft)?;
            let job = Job::create(&pipeline, None)?;
            let dir_text = job.dir.to_string_lossy().into_owned();
            g.cli = Some(cli.clone());
            start(ctx, &mut g, job, options(app, &cli))?;
            g.save(dir.as_deref());
            Ok(json!({"job": dir_text, "items": g.book.as_ref().map(|b| b.items.len())}))
        }
        "cancel" => {
            let job = g.job.as_ref().ok_or("no batch to cancel")?;
            job.request_cancel()?;
            Ok(json!({"cancelling": job.dir.to_string_lossy()}))
        }
        "retry" => {
            if g.is_running() {
                return Err("wait for the batch to finish (or cancel it) before retrying".into());
            }
            let job = g.job.clone().ok_or("no batch to retry")?;
            if let Some(pid) = job.runner_alive() {
                return Err(format!("this batch is still running in process {pid}"));
            }
            let names: Vec<String> =
                p.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
            let names = if names.is_empty() { str_p(p, "item").map(|s| vec![s.to_string()]).unwrap_or_default() } else { names };
            let reopened = job.retry(&names)?;
            let cli = cli_param(p, g.cli.clone().as_deref())?;
            g.cli = Some(cli.clone());
            start(ctx, &mut g, job, options(app, &cli))?;
            g.save(dir.as_deref());
            Ok(json!({"reopened": reopened}))
        }
        "open" => {
            let project = item(g.book.as_ref(), p)?.project.clone();
            if !Path::new(&project).is_file() {
                return Err(format!("{project} was not made (its create step did not finish)"));
            }
            drop(g);
            crate::menus::invoke(app, ctx, "file.open", json!({"path": project}))
        }
        "reveal" => {
            let path = match str_p(p, "path") {
                Some(path) => path.to_string(),
                None if p.get("item").is_some() => {
                    let i = item(g.book.as_ref(), p)?;
                    i.outputs().into_iter().rev().find(|o| Path::new(o).exists()).unwrap_or_else(|| i.project.clone())
                }
                None => g.job.as_ref().map(|j| j.dir.to_string_lossy().into_owned()).ok_or("nothing to reveal")?,
            };
            drop(g);
            crate::panels::menu_dialogs::open_path(app, ctx, &path, true)?;
            Ok(json!(path))
        }
        "presets" => Ok(json!(dir.as_deref().map(presets::list).unwrap_or_default())),
        "savePreset" => {
            let d = dir.as_deref().ok_or("presets need a data folder")?;
            let preset = str_p(p, "name").ok_or("`name` is required")?;
            let text = serde_json::to_string_pretty(&prepared(&g.draft)?).map_err(|e| e.to_string())?;
            let path = presets::save(d, preset, &text)?;
            Ok(json!(path.to_string_lossy()))
        }
        "loadPreset" => {
            let d = dir.as_deref().ok_or("presets need a data folder")?;
            let preset = str_p(p, "name").ok_or("`name` is required")?;
            g.draft = presets::load(d, preset, &pipeline::absolute(Path::new(".")))?;
            Ok(json!(g.draft))
        }
        "deletePreset" => {
            let d = dir.as_deref().ok_or("presets need a data folder")?;
            presets::delete(d, str_p(p, "name").ok_or("`name` is required")?)?;
            Ok(Value::Null)
        }
        "recent" => Ok(json!(g.memory.recent)),
        "openJob" => {
            if g.is_running() {
                return Err("a batch is running; wait for it or cancel it first".into());
            }
            let job = Job::open(str_p(p, "job").ok_or("`job` (folder) is required")?)?;
            if p.get("asDraft").and_then(Value::as_bool).unwrap_or(false) {
                g.draft = job.pipeline()?;
            }
            g.job = Some(job);
            g.summary = None;
            g.error = None;
            g.read();
            Ok(state_json(&g))
        }
        "durations" => Ok(json!(g.memory.durations)),
        _ => Err(format!("unknown command bashcut.batch.{name}")),
    }
}

pub fn commands() -> Vec<CommandInfo> {
    [
        ("state", "Batch State"),
        ("draft", "Batch Draft"),
        ("clear", "Clear Batch Draft"),
        ("addInput", "Add Batch Inputs"),
        ("removeInput", "Remove Batch Input"),
        ("addStep", "Add Batch Step"),
        ("setStep", "Change Batch Step"),
        ("removeStep", "Remove Batch Step"),
        ("moveStep", "Move Batch Step"),
        ("addOutput", "Add Batch Output"),
        ("removeOutput", "Remove Batch Output"),
        ("set", "Batch Settings"),
        ("run", "Run Batch"),
        ("cancel", "Cancel Batch"),
        ("retry", "Retry Batch Items"),
        ("open", "Open Batch Item"),
        ("reveal", "Reveal Batch Output"),
        ("presets", "List Batch Presets"),
        ("savePreset", "Save Batch Preset"),
        ("loadPreset", "Load Batch Preset"),
        ("deletePreset", "Delete Batch Preset"),
        ("recent", "Recent Batches"),
        ("openJob", "Open Batch Job"),
        ("prefill", "Run on Many Files"),
        ("durations", "Learned Batch Step Times"),
    ]
    .into_iter()
    .map(|(id, label)| CommandInfo::new(&format!("bashcut.batch.{id}"), label, &[], None))
    .collect()
}

/// A palette item's or tool call's command as `prefill` params, when it can run in a batch.
pub fn prefill_params(command: &str, params: &Value) -> Option<Value> {
    if command == "bashcut.plugins.run" {
        let mut m = Map::new();
        m.insert("plugin".into(), params.get("plugin")?.clone());
        m.insert("action".into(), params.get("action")?.clone());
        m.insert("params".into(), params.get("params").cloned().unwrap_or(json!({})));
        return Some(Value::Object(m));
    }
    filmcraft_engine::commands::find(command).map(|_| json!({"command": command, "params": params}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bashcut_batch::book::StepState;

    #[test]
    fn learned_times_average_and_weight_progress() {
        let dir = std::env::temp_dir().join(format!("bashcut-ui-batch-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.mov"), b"x").unwrap();
        let mut draft = empty_draft();
        draft.inputs = vec![dir.join("a.mov").to_string_lossy().into_owned()];
        draft.steps.push(Step { command: Some("edit.undo".into()), ..Step::default() });
        let p = prepared(&draft).unwrap();
        assert_eq!(p.projects_dir(), dir.join("batch"));
        let job = Job::create(&p, Some("t")).unwrap();
        let mut book = job.book().unwrap();
        let item = &mut book.items[0];
        let set = |s: &mut StepState, st: Status, start: u64, ms: u64| {
            s.status = st;
            s.started_at = Some(start);
            s.ended_at = Some(start + ms);
            s.duration_ms = Some(ms);
        };
        set(&mut item.steps[0], Status::Done, 1_000, 2_000);
        set(&mut item.steps[1], Status::Done, 3_000, 6_000);
        let mut m = Memory::default();
        m.learn(&book, 0);
        m.learn(&book, 0);
        assert_eq!(m.durations["Create project"], Learned { ms: 2_000, runs: 2 });
        m.learn(&book, 999_999);
        assert_eq!(m.durations["Create project"].runs, 2, "steps that ended before the run are not learned again");

        let item = &mut book.items[0];
        item.status = Status::Running;
        item.steps[1].status = Status::Running;
        item.steps[1].ended_at = None;
        // create done (2 s of 8 s), the edit 3 s into its 6 s: 5 / 8
        let p = item_progress(item, &m, 6_000);
        assert!((p - 5.0 / 8.0).abs() < 0.01, "{p}");
        // a running step never counts past 95 %
        let p = item_progress(item, &m, 1_000_000);
        assert!(p < 1.0 && p > 0.9, "{p}");
        item.status = Status::Failed;
        assert_eq!(item_progress(item, &m, 0), 1.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepared_needs_inputs_and_prefill_takes_engine_commands_and_plugin_runs() {
        assert!(prepared(&empty_draft()).is_err());
        assert!(prefill_params("bashcut.ui.focusAgent", &json!({})).is_none());
        let v = prefill_params("bashcut.plugins.run", &json!({"plugin": "p", "action": "a"})).unwrap();
        assert_eq!(v, json!({"plugin": "p", "action": "a", "params": {}}));
        let id = filmcraft_engine::commands::command_specs().first().map(|c| c.id).unwrap();
        assert_eq!(prefill_params(id, &json!({"x": 1})).unwrap()["command"], json!(id));
    }
}
