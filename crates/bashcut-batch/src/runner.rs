//! The runner: up to `concurrency` items at once, each step of an item in its own process.
//!
//! Engine steps run as `filmcraft-cli run <step>.jsonl --project <item>.fcproj --save`: the CLI
//! stops at the first failing line before saving, so a step is all or nothing and every finished
//! step is on disk. Exports run as `filmcraft-cli export <out> --project … --preset …`. Plugin and
//! agent steps run their own command line with the item's placeholders. The app does not need to
//! be open. One runner per job at a time (`runner.lock`).

use crate::agent;
use crate::book::{self, AgentUsage, Book, Category, ErrorInfo, Job, Status, StepRef, StepState, now_ms};
use crate::governor::{self, KeepAwake, Meter, Probe, SystemProbe};
use crate::pipeline::{self, AgentStep, CommandLine, OnFailure, Pipeline, StepKind, Vars};
use crate::process::{self, Ended, Spec};
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How much of a step's stdout / stderr goes into its error message.
const LOG_TAIL: u64 = 4096;
const MAX_MESSAGE: usize = 600;

#[derive(Clone, Debug)]
pub struct Options {
    /// The engine to run: `bashcutpro` or `filmcraft-cli` ([`crate::cli`]).
    pub cli: PathBuf,
    /// Overrides the pipeline's `concurrency`.
    pub concurrency: Option<usize>,
    /// Passed to filmcraft-cli as `--data-dir` (user export presets).
    pub data_dir: Option<PathBuf>,
    /// How often a waiting runner checks its processes, cancel and timeouts.
    pub poll: Duration,
    /// Extra environment for every process the runner starts.
    pub env: Vec<(String, String)>,
    /// Reads free RAM, cores, disk and process memory (#61; tests pass a [`crate::FakeProbe`]).
    pub probe: Arc<dyn Probe>,
    /// Keep the machine from idle-sleeping while the batch runs (the command line turns it on).
    pub keep_awake: bool,
    /// How often a step's process memory is checked against the cap.
    pub watch_every: Duration,
    /// `bashcut-mcp`, the MCP server agent steps get (writes limited to the item folder and the
    /// outputs, #51); without it they get `filmcraft-cli mcp --project` directly.
    pub mcp: Option<PathBuf>,
    /// Overrides the pipeline's `agentConcurrency`.
    pub agent_concurrency: Option<usize>,
    /// The speech model for transcribe steps that name none; else picked from Settings and the
    /// downloaded models ([`crate::speech`], #172).
    pub speech_model: Option<String>,
    /// The agent kit (bashcutpro-agent-kit): `{kit}` in steps, `--plugin-dir` and script tools for
    /// agent steps, `BASHCUT_KIT` for plugins and agents (#204).
    pub kit: Option<PathBuf>,
}

impl Options {
    pub fn new(cli: impl Into<PathBuf>) -> Self {
        Options {
            cli: cli.into(),
            concurrency: None,
            data_dir: None,
            poll: Duration::from_millis(200),
            env: Vec::new(),
            probe: Arc::new(SystemProbe::default()),
            keep_awake: false,
            watch_every: Duration::from_secs(2),
            mcp: None,
            agent_concurrency: None,
            speech_model: None,
            kit: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub job: String,
    pub status: Status,
    pub counts: book::Counts,
    /// Agent steps' turns, tokens, cost and time.
    pub agent: book::AgentTotals,
    /// Tripped steps and other things a person should look at.
    pub reports: Vec<String>,
    pub elapsed_ms: u64,
    /// Peak memory, time per step, throughput (#61).
    pub resources: governor::Report,
}

impl Summary {
    pub fn of(job: &Job, book: &Book, reports: Vec<String>, elapsed_ms: u64, resources: governor::Report) -> Summary {
        Summary {
            id: book.id.clone(),
            job: job.dir.to_string_lossy().into_owned(),
            status: book.status,
            counts: book.counts(),
            agent: book.agent_totals(),
            reports,
            elapsed_ms,
            resources,
        }
    }
}

/// Run (or resume) a job until every item is finished, the batch stops after a failure
/// (`onFailure: stop`) or it is cancelled. Steps a previous runner left running are tripped, not
/// re-run (see [`crate::book`]).
pub fn run(job: &Job, opts: &Options) -> Result<Summary, String> {
    let start = Instant::now();
    let _lock = Lock::acquire(job)?;
    let pipeline = job.pipeline()?;
    let mut book = job.book()?;
    // a new run clears an old cancel request
    let _ = std::fs::remove_file(job.cancel_path());
    // steps may write into `{outputs}` (an SRT, a report) before any export makes the folder
    let _ = std::fs::create_dir_all(pipeline.outputs_dir());
    let mut reports = book.trip_running();
    for item in &mut book.items {
        if item.status == Status::NotRun {
            item.status = Status::Pending;
        }
    }
    book.runner_pid = Some(std::process::id());
    book.status = Status::Running;
    book.updated_at = now_ms();
    job.save(&book)?;

    let pending = book.items.iter().filter(|i| i.status == Status::Pending).count();
    let max_items = opts.concurrency.unwrap_or(pipeline.concurrency).clamp(1, pipeline::MAX_CONCURRENCY);
    let workers = max_items.min(pending.max(1));
    // what the machine allows now; workers above it wait until RAM frees up (re-checked per item)
    let first = governor::allowed(opts.probe.as_ref(), &pipeline.resources, workers);
    let _awake = opts.keep_awake.then(KeepAwake::start);
    let shared = Shared {
        job,
        pipeline: &pipeline,
        opts,
        book: Mutex::new(book),
        stop: AtomicBool::new(false),
        cancel: AtomicBool::new(false),
        save_error: Mutex::new(None),
        max_items: workers,
        meter: Meter::default(),
        agents: Mutex::new(0),
        agents_free: Condvar::new(),
        agent_cap: opts.agent_concurrency.unwrap_or(pipeline.agent_concurrency).clamp(1, pipeline::MAX_CONCURRENCY),
        speech_model: speech_model(&pipeline, opts),
    };
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                // a bug in one worker must not take the batch down: stop handing out items instead
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker(&shared))).is_err() {
                    shared.stop.store(true, Ordering::Relaxed);
                }
            });
        }
    });

    let cancelled = shared.cancelled();
    let save_error = shared.save_error.lock().unwrap_or_else(PoisonError::into_inner).take();
    let mut book = shared.book.into_inner().unwrap_or_else(PoisonError::into_inner);
    reports.extend(book.trip_running());
    for item in &mut book.items {
        if item.status == Status::Pending {
            item.status = if cancelled { Status::Cancelled } else { Status::NotRun };
        }
    }
    for item in &book.items {
        if item.status == Status::NotRun && item.error.as_ref().is_some_and(|e| e.category == Category::Resources) {
            reports.push(format!("{}: stopped over the memory cap; resume runs it again (fewer items at once when RAM is short)", item.name));
        }
    }
    book.status = book.derive_status(cancelled);
    book.runner_pid = None;
    book.paused = None;
    book.updated_at = now_ms();
    job.save(&book)?;
    if let Some(e) = save_error {
        reports.push(format!("the job book could not be saved during the run: {e}"));
    }
    let elapsed = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let resources = shared.meter.report(elapsed, first);
    Ok(Summary::of(job, &book, reports, elapsed, resources))
}

