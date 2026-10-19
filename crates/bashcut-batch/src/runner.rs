//! The runner: up to `concurrency` items at once, each step of an item in its own process.
//!
//! Engine steps run as `filmcraft-cli run <step>.jsonl --project <item>.fcproj --save`: the CLI
//! stops at the first failing line before saving, so a step is all or nothing and every finished
//! step is on disk. Exports run as `filmcraft-cli export <out> --project … --preset …`. Plugin and
//! agent steps run their own command line with the item's placeholders. The app does not need to
//! be open. One runner per job at a time (`runner.lock`).

use crate::book::{self, Book, Category, ErrorInfo, Job, Status, StepRef, StepState, now_ms};
use crate::pipeline::{self, CommandLine, DEFAULT_AGENT_ARGV, OnFailure, Pipeline, StepKind, Vars};
use crate::process::{self, Ended, Spec};
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How much of a step's stdout / stderr goes into its error message.
const LOG_TAIL: u64 = 4096;
const MAX_MESSAGE: usize = 600;

#[derive(Clone, Debug)]
pub struct Options {
    /// The `filmcraft-cli` to run.
    pub cli: PathBuf,
    /// Overrides the pipeline's `concurrency`.
    pub concurrency: Option<usize>,
    /// Passed to filmcraft-cli as `--data-dir` (user export presets).
    pub data_dir: Option<PathBuf>,
    /// How often a waiting runner checks its processes, cancel and timeouts.
    pub poll: Duration,
    /// Extra environment for every process the runner starts.
    pub env: Vec<(String, String)>,
}

