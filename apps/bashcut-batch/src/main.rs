//! `bashcut-batch`: run BashCutPro batch pipelines headless (#45). Each input becomes its own
//! project; up to `concurrency` items run at once, each step in its own engine (`bashcutpro-cli`) process; the
//! job book in `<projectsFolder>/.bashcut-batch/<id>/` survives restarts. Output is JSON.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

use bashcut_batch::pipeline::{self, Pipeline};
use bashcut_batch::{Choices, Job, Options, Status, Summary, presets};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const HELP: &str = "\
bashcut-batch: BashCutPro batch pipelines through the bashcutpro-cli engine (headless; the app need not be open)

USAGE
  bashcut-batch run <pipeline.json> [--id ID] [--input PATH]... [--projects DIR] [--outputs DIR] [RUN OPTIONS]
  bashcut-batch run --preset NAME   [--id ID] [--input PATH]... [--projects DIR] [--outputs DIR] [RUN OPTIONS]
                                 make a job (one project per input) and run it to the end; with a preset,
                                 --projects alone puts the exports in DIR/exports
  bashcut-batch resume <job>      continue a stopped or interrupted job; steps that were running
                                 when it stopped are reported (tripped), not run again
  bashcut-batch retry <job> [--items a,b] [--done]
                                 reopen failed / tripped / cancelled items (or the named ones) and run;
                                 --done: their failed step was finished by hand (a hand-off), go on after it
  bashcut-batch cancel <job>      ask the job's runner to stop (running steps are killed with what they
                                 started); Ctrl+C on run / resume / retry does the same, twice quits at once
  bashcut-batch status <job> [--wait] [--timeout S]
                                 the job book; --wait blocks until no runner holds the job
  bashcut-batch list <projects-folder>
                                 jobs in that folder
  bashcut-batch validate <pipeline.json | --preset NAME> [--input PATH]... [--projects DIR] [--outputs DIR]
                                 check a pipeline and print the items it would make
  bashcut-batch presets [list | show NAME | save NAME FILE | install FILE|FOLDER | delete NAME]
                                 saved pipelines in <data dir>/bashcut/batch-presets/; install takes a
                                 pack such as bashcutpro-plugins/packs/batch/

  <job> is a job folder (`<projectsFolder>/.bashcut-batch/<id>`, printed by run and list).