/// `runner.lock` holds the runner's pid while it runs; a lock whose process is gone is taken over.
struct Lock {
    path: PathBuf,
}

impl Lock {
    fn acquire(job: &Job) -> Result<Lock, String> {
        let path = job.lock_path();
        for _ in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return Ok(Lock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Some(pid) = job.runner_alive().filter(|p| *p != std::process::id()) {
                        return Err(format!("job is already running (runner pid {pid})"));
                    }
                    let _ = std::fs::remove_file(&path);
                }
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        }
        Err(format!("{}: could not take the runner lock", path.display()))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

struct Shared<'a> {
    job: &'a Job,
    pipeline: &'a Pipeline,
    opts: &'a Options,
    book: Mutex<Book>,
    stop: AtomicBool,
    cancel: AtomicBool,
    save_error: Mutex<Option<String>>,
    /// Items at once at most (the governor may allow fewer).
    max_items: usize,
    meter: Meter,
    /// Agent steps running now, at most `agent_cap` (`agentConcurrency`).
    agents: Mutex<usize>,
    agents_free: Condvar,
    agent_cap: usize,
    /// `model` for transcribe steps that name none ([`crate::speech`]).
    speech_model: Option<String>,
}

/// What a worker gets when it asks for an item.
enum Claim {
    Item(String),
    /// Items are pending, but the machine has no room for one more now.
    Wait,
    None,
}

/// One of the `agentConcurrency` slots, given back on drop.
struct AgentSlot<'a, 'b> {
    sh: &'a Shared<'b>,
}

impl<'a, 'b> AgentSlot<'a, 'b> {
    /// Wait for a free slot; `None` when the batch is cancelled meanwhile.
    fn take(sh: &'a Shared<'b>) -> Option<Self> {
        let mut n = sh.agents.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if sh.cancelled() {
                return None;
            }
            if *n < sh.agent_cap {
                *n += 1;
                return Some(AgentSlot { sh });
            }
            n = match sh.agents_free.wait_timeout(n, sh.opts.poll) {
                Ok((g, _)) => g,
                Err(e) => e.into_inner().0,
            };
        }
    }
}

impl Drop for AgentSlot<'_, '_> {
    fn drop(&mut self) {
        let mut n = self.sh.agents.lock().unwrap_or_else(PoisonError::into_inner);
        *n = n.saturating_sub(1);
        drop(n);
        self.sh.agents_free.notify_all();
    }
}

