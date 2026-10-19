//! The job book: `<projectsFolder>/.bashcut-batch/<id>/state.json`, every item and step with its
//! state, outputs, error (with a category) and timings. Written after every change (to a temporary
//! file, then renamed), so a batch survives the runner, the app or the machine stopping.
//!
//! **Tripwire:** a step is marked `running` (with the process id) before its process starts. A
//! runner that finds a step still `running` from an earlier run does not run it again: the process
//! may have crashed or been killed half way, so the step becomes `tripped`, its item
//! `needsAttention`, and only an explicit `retry` runs it again.

use crate::pipeline::{self, Pipeline};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const STATE_SCHEMA: &str = "bashcut-batch-state/1";
/// Folder (inside the projects folder) that holds the job books.
pub const JOBS_DIR: &str = ".bashcut-batch";

/// State of a job, an item or a step (not every value applies to each).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Pending,
    Running,
    Done,
    Failed,
    Cancelled,
    /// Step: it was running when the runner stopped; not re-run without `retry`.
    Tripped,
    /// Item or job: a tripped step or a hand-off waits for a person or an agent.
    NeedsAttention,
    /// Item: never started, or stopped between steps, because the batch stopped (`onFailure: stop`).
    NotRun,
    /// Job: stopped after a failure with items left.
    Stopped,
}

impl Status {
    /// Finished for good in this run (retry may still reopen it).
    pub fn is_final(self) -> bool {
        !matches!(self, Status::Pending | Status::Running)
    }
}

/// Why a step failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    /// filmcraft-cli rejected the arguments or a script line (exit 2).
    Usage,
    /// A command, plugin or agent reported a failure (exit 1).
    Command,
    /// An export drew placeholder frames for missing media.
    OfflineMedia,
    Timeout,
    Cancelled,
    /// The process died (a signal, a panic, an unexpected exit status).
    Crashed,
    /// The program could not be started.
    Spawn,
    Tripped,
    /// A plugin needs the calling agent (exit 3, `bashcut-result/1`).
    Handoff,
    /// Reading or writing files around the step.
    Io,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub category: Category,
    pub message: String,
}

impl ErrorInfo {
    pub fn new(category: Category, message: impl Into<String>) -> Self {
        ErrorInfo { category, message: message.into() }
    }
}