impl Options {
    pub fn new(cli: impl Into<PathBuf>) -> Self {
        Options { cli: cli.into(), concurrency: None, data_dir: None, poll: Duration::from_millis(200), env: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub job: String,
    pub status: Status,
    pub counts: book::Counts,
    /// Tripped steps and other things a person should look at.
    pub reports: Vec<String>,
    pub elapsed_ms: u64,
}

impl Summary {
    pub fn of(job: &Job, book: &Book, reports: Vec<String>, elapsed_ms: u64) -> Summary {
        Summary { id: book.id.clone(), job: job.dir.to_string_lossy().into_owned(), status: book.status, counts: book.counts(), reports, elapsed_ms }
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
    let workers = opts.concurrency.unwrap_or(pipeline.concurrency).clamp(1, pipeline::MAX_CONCURRENCY).min(pending.max(1));
    let shared = Shared {
        job,
        pipeline: &pipeline,
        opts,
        book: Mutex::new(book),
        stop: AtomicBool::new(false),
        cancel: AtomicBool::new(false),
        save_error: Mutex::new(None),
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
    book.status = book.derive_status(cancelled);
    book.runner_pid = None;
    book.updated_at = now_ms();
    job.save(&book)?;
    if let Some(e) = save_error {
        reports.push(format!("the job book could not be saved during the run: {e}"));
    }
    let elapsed = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(Summary::of(job, &book, reports, elapsed))
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
        let claimed = sh.with(|b| {
            let item = b.items.iter_mut().find(|i| i.status == Status::Pending)?;
            item.status = Status::Running;
            item.error = None;
            item.started_at = Some(now_ms());
            item.ended_at = None;
            Some(item.name.clone())
        });
        let Some(name) = claimed else { return };
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
                    Category::Handoff => (Status::Failed, Status::NeedsAttention),
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
                    let argv: Vec<String> = p.argv.iter().map(|a| ctx.vars.expand(a)).collect();
                    let mut env: Vec<(String, String)> = p.env.iter().map(|(k, v)| (k.clone(), ctx.vars.expand(v))).collect();
                    env.extend(item_env(sh, ctx));
                    let spec = Spec { argv, cwd: Some(ctx.dir.clone()), env, timeout, log: ctx.dir.join(&key) };
                    let ended = spawn(sh, idx, s, &spec)?;
                    outcome(ended, &spec.log, &key).map(|()| Vec::new())
                }
                StepKind::Agent(a) => {
                    let prompt =
                        format!("Use the {} skill from the BashCutPro kit on the project {}. {}", a.skill, ctx.project.display(), ctx.vars.expand(&a.brief));
                    let mut vars = ctx.vars.clone();
                    vars.0.push(("prompt", prompt.trim().to_string()));
                    let argv: Vec<String> = if a.argv.is_empty() {
                        DEFAULT_AGENT_ARGV.iter().map(|x| vars.expand(x)).collect()
                    } else {
                        a.argv.iter().map(|x| vars.expand(x)).collect()
                    };
                    // the agent works in the project's folder (its allowed root, #51)
                    let cwd = ctx.project.parent().map(Path::to_path_buf).unwrap_or_else(|| ctx.dir.clone());
                    let spec = Spec { argv, cwd: Some(cwd), env: item_env(sh, ctx), timeout, log: ctx.dir.join(&key) };
                    let ended = spawn(sh, idx, s, &spec)?;
                    outcome(ended, &spec.log, &key).map(|()| Vec::new())
                }
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
            let spec = Spec {
                argv,
                cwd: Some(ctx.dir.clone()),
                env: sh.opts.env.clone(),
                timeout: o.timeout_seconds.map(Duration::from_secs),
                log: ctx.dir.join(&key),
            };
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

fn engine_step(sh: &Shared, idx: usize, s: usize, ctx: &Ctx, key: &str, lines: &[CommandLine], timeout: Option<Duration>) -> Result<Vec<String>, ErrorInfo> {
    let script = write_script(ctx, key, lines)?;
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
fn spawn(sh: &Shared, idx: usize, s: usize, spec: &Spec) -> Result<Ended, ErrorInfo> {
    process::run(
        spec,
        sh.opts.poll,
        &mut |pid| {
            sh.step(idx, s, |st| st.pid = Some(pid));
        },
        &|| sh.cancelled(),
    )
}

/// A step's result from its exit status and the end of its output: 0 ok, 1 a reported failure
/// (offline media for an export), 2 usage, 3 a hand-off, anything else a crash.
fn outcome(ended: Ended, log: &Path, what: &str) -> Result<(), ErrorInfo> {
    let code = match ended {
        Ended::Exited(Some(0)) => return Ok(()),
        Ended::TimedOut => return Err(ErrorInfo::new(Category::Timeout, format!("{what}: timed out"))),
        Ended::Cancelled => return Err(ErrorInfo::new(Category::Cancelled, format!("{what}: cancelled"))),
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
    Err(ErrorInfo::new(category, message))
}

/// The error in the last JSON result line that says `"ok": false` (a `filmcraft-cli run` line or
/// a `bashcut-result/1` object).
fn reported_error(out: &str) -> Option<String> {
    out.lines().rev().filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok()).find(|v| v.get("ok") == Some(&Value::Bool(false))).map(|v| {
        let text = |x: Option<&Value>| x.and_then(Value::as_str).map(str::to_string);
        text(v.get("error")).or_else(|| text(v.get("message"))).or_else(|| text(v.get("error").and_then(|e| e.get("message")))).unwrap_or_else(|| v.to_string())
    })
}

fn item_env(sh: &Shared, ctx: &Ctx) -> Vec<(String, String)> {
    let mut env = sh.opts.env.clone();
    env.extend([
        ("BASHCUT_PROJECT".into(), ctx.project.to_string_lossy().into_owned()),
        ("BASHCUT_FILMCRAFT_CLI".into(), cli(sh)),
        ("BASHCUT_BATCH_ITEM".into(), ctx.name.clone()),
        ("BASHCUT_BATCH_JOB".into(), sh.job.dir.to_string_lossy().into_owned()),
    ]);
    env
}

fn cli(sh: &Shared) -> String {
    path_arg(&sh.opts.cli)
}

fn data_dir_args(sh: &Shared) -> Vec<String> {
    sh.opts.data_dir.as_ref().map(|d| vec!["--data-dir".to_string(), path_arg(d)]).unwrap_or_default()
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
    fn reported_errors_come_from_the_last_failed_line() {
        let out = "{\"line\":1,\"id\":\"a\",\"ok\":true}\n{\"line\":2,\"id\":\"b\",\"ok\":false,\"error\":\"no clip\"}\n";
        assert_eq!(reported_error(out).as_deref(), Some("no clip"));
        assert_eq!(reported_error("{\"ok\":false,\"message\":\"needs a key\"}").as_deref(), Some("needs a key"));
        assert_eq!(reported_error("{\"ok\":false,\"error\":{\"message\":\"x\"}}").as_deref(), Some("x"));
        assert_eq!(reported_error("plain text"), None);
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