impl Shared<'_> {
    /// Change the book and save it.
    fn with<R>(&self, f: impl FnOnce(&mut Book) -> R) -> R {
        let mut b = self.book.lock().unwrap_or_else(PoisonError::into_inner);
        let r = f(&mut b);
        b.updated_at = now_ms();
        if let Err(e) = self.job.save(&b) {
            *self.save_error.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
        }
        r
    }

    /// Read the book without saving it.
    fn peek<R>(&self, f: impl FnOnce(&Book) -> R) -> R {
        f(&self.book.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The next pending item, if the governor lets one more start now (saved only when claimed).
    fn claim(&self) -> Claim {
        let mut b = self.book.lock().unwrap_or_else(PoisonError::into_inner);
        if !b.items.iter().any(|i| i.status == Status::Pending) {
            return Claim::None;
        }
        let running = b.items.iter().filter(|i| i.status == Status::Running).count();
        let since = now_ms().saturating_sub(governor::SETTLE_MS);
        let settling = b.items.iter().filter(|i| i.status == Status::Running && i.started_at.is_some_and(|t| t >= since)).count();
        if !governor::may_start(self.opts.probe.as_ref(), &self.pipeline.resources, self.max_items, running, settling) {
            return Claim::Wait;
        }
        let Some(item) = b.items.iter_mut().find(|i| i.status == Status::Pending) else { return Claim::None };
        item.status = Status::Running;
        item.error = None;
        item.started_at = Some(now_ms());
        item.ended_at = None;
        let name = item.name.clone();
        b.updated_at = now_ms();
        if let Err(e) = self.job.save(&b) {
            *self.save_error.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
        }
        Claim::Item(name)
    }

    fn step<R>(&self, idx: usize, s: usize, f: impl FnOnce(&mut StepState) -> R) -> Option<R> {
        self.with(|b| b.items.get_mut(idx).and_then(|i| i.steps.get_mut(s)).map(f))
    }

    fn cancelled(&self) -> bool {
        if self.cancel.load(Ordering::Relaxed) {
            return true;
        }
        if self.job.cancel_requested() {
            self.cancel.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

fn worker(sh: &Shared) {
    loop {
        if sh.cancelled() || sh.stopping() {
            return;
        }
        let name = match sh.claim() {
            Claim::Item(name) => name,
            Claim::Wait => {
                std::thread::sleep(sh.opts.poll.max(Duration::from_millis(10)));
                continue;
            }
            Claim::None => return,
        };
        let idx = sh.with(|b| b.items.iter().position(|i| i.name == name));
        if let Some(idx) = idx {
            run_item(sh, idx);
        }
    }
}

/// What one item's steps need.
struct Ctx {
    name: String,
    input: String,
    input_is_project: bool,
    project: PathBuf,
    dir: PathBuf,
    vars: Vars,
}

fn run_item(sh: &Shared, idx: usize) {
    let snapshot = sh.with(|b| {
        b.items.get(idx).map(|i| {
            let steps: Vec<(StepRef, Status, Option<ErrorInfo>)> = i.steps.iter().map(|s| (s.step, s.status, s.error.clone())).collect();
            (i.name.clone(), i.input.clone(), i.input_is_project, i.project.clone(), steps)
        })
    });
    let Some((name, input, input_is_project, project, steps)) = snapshot else { return };
    let dir = sh.job.item_dir(&name);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        finish_item(sh, idx, Status::Failed, Some(ErrorInfo::new(Category::Io, format!("{}: {e}", dir.display()))));
        return;
    }
    let vars = Vars(vec![
        ("input", input.clone()),
        ("media", input.clone()),
        ("name", name.clone()),
        ("project", project.clone()),
        ("itemDir", dir.to_string_lossy().into_owned()),
        ("outputs", sh.pipeline.outputs_dir().to_string_lossy().into_owned()),
        ("cli", cli(sh)),
        ("plugins", plugins_dir(sh).to_string_lossy().into_owned()),
        ("kit", sh.opts.kit.as_deref().map(path_arg).unwrap_or_default()),
    ]);
    let ctx = Ctx { name, input, input_is_project, project: PathBuf::from(project), dir, vars };

    for (s, (step_ref, status, error)) in steps.into_iter().enumerate() {
        match status {
            Status::Done => continue,
            Status::Tripped => {
                finish_item(sh, idx, Status::NeedsAttention, error);
                return;
            }
            _ => {}
        }
        if sh.cancelled() {
            sh.step(idx, s, |st| st.status = Status::Cancelled);
            finish_item(sh, idx, Status::Cancelled, Some(ErrorInfo::new(Category::Cancelled, "cancelled")));
            return;
        }
        if sh.stopping() {
            finish_item(sh, idx, Status::NotRun, None);
            return;
        }
        if matches!(step_ref, StepRef::Create | StepRef::Export(_)) && !wait_for_disk(sh, matches!(step_ref, StepRef::Export(_))) {
            sh.step(idx, s, |st| st.status = Status::Cancelled);
            finish_item(sh, idx, Status::Cancelled, Some(ErrorInfo::new(Category::Cancelled, "cancelled")));
            return;
        }
        let started = now_ms();
        sh.step(idx, s, |st| {
            st.status = Status::Running;
            st.attempts = st.attempts.saturating_add(1);
            st.started_at = Some(started);
            st.ended_at = None;
            st.duration_ms = None;
            st.error = None;
            st.pid = None;
            st.outputs.clear();
        });
        let result = run_step(sh, idx, s, step_ref, &ctx);
        let ended = now_ms();
        match result {
            Ok(outputs) => {
                let label = sh.peek(|b| b.items.get(idx).and_then(|i| i.steps.get(s)).map(|x| x.label.clone())).unwrap_or_default();
                sh.meter.step(&label, ended.saturating_sub(started));
                sh.step(idx, s, |st| {
                    st.status = Status::Done;
                    st.outputs = outputs;
                    st.ended_at = Some(ended);
                    st.duration_ms = Some(ended.saturating_sub(started));
                    st.pid = None;
                });
            }
            Err(e) => {
                let (step_status, item_status) = match e.category {
                    Category::Cancelled => (Status::Cancelled, Status::Cancelled),
                    // over the memory cap: nothing is wrong with the step; it runs again later
                    Category::Resources => (Status::Pending, Status::NotRun),
                    Category::Handoff => (Status::Failed, Status::NeedsAttention),
                    Category::Review => (Status::Failed, Status::NeedsFix),
                    _ => (Status::Failed, Status::Failed),
                };
                let err = e.clone();
                sh.step(idx, s, |st| {
                    st.status = step_status;
                    st.error = Some(err);
                    st.ended_at = Some(ended);
                    st.duration_ms = Some(ended.saturating_sub(started));
                    st.pid = None;
                });
                finish_item(sh, idx, item_status, Some(e));
                if item_status == Status::Failed && sh.pipeline.on_failure == OnFailure::Stop {
                    sh.stop.store(true, Ordering::Relaxed);
                }
                return;
            }
        }
    }
    finish_item(sh, idx, Status::Done, None);
    sh.meter.item_done();
}

/// Wait while the projects or outputs disk is low (the book says why in `paused`). False when the
/// batch is cancelled while waiting.
fn wait_for_disk(sh: &Shared, export: bool) -> bool {
    let paths = [sh.pipeline.projects_dir(), sh.pipeline.outputs_dir()];
    let pause = (sh.opts.poll * 5).clamp(Duration::from_millis(10), Duration::from_secs(2));
    loop {
        let low = governor::disk_low(sh.opts.probe.as_ref(), &sh.pipeline.resources, &paths, export);
        let shown = sh.peek(|b| b.paused.clone());
        if shown != low {
            let l = low.clone();
            sh.with(|b| b.paused = l);
        }
        if low.is_none() {
            return true;
        }
        if sh.cancelled() {
            return false;
        }
        std::thread::sleep(pause);
    }
}

fn finish_item(sh: &Shared, idx: usize, status: Status, error: Option<ErrorInfo>) {
    sh.with(|b| {
        if let Some(item) = b.items.get_mut(idx) {
            let now = now_ms();
            item.status = status;
            item.error = error;
            item.ended_at = Some(now);
            item.duration_ms = item.started_at.map(|s| now.saturating_sub(s));
        }
    });
}

fn run_step(sh: &Shared, idx: usize, s: usize, step: StepRef, ctx: &Ctx) -> Result<Vec<String>, ErrorInfo> {
    let gone = || ErrorInfo::new(Category::Io, "the job's pipeline has no such step (pipeline.json changed?)");
    match step {
        StepRef::Create => create(sh, idx, s, ctx).map(|()| Vec::new()),
        StepRef::Step(i) => {
            let step = sh.pipeline.steps.get(i).ok_or_else(gone)?;
            let key = format!("step-{}", i.saturating_add(1));
            let timeout = step.timeout_seconds.map(Duration::from_secs);
            let kind = step.kind().map_err(|e| ErrorInfo::new(Category::Usage, e))?;
            match kind {
                StepKind::Command(id, params) => {
                    let line = CommandLine { id: id.to_string(), params: params.cloned().unwrap_or_else(|| Value::Object(serde_json::Map::new())) };
                    engine_step(sh, idx, s, ctx, &key, std::slice::from_ref(&line), timeout)
                }
                StepKind::Commands(lines) => engine_step(sh, idx, s, ctx, &key, lines, timeout),
                StepKind::Plugin(p) => {
                    if ctx.vars.get("kit").is_none() && p.argv.iter().chain(p.env.values()).any(|a| a.contains("{kit}")) {
                        return Err(ErrorInfo::new(
                            Category::Usage,
                            format!("{key}: this step runs a script of the agent kit; pass --kit <bashcutpro-agent-kit folder> or set BASHCUT_KIT"),
                        ));
                    }
                    let argv = crate::python::resolve(p.argv.iter().map(|a| ctx.vars.expand(a)).collect());
                    let mut env: Vec<(String, String)> = p.env.iter().map(|(k, v)| (k.clone(), ctx.vars.expand(v))).collect();
                    env.extend(item_env(sh, ctx));
                    let spec = Spec { argv, cwd: Some(ctx.dir.clone()), env, timeout, log: ctx.dir.join(&key) };
                    let ended = spawn(sh, idx, s, &spec)?;
                    outcome(ended, &spec.log, &key).map(|()| Vec::new())
                }
                StepKind::Agent(a) => agent_step(sh, idx, s, ctx, &key, a),
            }
        }
        StepRef::Export(i) => {
            let o = sh.pipeline.outputs.get(i).ok_or_else(gone)?;
            let key = format!("export-{}", i.saturating_add(1));
            let rel = ctx.vars.expand(&o.name);
            if !is_relative_inside(Path::new(&rel)) {
                return Err(ErrorInfo::new(Category::Usage, format!("{key}: output name must stay inside the outputs folder: {rel}")));
            }
            let out = sh.pipeline.outputs_dir().join(&rel);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| ErrorInfo::new(Category::Io, format!("{}: {e}", parent.display())))?;
            }
            let timeout = o.timeout_seconds.map(Duration::from_secs);
            if let Some(profile) = &o.profile {
                // the profile exports (and applies its loudness and captions) in one engine command
                let line =
                    CommandLine { id: "bashcut.profiles.apply".into(), params: serde_json::json!({"profile": profile, "path": path_arg(&out), "wait": true}) };
                let script = write_script(ctx, &key, std::slice::from_ref(&line))?;
                let mut argv = vec![cli(sh), "run".into(), path_arg(&script), "--project".into(), path_arg(&ctx.project), "--compact".into()];
                argv.extend(data_dir_args(sh));
                let spec = Spec { argv, cwd: Some(ctx.dir.clone()), env: sh.opts.env.clone(), timeout, log: ctx.dir.join(&key) };
                let ended = spawn(sh, idx, s, &spec)?;
                outcome(ended, &spec.log, &key)?;
                return Ok(vec![out.to_string_lossy().into_owned()]);
            }
            let mut argv = vec![cli(sh), "export".into(), path_arg(&out), "--project".into(), path_arg(&ctx.project), "--compact".into()];
            if let Some(p) = &o.preset {
                argv.extend(["--preset".into(), p.clone()]);
            }
            if let Some(f) = &o.format {
                argv.extend(["--format".into(), f.clone()]);
            }
            if let Some(v) = &o.settings {
                argv.extend(["--settings".into(), ctx.vars.expand_value(v).to_string()]);
            }
            argv.extend(data_dir_args(sh));
            let spec = Spec { argv, cwd: Some(ctx.dir.clone()), env: sh.opts.env.clone(), timeout, log: ctx.dir.join(&key) };
            let ended = spawn(sh, idx, s, &spec)?;
            outcome(ended, &spec.log, &key)?;
            Ok(vec![out.to_string_lossy().into_owned()])
        }
    }
}

/// The item's project: a copy of the input project or the template, or a new project built by the
/// template's commands (default: import the input and make a sequence from it). Written under a
/// temporary name and renamed, so a half-made project never takes the item's name.
fn create(sh: &Shared, idx: usize, s: usize, ctx: &Ctx) -> Result<(), ErrorInfo> {
    let io = |e: std::io::Error, p: &Path| ErrorInfo::new(Category::Io, format!("{}: {e}", p.display()));
    if ctx.project.exists() {
        return Err(ErrorInfo::new(Category::Io, format!("{} already exists; move it away to make it again", ctx.project.display())));
    }
    let folder = ctx.project.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&folder).map_err(|e| io(e, &folder))?;
    let tmp = folder.join(format!(".{}.creating.{}", pipeline::safe_name(&ctx.name), pipeline::PROJECT_EXTENSION));
    let _ = std::fs::remove_file(&tmp);
    let template = sh.pipeline.template.as_ref();
    let copy_from = if ctx.input_is_project { Some(PathBuf::from(&ctx.input)) } else { template.and_then(|t| t.project.as_ref()).map(PathBuf::from) };
    if let Some(src) = copy_from {
        std::fs::copy(&src, &tmp).map_err(|e| io(e, &src))?;
    } else {
        let lines = match template {
            Some(t) if !t.commands.is_empty() => t.commands.clone(),
            _ => pipeline::default_create_commands(),
        };
        let script = write_script(ctx, "create", &lines)?;
        let mut argv = vec![cli(sh), "run".into(), path_arg(&script), "--save-as".into(), path_arg(&tmp), "--compact".into()];
        argv.extend(data_dir_args(sh));
        let spec = Spec { argv, cwd: Some(ctx.dir.clone()), env: sh.opts.env.clone(), timeout: None, log: ctx.dir.join("create") };
        let ended = spawn(sh, idx, s, &spec)?;
        outcome(ended, &spec.log, "create")?;
        if !tmp.is_file() {
            return Err(ErrorInfo::new(Category::Command, "create: filmcraft-cli saved no project"));
        }
    }
    std::fs::rename(&tmp, &ctx.project).map_err(|e| io(e, &ctx.project))
}