/// What a step is, so a resumed runner rebuilds it from the stored pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "index")]
pub enum StepRef {
    /// Make the item's project (copy the template or the input project, or build a new one).
    Create,
    /// `pipeline.steps[i]`.
    Step(usize),
    /// `pipeline.outputs[i]`.
    Export(usize),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepState {
    /// `create`, `step-1`, `export-1`…: also the name of its log files.
    pub key: String,
    #[serde(rename = "ref")]
    pub step: StepRef,
    pub label: String,
    pub status: Status,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
}

impl StepState {
    fn new(key: String, step: StepRef, label: String) -> Self {
        StepState {
            key,
            step,
            label,
            status: Status::Pending,
            attempts: 0,
            pid: None,
            started_at: None,
            ended_at: None,
            duration_ms: None,
            error: None,
            outputs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemState {
    pub name: String,
    pub input: String,
    #[serde(default)]
    pub input_is_project: bool,
    pub project: String,
    pub status: Status,
    pub steps: Vec<StepState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl ItemState {
    /// Every output written so far.
    pub fn outputs(&self) -> Vec<String> {
        self.steps.iter().flat_map(|s| s.outputs.iter().cloned()).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    pub schema: String,
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub status: Status,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_pid: Option<u32>,
    pub items: Vec<ItemState>,
}

impl Book {
    /// The job's status from its items (`cancelled` when asked to stop by `cancel`).
    pub fn derive_status(&self, cancelled: bool) -> Status {
        let any = |s: Status| self.items.iter().any(|i| i.status == s);
        if any(Status::Running) || (any(Status::Pending) && !cancelled && !any(Status::NotRun)) {
            return Status::Running;
        }
        if cancelled || any(Status::Cancelled) {
            Status::Cancelled
        } else if any(Status::NeedsAttention) {
            Status::NeedsAttention
        } else if any(Status::NotRun) || any(Status::Pending) {
            Status::Stopped
        } else if any(Status::Failed) {
            Status::Failed
        } else {
            Status::Done
        }
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for i in &self.items {
            match i.status {
                Status::Done => c.done += 1,
                Status::Failed => c.failed += 1,
                Status::NeedsAttention => c.needs_attention += 1,
                Status::Cancelled => c.cancelled += 1,
                Status::Running => c.running += 1,
                Status::Pending => c.pending += 1,
                _ => c.not_run += 1,
            }
        }
        c
    }

    /// Steps left `running` by a runner that is gone become `tripped` (see the module docs);
    /// returns one line per step.
    pub fn trip_running(&mut self) -> Vec<String> {
        let mut reports = Vec::new();
        let now = now_ms();
        for item in &mut self.items {
            let mut tripped = false;
            for s in &mut item.steps {
                if s.status == Status::Running {
                    let pid = s.pid.map(|p| format!(" (pid {p})")).unwrap_or_default();
                    let msg = format!("was running when the runner stopped{pid}; not run again: check the project, then retry");
                    reports.push(format!("{}: {} {msg}", item.name, s.key));
                    s.status = Status::Tripped;
                    s.ended_at = Some(now);
                    s.error = Some(ErrorInfo::new(Category::Tripped, msg));
                    tripped = true;
                }
            }
            if tripped {
                item.status = Status::NeedsAttention;
                item.error = item.steps.iter().find(|s| s.status == Status::Tripped).and_then(|s| s.error.clone());
            } else if item.status == Status::Running {
                // between two steps: nothing was half done, so it simply continues
                item.status = Status::Pending;
            }
        }
        reports
    }

    /// Reopen items for another run: `names` (or, when empty, every failed, cancelled,
    /// needs-attention and not-run item). Their failed, tripped and cancelled steps go back to
    /// pending; finished steps stay done. Returns how many items were reopened.
    pub fn reopen(&mut self, names: &[String]) -> Result<usize, String> {
        for n in names {
            if !self.items.iter().any(|i| &i.name == n) {
                return Err(format!("no item named {n}"));
            }
        }
        let mut count = 0;
        for item in &mut self.items {
            let pick = if names.is_empty() {
                matches!(item.status, Status::Failed | Status::Cancelled | Status::NeedsAttention | Status::NotRun)
            } else {
                names.contains(&item.name)
            };
            if !pick || item.status == Status::Running {
                continue;
            }
            for s in &mut item.steps {
                if matches!(s.status, Status::Failed | Status::Tripped | Status::Cancelled | Status::Running) {
                    s.status = Status::Pending;
                    s.pid = None;
                }
            }
            if item.steps.iter().all(|s| s.status == Status::Done) && item.status == Status::Done {
                continue;
            }
            item.status = Status::Pending;
            item.error = None;
            count += 1;
        }
        Ok(count)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub done: usize,
    pub failed: usize,
    pub needs_attention: usize,
    pub cancelled: usize,
    pub running: usize,
    pub pending: usize,
    pub not_run: usize,
}

/// A job folder: `state.json` (the book), `pipeline.json` (the resolved pipeline), `items/<name>/`
/// (step scripts and logs), `cancel` (asks the runner to stop), `runner.lock`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub dir: PathBuf,
}

impl Job {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Job, String> {
        let job = Job { dir: pipeline::absolute(&dir.into()) };
        if !job.state_path().is_file() {
            return Err(format!("{}: not a batch job (no state.json)", job.dir.display()));
        }
        Ok(job)
    }

    /// A new job for `pipeline` in `<projectsFolder>/.bashcut-batch/<id>/` (default id: the
    /// pipeline name and the time). Fails if the id is taken or an item's project already exists.
    pub fn create(p: &Pipeline, id: Option<&str>) -> Result<Job, String> {
        let inputs = p.expand_inputs()?;
        let names = p.item_names(&inputs);
        let id = match id {
            Some(id) => pipeline::safe_name(id),
            None => {
                let base = if p.name.trim().is_empty() { "batch".to_string() } else { pipeline::safe_name(&p.name) };
                format!("{base}-{}", now_ms() / 1000)
            }
        };
        let projects = p.projects_dir();
        let dir = projects.join(JOBS_DIR).join(&id);
        if dir.exists() {
            return Err(format!("job {id} already exists ({})", dir.display()));
        }
        let mut items = Vec::with_capacity(inputs.len());
        for (input, name) in inputs.iter().zip(names) {
            let project = projects.join(format!("{name}.{}", pipeline::PROJECT_EXTENSION));
            if project.exists() {
                return Err(format!("{}: already exists; remove it or use another projectsFolder or outputName", project.display()));
            }
            let mut steps = vec![StepState::new("create".into(), StepRef::Create, "Create project".into())];
            for (i, s) in p.steps.iter().enumerate() {
                steps.push(StepState::new(format!("step-{}", i + 1), StepRef::Step(i), s.describe()));
            }
            for (i, o) in p.outputs.iter().enumerate() {
                steps.push(StepState::new(format!("export-{}", i + 1), StepRef::Export(i), format!("Export {}", o.name)));
            }
            items.push(ItemState {
                name,
                input: input.path.to_string_lossy().into_owned(),
                input_is_project: input.is_project,
                project: project.to_string_lossy().into_owned(),
                status: Status::Pending,
                steps,
                error: None,
                started_at: None,
                ended_at: None,
                duration_ms: None,
            });
        }
        std::fs::create_dir_all(dir.join("items")).map_err(|e| format!("{}: {e}", dir.display()))?;
        let job = Job { dir };
        let now = now_ms();
        let book =
            Book { schema: STATE_SCHEMA.into(), id, name: p.name.clone(), status: Status::Pending, created_at: now, updated_at: now, runner_pid: None, items };
        write_atomic(&job.pipeline_path(), &serde_json::to_vec_pretty(p).map_err(|e| e.to_string())?)?;
        job.save(&book)?;
        Ok(job)
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    pub fn pipeline_path(&self) -> PathBuf {
        self.dir.join("pipeline.json")
    }

    pub fn cancel_path(&self) -> PathBuf {
        self.dir.join("cancel")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.dir.join("runner.lock")
    }

    pub fn item_dir(&self, name: &str) -> PathBuf {
        self.dir.join("items").join(pipeline::safe_name(name))
    }

    pub fn book(&self) -> Result<Book, String> {
        let text = pipeline::read_capped(&self.state_path())?;
        let b: Book = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", self.state_path().display()))?;
        if b.schema != STATE_SCHEMA {
            return Err(format!("{}: unknown schema {}", self.state_path().display(), b.schema));
        }
        Ok(b)
    }

    pub fn save(&self, book: &Book) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(book).map_err(|e| e.to_string())?;
        write_atomic(&self.state_path(), &bytes)
    }

    /// The pipeline stored with the job (absolute paths), not the file it came from.
    pub fn pipeline(&self) -> Result<Pipeline, String> {
        let text = pipeline::read_capped(&self.pipeline_path())?;
        let p: Pipeline = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", self.pipeline_path().display()))?;
        p.validate()?;
        Ok(p)
    }

    /// Ask a running batch to stop: running steps are killed, nothing new starts.
    pub fn request_cancel(&self) -> Result<(), String> {
        std::fs::write(self.cancel_path(), b"cancel\n").map_err(|e| format!("{}: {e}", self.cancel_path().display()))
    }

    pub fn cancel_requested(&self) -> bool {
        self.cancel_path().exists()
    }

    /// Reopen items (see [`Book::reopen`]); `run` then picks them up.
    pub fn retry(&self, names: &[String]) -> Result<usize, String> {
        let mut book = self.book()?;
        let n = book.reopen(names)?;
        book.status = book.derive_status(false);
        book.updated_at = now_ms();
        self.save(&book)?;
        Ok(n)
    }

    /// The runner currently holding this job, if its process is alive.
    pub fn runner_alive(&self) -> Option<u32> {
        let pid = std::fs::read_to_string(self.lock_path()).ok()?.trim().parse::<u32>().ok()?;
        crate::process::pid_alive(pid).then_some(pid)
    }
}

/// Every job under a projects folder, oldest first.
pub fn list_jobs(projects: &Path) -> Vec<Job> {
    let Ok(rd) = std::fs::read_dir(projects.join(JOBS_DIR)) else { return Vec::new() };
    let mut jobs: Vec<(u64, Job)> = rd
        .filter_map(|e| e.ok())
        .map(|e| Job { dir: e.path() })
        .filter(|j| j.state_path().is_file())
        .map(|j| (j.book().map(|b| b.created_at).unwrap_or(0), j))
        .collect();
    jobs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.dir.cmp(&b.1.dir)));
    jobs.into_iter().map(|(_, j)| j).collect()
}

/// Write `bytes` to `<path>.tmp`, then rename over `path`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(key: &str, status: Status) -> StepState {
        let mut s = StepState::new(key.into(), StepRef::Create, key.into());
        s.status = status;
        s
    }

    fn item(name: &str, status: Status, steps: Vec<StepState>) -> ItemState {
        ItemState {
            name: name.into(),
            input: format!("/in/{name}.mp4"),
            input_is_project: false,
            project: format!("/p/{name}.fcproj"),
            status,
            steps,
            error: None,
            started_at: None,
            ended_at: None,
            duration_ms: None,
        }
    }

    fn book(items: Vec<ItemState>) -> Book {
        Book {
            schema: STATE_SCHEMA.into(),
            id: "t".into(),
            name: String::new(),
            status: Status::Running,
            created_at: 0,
            updated_at: 0,
            runner_pid: None,
            items,
        }
    }

    #[test]
    fn running_steps_trip_and_their_items_need_attention() {
        let mut b = book(vec![
            item("a", Status::Running, vec![step("create", Status::Done), step("step-1", Status::Running)]),
            item("b", Status::Running, vec![step("create", Status::Done), step("step-1", Status::Pending)]),
            item("c", Status::Done, vec![step("create", Status::Done)]),
        ]);
        let reports = b.trip_running();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].starts_with("a: step-1"), "{reports:?}");
        assert_eq!(b.items[0].status, Status::NeedsAttention);
        assert_eq!(b.items[0].steps[1].status, Status::Tripped);
        assert_eq!(b.items[0].error.as_ref().map(|e| e.category), Some(Category::Tripped));
        assert_eq!(b.items[1].status, Status::Pending, "between steps: just continues");
        assert_eq!(b.derive_status(false), Status::Running);
    }