RUN OPTIONS
  --cli PATH          the engine to use (default: $BASHCUTPRO_CLI, $BASHCUT_FILMCRAFT_CLI or
                      $FILMCRAFT_CLI, else bashcutpro-cli, bcpro or filmcraft-cli next to this program,
                      else the same on PATH)
  --concurrency N     items at once (overrides the pipeline; free RAM and cores may allow fewer)
  --no-keep-awake     let the machine sleep while the batch runs
  --agent-concurrency N
                      agent steps at once, across items (overrides the pipeline's agentConcurrency)
  --mcp PATH          bashcut-mcp for agent steps (default: $BASHCUT_MCP, else the one next to this
                      program; without it agents get bashcutpro-cli mcp, writes not limited)
  --data-dir DIR      BashCutPro data dir: presets, and --data-dir for the engine
  --kit DIR           the agent kit (bashcutpro-agent-kit): `{kit}` in steps, and agent steps get its
                      skills and scripts (default: $BASHCUT_KIT, else <data dir>/bashcut-kit if there)
  --speech-model ID   the speech model for transcribe steps that name none (default: the one picked
                      in Settings when downloaded, else the engine's default, else the downloaded one)
  --compact           one-line JSON

EXIT STATUS
  0 every item done · 1 the job ended failed, stopped, cancelled or needing attention · 2 usage or error
";

const VALUED: &[&str] = &[
    "--preset",
    "--id",
    "--input",
    "--projects",
    "--outputs",
    "--cli",
    "--concurrency",
    "--agent-concurrency",
    "--mcp",
    "--data-dir",
    "--items",
    "--timeout",
    "--speech-model",
    "--kit",
];

struct Args {
    pos: Vec<String>,
    opts: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(it: impl IntoIterator<Item = String>) -> Args {
        let mut a = Args { pos: Vec::new(), opts: Vec::new() };
        let mut it = it.into_iter();
        while let Some(s) = it.next() {
            if let Some((k, v)) = s.split_once('=').filter(|(k, _)| k.starts_with("--")) {
                a.opts.push((k.to_string(), Some(v.to_string())));
            } else if VALUED.contains(&s.as_str()) {
                let v = it.next();
                a.opts.push((s, v));
            } else if s.starts_with("--") {
                a.opts.push((s, None));
            } else {
                a.pos.push(s);
            }
        }
        a
    }

    fn pos(&self, i: usize) -> Option<&str> {
        self.pos.get(i).map(String::as_str)
    }

    fn opt(&self, k: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(n, _)| n == k).and_then(|(_, v)| v.as_deref())
    }

    fn all(&self, k: &str) -> Vec<&str> {
        self.opts.iter().filter(|(n, _)| n == k).filter_map(|(_, v)| v.as_deref()).collect()
    }

    fn flag(&self, k: &str) -> bool {
        self.opts.iter().any(|(n, _)| n == k)
    }
}

/// An error and the exit status it gives.
struct Fail(String, u8);

fn usage(msg: impl Into<String>) -> Fail {
    Fail(msg.into(), 2)
}

fn main() -> ExitCode {
    let a = Args::parse(std::env::args().skip(1));
    match real_main(&a) {
        Ok(code) => ExitCode::from(code),
        Err(Fail(msg, code)) => {
            let _ = writeln!(std::io::stderr(), "bashcut-batch: {msg}");
            ExitCode::from(code)
        }
    }
}

fn print(a: &Args, v: &Value) {
    let s = if a.flag("--compact") { serde_json::to_string(v) } else { serde_json::to_string_pretty(v) };
    // a closed stdout is not a crash
    let _ = writeln!(std::io::stdout(), "{}", s.unwrap_or_default());
}

fn real_main(a: &Args) -> Result<u8, Fail> {
    match a.pos(0) {
        Some("run") => {
            let p = load_pipeline(a, 1)?;
            let job = Job::create(&p, a.opt("--id")).map_err(usage)?;
            run(a, &job)
        }
        Some("resume") => {
            let job = open_job(a)?;
            run(a, &job)
        }
        Some("retry") => {
            let job = open_job(a)?;
            let items: Vec<String> =
                a.opt("--items").map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
            let n = job.retry_with(&items, a.flag("--done")).map_err(usage)?;
            if n == 0 {
                let _ = writeln!(std::io::stderr(), "bashcut-batch: no item to retry; resuming");
            }
            run(a, &job)
        }
        Some("cancel") => {
            let job = open_job(a)?;
            job.request_cancel().map_err(|e| Fail(e, 2))?;
            let runner = job.runner_alive();
            print(a, &json!({"job": job.dir.to_string_lossy(), "cancelRequested": true, "runnerPid": runner}));
            Ok(0)
        }
        Some("status") => {
            let job = open_job(a)?;
            if a.flag("--wait") {
                let limit = a.opt("--timeout").map(|t| t.parse::<u64>().map_err(|_| usage("--timeout takes seconds"))).transpose()?;
                let start = Instant::now();
                while job.runner_alive().is_some() {
                    if limit.is_some_and(|l| start.elapsed() >= Duration::from_secs(l)) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            let book = job.book().map_err(|e| Fail(e, 2))?;
            let runner = job.runner_alive();
            let mut v = serde_json::to_value(&book).map_err(|e| Fail(e.to_string(), 2))?;
            if let Some(m) = v.as_object_mut() {
                m.insert("job".into(), json!(job.dir.to_string_lossy()));
                m.insert("counts".into(), json!(book.counts()));
                m.insert("runnerAlive".into(), json!(runner.is_some()));
                if runner.is_none() && book.status == Status::Running {
                    m.insert("note".into(), json!("interrupted: no runner holds this job; run `bashcut-batch resume` on it"));
                }
            }
            print(a, &v);
            Ok(exit_for(book.status))
        }
        Some("list") => {
            let folder = a.pos(1).ok_or_else(|| usage("list <projects-folder>"))?;
            let jobs: Vec<Value> = bashcut_batch::list_jobs(Path::new(folder))
                .iter()
                .map(|j| match j.book() {
                    Ok(b) => {
                        json!({"id": b.id, "job": j.dir.to_string_lossy(), "name": b.name, "status": b.status, "createdAt": b.created_at, "counts": b.counts()})
                    }
                    Err(e) => json!({"job": j.dir.to_string_lossy(), "error": e}),
                })
                .collect();
            print(a, &json!(jobs));
            Ok(0)
        }
        Some("validate") => {
            let p = load_pipeline(a, 1)?;
            let inputs = p.expand_inputs().map_err(usage)?;
            let names = p.item_names(&inputs);
            let items: Vec<Value> = inputs
                .iter()
                .zip(&names)
                .map(|(i, n)| json!({"name": n, "input": i.path.to_string_lossy(), "project": p.projects_dir().join(format!("{n}.{}", pipeline::PROJECT_EXTENSION)).to_string_lossy()}))
                .collect();
            let steps: Vec<String> = p.steps.iter().map(|s| s.describe()).collect();
            print(
                a,
                &json!({"ok": true, "items": items, "steps": steps, "outputs": p.outputs, "outputsFolder": p.outputs_dir().to_string_lossy(), "concurrency": p.concurrency}),
            );
            Ok(0)
        }
        Some("presets") => presets_cmd(a),
        Some("help") | None => {
            let _ = write!(std::io::stdout(), "{HELP}");
            Ok(if a.pos(0).is_none() { 2 } else { 0 })
        }
        Some(other) => Err(usage(format!("unknown subcommand {other:?} (bashcut-batch help)"))),
    }
}

/// Run `job` to its end. Ctrl+C asks it to cancel (as `bashcut-batch cancel`): the runner stops its
/// steps with their process groups and saves the book; a second Ctrl+C quits at once.
fn run(a: &Args, job: &Job) -> Result<u8, Fail> {
    let opts = options(a)?;
    let target = job.clone();
    let mut asked = false;
    let handler = ctrlc::set_handler(move || {
        if asked {
            let _ = writeln!(std::io::stderr(), "bashcut-batch: quitting; steps may still be running (bashcut-batch resume reports them)");
            std::process::exit(130);
        }
        asked = true;
        let _ = writeln!(std::io::stderr(), "bashcut-batch: cancelling (stopping the running steps); Ctrl+C again to quit at once");
        if let Err(e) = target.request_cancel() {
            let _ = writeln!(std::io::stderr(), "bashcut-batch: {e}");
        }
    });
    if let Err(e) = handler {
        let _ = writeln!(std::io::stderr(), "bashcut-batch: Ctrl+C will not cancel cleanly: {e}");
    }
    finish(a, job, bashcut_batch::run(job, &opts))
}

fn finish(a: &Args, job: &Job, r: Result<Summary, String>) -> Result<u8, Fail> {
    let s = r.map_err(|e| Fail(format!("{}: {e}", job.dir.display()), 2))?;
    print(a, &serde_json::to_value(&s).unwrap_or(Value::Null));
    Ok(exit_for(s.status))
}

fn exit_for(status: Status) -> u8 {
    if status == Status::Done { 0 } else { 1 }
}

fn open_job(a: &Args) -> Result<Job, Fail> {
    let dir = a.pos(1).ok_or_else(|| usage(format!("{} <job folder>", a.pos(0).unwrap_or_default())))?;
    Job::open(dir).map_err(usage)
}

fn data_dir(a: &Args) -> Result<PathBuf, Fail> {
    a.opt("--data-dir").map(|d| pipeline::absolute(Path::new(d))).or_else(bashcut_dirs::data_dir).ok_or_else(|| usage("no data dir: pass --data-dir"))
}

/// The pipeline from `pos(at)` or `--preset`, with `--input` / `--projects` / `--outputs` applied. A preset run
/// with `--projects` and no `--outputs` exports into `<projects>/exports`, so everything lands where asked.
fn load_pipeline(a: &Args, at: usize) -> Result<Pipeline, Fail> {
    let cwd = pipeline::absolute(Path::new("."));
    let mut p = match (a.opt("--preset"), a.pos(at)) {
        (Some(name), None) => presets::load(&data_dir(a)?, name, &cwd).map_err(usage)?,
        (None, Some(file)) => Pipeline::load(Path::new(file)).map_err(usage)?,
        _ => return Err(usage("give a pipeline file or --preset NAME")),
    };
    let inputs = a.all("--input");
    if !inputs.is_empty() {
        p.inputs = inputs.iter().map(|i| pipeline::absolute(Path::new(i)).to_string_lossy().into_owned()).collect();
    }
    if let Some(dir) = a.opt("--projects") {
        p.projects_folder = pipeline::absolute(Path::new(dir)).to_string_lossy().into_owned();
        if a.opt("--preset").is_some() {
            p.outputs_folder = None;
        }
    }
    if let Some(dir) = a.opt("--outputs") {
        p.outputs_folder = Some(pipeline::absolute(Path::new(dir)).to_string_lossy().into_owned());
    }
    p.validate().map_err(usage)?;
    Ok(p)
}

fn options(a: &Args) -> Result<Options, Fail> {
    let count = |k: &str| a.opt(k).map(|n| n.parse::<usize>().map_err(|_| usage(format!("{k} takes a number")))).transpose();
    Options::resolve(Choices {
        cli: a.opt("--cli").map(PathBuf::from),
        concurrency: count("--concurrency")?,
        agent_concurrency: count("--agent-concurrency")?,
        data_dir: a.opt("--data-dir").map(PathBuf::from),
        mcp: a.opt("--mcp").map(PathBuf::from),
        kit: a.opt("--kit").map(PathBuf::from),
        speech_model: a.opt("--speech-model").map(str::to_string),
        no_keep_awake: a.flag("--no-keep-awake"),
    })
    .map_err(usage)
}

fn presets_cmd(a: &Args) -> Result<u8, Fail> {
    let dir = data_dir(a)?;
    match a.pos(1).unwrap_or("list") {
        "list" => {
            let list: Vec<Value> = presets::list(&dir).into_iter().map(|n| json!({"name": n, "description": presets::description(&dir, &n)})).collect();
            print(a, &json!({"dir": presets::presets_dir(&dir).to_string_lossy(), "presets": list}));
            Ok(0)
        }
        "install" => {
            let src = a.pos(2).ok_or_else(|| usage("presets install FILE|FOLDER"))?;
            let names = presets::install(&dir, Path::new(src)).map_err(usage)?;
            print(a, &json!({"installed": names}));
            Ok(0)
        }
        "show" => {
            let name = a.pos(2).ok_or_else(|| usage("presets show NAME"))?;
            let text = presets::read(&dir, name).map_err(usage)?;
            let v: Value = serde_json::from_str(&text).map_err(|e| usage(e.to_string()))?;
            print(a, &v);
            Ok(0)
        }
        "save" => {
            let (Some(name), Some(file)) = (a.pos(2), a.pos(3)) else { return Err(usage("presets save NAME FILE")) };
            let text = pipeline::read_capped(Path::new(file)).map_err(usage)?;
            let path = presets::save(&dir, name, &text).map_err(usage)?;
            print(a, &json!({"saved": path.to_string_lossy()}));
            Ok(0)
        }
        "delete" => {
            let name = a.pos(2).ok_or_else(|| usage("presets delete NAME"))?;
            presets::delete(&dir, name).map_err(usage)?;
            print(a, &json!({"deleted": name}));
            Ok(0)
        }
        other => Err(usage(format!("presets: unknown action {other:?} (list | show | save | install | delete)"))),
    }
}