/// An agent step (#60): rounds of the agent, each followed by `bashcut.review.run` on every sequence
/// the agent made (#195; the active one when it made none); blocking findings go back to the agent
/// (resuming its session) at most `fixRounds` times, then the item needs fixing.
/// `maxMinutes` covers the whole step, `maxTurns` each round; at most `agentConcurrency` agent steps
/// run at once. Usage and transcripts go into the step's `agent`, the last review into `review`.
fn agent_step(sh: &Shared, idx: usize, s: usize, ctx: &Ctx, key: &str, a: &AgentStep) -> Result<Vec<String>, ErrorInfo> {
    let io = |e: std::io::Error, p: &Path| ErrorInfo::new(Category::Io, format!("{}: {e}", p.display()));
    let Some(_slot) = AgentSlot::take(sh) else { return Err(ErrorInfo::new(Category::Cancelled, format!("{key}: cancelled"))) };
    let start = Instant::now();
    let limit = a.max_duration();
    let roots = vec![ctx.dir.clone(), sh.pipeline.outputs_dir()];
    let (program, args) = agent::mcp_server(&cli(sh), sh.opts.mcp.as_deref(), &ctx.project, &roots);
    let config = ctx.dir.join(format!("{key}.mcp.json"));
    std::fs::write(&config, agent::mcp_config(&program, &args).to_string()).map_err(|e| io(e, &config))?;
    let mut usage = AgentUsage { agent: if a.argv.is_empty() { a.agent_name().to_string() } else { "custom".into() }, ..AgentUsage::default() };
    let mut prompt = agent::first_prompt(a, &ctx.vars);
    let rounds = a.fix_rounds();
    let before = if a.review { start_sequences(sh, idx, s, ctx, key)? } else { Vec::new() };
    let minutes = limit.as_secs_f64() / 60.0;
    for round in 0..=rounds {
        let remaining = limit.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(ErrorInfo::new(Category::Timeout, format!("{key}: the agent ran past maxMinutes ({minutes:.1})")));
        }
        let log = ctx.dir.join(format!("{key}.round-{round}"));
        let argv = agent::argv(&agent::Launch { step: a, vars: &ctx.vars, prompt: &prompt, session: usage.session.as_deref(), mcp_config: &config });
        let cwd = ctx.project.parent().map(Path::to_path_buf).unwrap_or_else(|| ctx.dir.clone());
        let spec = Spec { argv, cwd: Some(cwd), env: item_env(sh, ctx), timeout: Some(remaining), log: log.clone() };
        let ended = spawn(sh, idx, s, &spec)?;
        let r = agent::read_round(&process::out_path(&log));
        usage.rounds = round.saturating_add(1);
        usage.turns = usage.turns.saturating_add(r.turns);
        usage.input_tokens = usage.input_tokens.saturating_add(r.input_tokens);
        usage.output_tokens = usage.output_tokens.saturating_add(r.output_tokens);
        usage.cost_usd += r.cost_usd;
        usage.ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        if r.session.is_some() {
            usage.session = r.session.clone();
        }
        usage.transcripts.push(process::out_path(&log).to_string_lossy().into_owned());
        let snapshot = usage.clone();
        sh.step(idx, s, |st| st.agent = Some(snapshot));
        match &ended {
            Ended::TimedOut => return Err(ErrorInfo::new(Category::Timeout, format!("{key}: the agent ran past maxMinutes ({minutes:.1})"))),
            Ended::Cancelled => return Err(ErrorInfo::new(Category::Cancelled, format!("{key}: cancelled"))),
            // over the memory cap: `outcome` below reports it
            Ended::Exited(_) | Ended::OverLimit(_) => {}
        }
        if r.hit_turn_limit {
            return Err(ErrorInfo::new(Category::Limit, format!("{key}: the agent hit maxTurns ({}) without finishing", a.max_turns())));
        }
        outcome(ended, &log, key)?;
        if r.failed {
            return Err(ErrorInfo::new(Category::Command, format!("{key}: the agent reported a failure: {}", r.message)));
        }
        if !a.review {
            return Ok(Vec::new());
        }
        let review = review_step(sh, idx, s, ctx, &format!("{key}.review-{round}"), a.review_profile.as_deref(), &before)?;
        let summary = agent::review_summary(&review);
        sh.step(idx, s, |st| st.review = Some(summary));
        let blocking = agent::blocking_count(&review);
        if blocking == 0 {
            return Ok(Vec::new());
        }
        if round == rounds {
            let checks = agent::blocking_labels(&review, 5);
            return Err(ErrorInfo::new(
                Category::Review,
                format!("{key}: {blocking} blocking review finding(s) left after {rounds} fix round(s): {}", checks.join(", ")),
            ));
        }
        prompt = agent::fix_prompt(a, &ctx.vars, &review);
    }
    Ok(Vec::new())
}