    #[test]
    fn reopen_resets_unfinished_steps_only() {
        let mut b = book(vec![
            item("a", Status::NeedsAttention, vec![step("create", Status::Done), step("step-1", Status::Tripped), step("export-1", Status::Pending)]),
            item("b", Status::Done, vec![step("create", Status::Done)]),
            item("c", Status::Failed, vec![step("create", Status::Failed)]),
        ]);
        assert!(b.reopen(&["zzz".into()]).is_err());
        assert_eq!(b.reopen(&["a".into()]).unwrap(), 1);
        assert_eq!(b.items[0].status, Status::Pending);
        assert_eq!(b.items[0].steps[0].status, Status::Done);
        assert_eq!(b.items[0].steps[1].status, Status::Pending);
        assert_eq!(b.items[2].status, Status::Failed, "not named");
        assert_eq!(b.reopen(&[]).unwrap(), 1, "only c is left to reopen");
        assert_eq!(b.items[1].status, Status::Done);
    }

    #[test]
    fn job_status_from_items() {
        let s = |st: &[Status]| book(st.iter().map(|x| item("i", *x, vec![])).collect()).derive_status(false);
        assert_eq!(s(&[Status::Done, Status::Done]), Status::Done);
        assert_eq!(s(&[Status::Done, Status::Failed]), Status::Failed);
        assert_eq!(s(&[Status::Failed, Status::NeedsAttention]), Status::NeedsAttention);
        assert_eq!(s(&[Status::Failed, Status::NotRun]), Status::Stopped);
        assert_eq!(s(&[Status::Done, Status::Pending]), Status::Running);
        assert_eq!(book(vec![item("i", Status::Pending, vec![])]).derive_status(true), Status::Cancelled);
    }
}