/// `bashcut.review.run` on the item's project (saved: it writes its findings as markers), once for
/// each sequence made since `before` (#195), merged into one review; the active sequence when none.
fn review_step(sh: &Shared, idx: usize, s: usize, ctx: &Ctx, key: &str, profile: Option<&str>, before: &[agent::Sequence]) -> Result<Value, ErrorInfo> {
    let inspect = inspect_step(sh, idx, s, ctx, &format!("{key}.inspect"))?;
    let targets = agent::review_targets(before, &agent::sequences(&inspect));
    let lines = agent::review_lines(&targets, agent::active_sequence(&inspect), profile);
    let script = write_script(ctx, key, &lines)?;
    let mut argv = vec![cli(sh), "run".into(), path_arg(&script), "--project".into(), path_arg(&ctx.project), "--save".into(), "--compact".into()];
    argv.extend(data_dir_args(sh));
    let spec = Spec { argv, cwd: Some(ctx.dir.clone()), env: sh.opts.env.clone(), timeout: None, log: ctx.dir.join(key) };
    let ended = spawn(sh, idx, s, &spec)?;
    outcome(ended, &spec.log, key)?;
    let mut results = agent::results(&process::out_path(&spec.log), "bashcut.review.run");
    if targets.is_empty() {
        return results.pop().ok_or_else(|| ErrorInfo::new(Category::Command, format!("{key}: no bashcut.review.run result")));
    }
    if results.len() != targets.len() {
        return Err(ErrorInfo::new(Category::Command, format!("{key}: {} of {} sequence reviews returned a result", results.len(), targets.len())));
    }
    Ok(agent::merge_reviews(&targets.into_iter().zip(results).collect::<Vec<_>>()))
}

/// The sequences the agent step started from (the ones it adds are its outputs), kept in
/// `<itemDir>/<key>.sequences.json` so a retry or resume still reviews what the first attempt made.
fn start_sequences(sh: &Shared, idx: usize, s: usize, ctx: &Ctx, key: &str) -> Result<Vec<agent::Sequence>, ErrorInfo> {
    let path = ctx.dir.join(format!("{key}.sequences.json"));
    if let Some(kept) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Vec<agent::Sequence>>(&t).ok()) {
        return Ok(kept);
    }
    let list = agent::sequences(&inspect_step(sh, idx, s, ctx, &format!("{key}.inspect"))?);
    let text = serde_json::to_string(&list).map_err(|e| ErrorInfo::new(Category::Io, format!("{}: {e}", path.display())))?;
    std::fs::write(&path, text).map_err(|e| ErrorInfo::new(Category::Io, format!("{}: {e}", path.display())))?;
    Ok(list)
}

/// `project.inspect` on the item's project (not saved).
fn inspect_step(sh: &Shared, idx: usize, s: usize, ctx: &Ctx, key: &str) -> Result<Value, ErrorInfo> {
    let line = CommandLine { id: "project.inspect".into(), params: serde_json::json!({}) };
    let script = write_script(ctx, key, std::slice::from_ref(&line))?;
    let mut argv = vec![cli(sh), "run".into(), path_arg(&script), "--project".into(), path_arg(&ctx.project), "--compact".into()];
    argv.extend(data_dir_args(sh));
    let spec = Spec { argv, cwd: Some(ctx.dir.clone()), env: sh.opts.env.clone(), timeout: None, log: ctx.dir.join(key) };
    let ended = spawn(sh, idx, s, &spec)?;
    outcome(ended, &spec.log, key)?;
    agent::results(&process::out_path(&spec.log), "project.inspect")
        .pop()
        .ok_or_else(|| ErrorInfo::new(Category::Command, format!("{key}: no project.inspect result")))
}

fn engine_step(sh: &Shared, idx: usize, s: usize, ctx: &Ctx, key: &str, lines: &[CommandLine], timeout: Option<Duration>) -> Result<Vec<String>, ErrorInfo> {
    let mut with_clean = Vec::with_capacity(lines.len());
    for l in lines {
        let params = crate::speech::with_model(&l.id, &crate::speech::without_clean_keys(&l.id, &l.params), sh.speech_model.as_deref());
        with_clean.push(CommandLine { id: l.id.clone(), params });
        if let Some((id, params)) = crate::speech::clean_after(&l.id, &l.params) {
            with_clean.push(CommandLine { id, params });
        }
    }
    let lines = with_clean;
    let script = write_script(ctx, key, &lines)?;
    let mut argv = vec![cli(sh), "run".into(), path_arg(&script), "--project".into(), path_arg(&ctx.project), "--save".into(), "--compact".into()];
    argv.extend(data_dir_args(sh));
    let spec = Spec { argv, cwd: Some(ctx.dir.clone()), env: sh.opts.env.clone(), timeout, log: ctx.dir.join(key) };
    let ended = spawn(sh, idx, s, &spec)?;
    outcome(ended, &spec.log, key).map(|()| Vec::new())
}

/// `<itemDir>/<key>.jsonl`: one `{"id","params"}` per line, placeholders filled in.
fn write_script(ctx: &Ctx, key: &str, lines: &[CommandLine]) -> Result<PathBuf, ErrorInfo> {
    let mut text = String::new();
    for l in lines {
        let params = if l.params.is_null() { Value::Object(serde_json::Map::new()) } else { ctx.vars.expand_value(&l.params) };
        text.push_str(&serde_json::json!({"id": l.id, "params": params}).to_string());
        text.push('\n');
    }
    let path = ctx.dir.join(format!("{key}.jsonl"));
    std::fs::write(&path, text).map_err(|e| ErrorInfo::new(Category::Io, format!("{}: {e}", path.display())))?;
    Ok(path)
}

/// Start a step's process, recording its pid in the book first thing (the tripwire).
/// The process's memory is checked every `watch_every` against the cap (#61).
fn spawn(sh: &Shared, idx: usize, s: usize, spec: &Spec) -> Result<Ended, ErrorInfo> {
    let cap = sh.pipeline.resources.memory_cap();
    let mut checked: Option<Instant> = None;
    process::run_watched(
        spec,
        sh.opts.poll,
        &mut |pid| {
            sh.step(idx, s, |st| st.pid = Some(pid));
        },
        &|| sh.cancelled(),
        &mut |pid| {
            if checked.is_some_and(|t| t.elapsed() < sh.opts.watch_every) {
                return None;
            }
            checked = Some(Instant::now());
            let used = sh.opts.probe.process_memory(pid)?;
            sh.meter.memory(used);
            let cap = cap?;
            let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
            (used > cap).then(|| format!("used {:.1} GB, over the {:.1} GB cap; stopped to retry later", gb(used), gb(cap)))
        },
    )
}

/// A step's result from its exit status and the end of its output: 0 ok, 1 a reported failure
/// (offline media for an export), 2 usage, 3 a hand-off, anything else a crash.
fn outcome(ended: Ended, log: &Path, what: &str) -> Result<(), ErrorInfo> {
    let code = match ended {
        Ended::Exited(Some(0)) => return Ok(()),
        Ended::TimedOut => return Err(ErrorInfo::new(Category::Timeout, format!("{what}: timed out"))),
        Ended::Cancelled => return Err(ErrorInfo::new(Category::Cancelled, format!("{what}: cancelled"))),
        Ended::OverLimit(why) => return Err(ErrorInfo::new(Category::Resources, format!("{what}: {why}"))),
        Ended::Exited(code) => code,
    };
    let out = process::tail(&process::out_path(log), LOG_TAIL);
    let err = process::tail(&process::err_path(log), LOG_TAIL);
    let category = match code {
        Some(1) if out.contains("\"offlineMedia\"") => Category::OfflineMedia,
        Some(1) => Category::Command,
        Some(2) => Category::Usage,
        Some(3) => Category::Handoff,
        _ => Category::Crashed,
    };
    let detail = reported_error(&out).or_else(|| err.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string)).unwrap_or_default();
    let status = code.map(|c| format!("exit {c}")).unwrap_or_else(|| "killed by a signal".into());
    let mut message = if detail.is_empty() { format!("{what}: {status}") } else { format!("{what}: {detail} ({status})") };
    if message.chars().count() > MAX_MESSAGE {
        message = message.chars().take(MAX_MESSAGE).collect::<String>() + "…";
    }
    // after the cut, so the next step is always shown
    if let Some(next) = speech_next_step(&detail) {
        message = format!("{message}. {next}");
    }
    Err(ErrorInfo::new(category, message))
}

/// What to do when a step failed for want of speech-to-text (#146): the batch presets and caption
/// skills start by transcribing, and the engine's own error names no fix.
fn speech_next_step(detail: &str) -> Option<String> {
    const PREFIX: &str = "the speech model `";
    if let Some(start) = detail.find(PREFIX) {
        let rest = detail.get(start.saturating_add(PREFIX.len())..).unwrap_or_default();
        let model = rest.split('`').next().filter(|m| !m.is_empty() && rest.contains("` is not downloaded"))?;
        return Some(format!(
            "Next: download it once with `bashcutpro exec transcript.downloadModel model={model}` (or Text panel > Transcribe in the app), then retry the batch; `bashcut-doctor` checks it"
        ));
    }
    detail.contains("speech-to-text is not available in this build").then(|| {
        "Next: install BashCutPro from its installer (it includes speech-to-text) or build with `cargo build -p filmcraft-cli --features whisper`; `bashcut-doctor` checks it".to_string()
    })
}

/// The error in the last JSON result that says `"ok": false` (a `filmcraft-cli run` line or a
/// `bashcut-result/1` object, which plugins may pretty-print over several lines).
fn reported_error(out: &str) -> Option<String> {
    let failed = |v: &Value| v.get("ok") == Some(&Value::Bool(false));
    let stream = serde_json::Deserializer::from_str(out).into_iter::<Value>().map_while(|v| v.ok()).filter(|v| failed(v)).last();
    let found = stream.or_else(|| out.lines().rev().filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok()).find(|v| failed(v)));
    found.map(|v| {
        let text = |x: Option<&Value>| x.and_then(Value::as_str).map(str::to_string);
        text(v.get("error")).or_else(|| text(v.get("message"))).or_else(|| text(v.get("error").and_then(|e| e.get("message")))).unwrap_or_else(|| v.to_string())
    })
}

fn item_env(sh: &Shared, ctx: &Ctx) -> Vec<(String, String)> {
    let mut env = sh.opts.env.clone();
    env.extend([
        ("BASHCUT_PROJECT".into(), ctx.project.to_string_lossy().into_owned()),
        ("BASHCUT_FILMCRAFT_CLI".into(), cli(sh)),
        ("BASHCUTPRO_CLI".into(), cli(sh)),
        ("BASHCUT_BATCH_ITEM".into(), ctx.name.clone()),
        ("BASHCUT_BATCH_JOB".into(), sh.job.dir.to_string_lossy().into_owned()),
    ]);
    if let Some(k) = &sh.opts.kit {
        env.push(("BASHCUT_KIT".into(), path_arg(k)));
    }
    env
}

fn cli(sh: &Shared) -> String {
    path_arg(&crate::cli::engine(&sh.opts.cli))
}

/// `<data dir>/bashcut-plugins`, where the plugin host installs plugins (`{plugins}`).
fn plugins_dir(sh: &Shared) -> PathBuf {
    sh.opts.data_dir.clone().or_else(crate::presets::default_data_dir).unwrap_or_default().join("bashcut-plugins")
}

fn data_dir_args(sh: &Shared) -> Vec<String> {
    dir_args(sh.opts)
}

fn dir_args(opts: &Options) -> Vec<String> {
    opts.data_dir.as_ref().map(|d| vec!["--data-dir".to_string(), path_arg(d)]).unwrap_or_default()
}

/// The model for transcribe steps that name none: asked of the engine only when there are such steps.
fn speech_model(pipeline: &Pipeline, opts: &Options) -> Option<String> {
    if opts.speech_model.is_some() {
        return opts.speech_model.clone();
    }
    let unnamed =
        |id: &str, params: Option<&Value>| crate::speech::TRANSCRIBE_COMMANDS.contains(&id) && params.and_then(|p| p.get("model")).is_none_or(Value::is_null);
    let needed = pipeline.steps.iter().filter_map(|s| s.kind().ok()).any(|k| match k {
        StepKind::Command(id, params) => unnamed(id, params),
        StepKind::Commands(lines) => lines.iter().any(|l| unnamed(&l.id, Some(&l.params))),
        _ => false,
    });
    if !needed {
        return None;
    }
    let settings = opts.data_dir.clone().or_else(crate::presets::default_data_dir).and_then(|d| crate::speech::settings_model(&d));
    let models = crate::speech::engine_models(&crate::cli::engine(&opts.cli), &dir_args(opts), &opts.env);
    crate::speech::choose(settings.as_deref(), models.as_ref())
}

fn path_arg(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A relative path with no `..`, root or prefix: it stays inside the folder it is joined to.
fn is_relative_inside(p: &Path) -> bool {
    !p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_speech_model_names_the_next_step() {
        let next = speech_next_step("the speech model `whisper-base` is not downloaded").unwrap();
        assert!(next.contains("transcript.downloadModel model=whisper-base"), "{next}");
        let next = speech_next_step("speech-to-text is not available in this build (built without the `whisper` feature)").unwrap();
        assert!(next.contains("--features whisper"), "{next}");
        assert_eq!(speech_next_step("no clip"), None);
        assert_eq!(speech_next_step("the speech model `"), None);
    }

    #[test]
    fn reported_errors_come_from_the_last_failed_line() {
        let out = "{\"line\":1,\"id\":\"a\",\"ok\":true}\n{\"line\":2,\"id\":\"b\",\"ok\":false,\"error\":\"no clip\"}\n";
        assert_eq!(reported_error(out).as_deref(), Some("no clip"));
        assert_eq!(reported_error("{\"ok\":false,\"message\":\"needs a key\"}").as_deref(), Some("needs a key"));
        assert_eq!(reported_error("{\"ok\":false,\"error\":{\"message\":\"x\"}}").as_deref(), Some("x"));
        assert_eq!(reported_error("plain text"), None);
        let pretty = "{
 \"ok\": false,
 \"error\": {
  \"code\": \"no_cli\",
  \"message\": \"filmcraft-cli not found\"
 }
}
";
        assert_eq!(reported_error(pretty).as_deref(), Some("filmcraft-cli not found"), "a pretty-printed plugin result");
    }

    #[test]
    fn output_names_stay_inside() {
        assert!(is_relative_inside(Path::new("a.mp4")));
        assert!(is_relative_inside(Path::new("tiktok/a.mp4")));
        assert!(!is_relative_inside(Path::new("../a.mp4")));
        assert!(!is_relative_inside(Path::new("/tmp/a.mp4")));
        assert!(!is_relative_inside(Path::new("")));
    }
}
