//! The batch runner against a fake `filmcraft-cli` (`tests/support/fake_cli.rs`): parallel items,
//! failures with skip and stop, cancel, resume after a kill (tripwire), error categories.

use bashcut_batch::{Category, ItemState, Job, Options, Pipeline, SCHEMA, Status, run};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FAKE: &str = env!("CARGO_BIN_EXE_bashcut-batch-fake-cli");

/// A scratch folder with `clips/<name>.mp4` files.
struct Bench {
    dir: PathBuf,
}

impl Bench {
    fn new(tag: &str, clips: &[&str]) -> Bench {
        let dir = std::env::temp_dir().join(format!("bashcut-batch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("clips")).unwrap();
        for c in clips {
            std::fs::write(dir.join("clips").join(format!("{c}.mp4")), b"media").unwrap();
        }
        Bench { dir }
    }

    fn pipeline(&self, extra: Value) -> Pipeline {
        let mut v = json!({"schema": SCHEMA, "name": "test", "inputs": ["clips"], "projectsFolder": "out"});
        if let (Some(m), Value::Object(e)) = (v.as_object_mut(), extra) {
            m.extend(e);
        }
        Pipeline::parse(&v.to_string(), &self.dir).unwrap()
    }

    fn opts(&self) -> Options {
        let mut o = Options::new(FAKE);
        o.poll = Duration::from_millis(10);
        o.env.push(("FAKE_CLI_LOG".into(), self.log().to_string_lossy().into_owned()));
        o
    }

    fn log(&self) -> PathBuf {
        self.dir.join("fake-cli.log")
    }

    fn out(&self, rel: &str) -> PathBuf {
        self.dir.join("out").join(rel)
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.out(rel)).unwrap_or_default()
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn item<'a>(book: &'a bashcut_batch::Book, name: &str) -> &'a ItemState {
    book.items.iter().find(|i| i.name == name).unwrap()
}

/// The most fake-cli processes that ran at the same time, from the log.
fn max_concurrent(log: &Path) -> i32 {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let mut events: Vec<(u128, i32)> = text
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let kind = w.next()?;
            let t: u128 = w.next()?.parse().ok()?;
            // at the same millisecond an end comes before a start
            Some((t, if kind == "start" { 1 } else { -1 }))
        })
        .collect();
    events.sort();
    let (mut now, mut max) = (0, 0);
    for (_, d) in events {
        now += d;
        max = max.max(now);
    }
    max
}

#[test]
fn parallel_items_each_get_a_project_and_exports() {
    let b = Bench::new("parallel", &["a", "b", "c", "d"]);
    let p = b.pipeline(json!({
        "concurrency": 2,
        "steps": [
            {"command": "fake.sleep", "params": {"ms": 300}},
            {"commands": [{"id": "captions.auto"}, {"id": "captions.style", "params": {"name": "{name}"}}]}
        ],
        "outputs": [{"name": "{name}-tiktok.mp4", "preset": "TikTok"}, {"name": "yt/{name}.mp4", "preset": "YouTube"}]
    }));
    let job = Job::create(&p, Some("par")).unwrap();
    assert!(job.dir.ends_with("out/.bashcut-batch/par"));
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Done, "{summary:?}");
    assert_eq!(summary.counts.done, 4);
    for n in ["a", "b", "c", "d"] {
        assert_eq!(b.read(&format!("{n}.fcproj")), "file.import\nfile.newSequenceFromClip\nfake.sleep\ncaptions.auto\ncaptions.style\n", "{n}");
        assert!(b.read(&format!("exports/{n}-tiktok.mp4")).starts_with("export preset=TikTok\n"));
        assert!(b.read(&format!("exports/yt/{n}.mp4")).starts_with("export preset=YouTube\n"));
    }
    let book = job.book().unwrap();
    let a = item(&book, "a");
    assert_eq!(a.outputs().len(), 2);
    assert!(a.steps.iter().all(|s| s.status == Status::Done && s.duration_ms.is_some() && s.attempts == 1));
    assert!(a.duration_ms.is_some());
    // placeholders filled in the step script
    let script = std::fs::read_to_string(job.item_dir("a").join("step-2.jsonl")).unwrap();
    assert!(script.contains("\"name\":\"a\""), "{script}");
    let create = std::fs::read_to_string(job.item_dir("a").join("create.jsonl")).unwrap();
    assert!(create.contains(&b.dir.join("clips").join("a.mp4").to_string_lossy().replace('\\', "\\\\")), "{create}");
    assert_eq!(max_concurrent(&b.log()), 2, "two items at a time");
    assert!(job.runner_alive().is_none(), "lock released");
}

#[test]
fn a_failing_item_is_skipped_and_the_others_finish() {
    let b = Bench::new("skip", &["a", "b", "c"]);
    let p = b.pipeline(json!({
        "steps": [{"command": "fake.failIf", "params": {"name": "{name}", "fail": "b"}}],
        "outputs": [{"name": "{name}.mp4"}]
    }));
    let job = Job::create(&p, Some("skip")).unwrap();
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Failed);
    assert_eq!((summary.counts.done, summary.counts.failed), (2, 1));
    let book = job.book().unwrap();
    let bad = item(&book, "b");
    assert_eq!(bad.status, Status::Failed);
    let err = bad.error.as_ref().unwrap();
    assert_eq!(err.category, Category::Command);
    assert!(err.message.contains("fake failure"), "{}", err.message);
    assert_eq!(bad.steps[2].status, Status::Pending, "its export never ran");
    assert!(!b.out("exports/b.mp4").exists());
    assert!(b.out("exports/c.mp4").exists());
}

#[test]
fn stop_on_failure_leaves_the_rest_not_run_and_a_new_run_finishes_them() {
    let b = Bench::new("stop", &["a", "b", "c"]);
    let p = b.pipeline(json!({
        "onFailure": "stop",
        "steps": [{"command": "fake.failIf", "params": {"name": "{name}", "fail": "b"}}]
    }));
    let job = Job::create(&p, Some("stop")).unwrap();
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Stopped);
    let book = job.book().unwrap();
    assert_eq!(item(&book, "a").status, Status::Done);
    assert_eq!(item(&book, "b").status, Status::Failed);
    assert_eq!(item(&book, "c").status, Status::NotRun);
    assert!(!b.out("c.fcproj").exists());
    // resume: not-run items go, the failed one waits for retry
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Failed);
    let book = job.book().unwrap();
    assert_eq!(item(&book, "c").status, Status::Done);
    assert_eq!(item(&book, "b").steps[1].attempts, 1, "failed step not re-run without retry");
}

#[test]
fn cancel_kills_the_running_step_and_starts_nothing_new() {
    let b = Bench::new("cancel", &["a", "b"]);
    let p = b.pipeline(json!({"steps": [{"command": "fake.sleep", "params": {"ms": 5000}}]}));
    let job = Job::create(&p, Some("cancel")).unwrap();
    let canceller = {
        let job = job.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            job.request_cancel().unwrap();
        })
    };
    let start = Instant::now();
    let summary = run(&job, &b.opts()).unwrap();
    canceller.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(4), "the 5 s step was killed");
    assert_eq!(summary.status, Status::Cancelled);
    let book = job.book().unwrap();
    let a = item(&book, "a");
    assert_eq!(a.status, Status::Cancelled);
    assert_eq!(a.steps[1].status, Status::Cancelled);
    assert_eq!(a.steps[1].error.as_ref().map(|e| e.category), Some(Category::Cancelled));
    assert_eq!(item(&book, "b").status, Status::Cancelled);
    assert_eq!(b.read("a.fcproj"), "file.import\nfile.newSequenceFromClip\n", "the cancelled step saved nothing");
}

#[test]
fn resume_after_a_kill_trips_the_running_step_instead_of_rerunning_it() {
    let b = Bench::new("trip", &["a", "b"]);
    let p = b.pipeline(json!({"steps": [{"command": "captions.auto"}], "outputs": [{"name": "{name}.mp4"}]}));
    let job = Job::create(&p, Some("trip")).unwrap();
    // As if the machine went down while a's step-1 ran: create done, step-1 running, a stale lock.
    let dead: u32 = u32::MAX - 7;
    let mut book = job.book().unwrap();
    {
        let a = book.items.iter_mut().find(|i| i.name == "a").unwrap();
        a.status = Status::Running;
        a.steps[0].status = Status::Done;
        a.steps[1].status = Status::Running;
        a.steps[1].attempts = 1;
        a.steps[1].pid = Some(dead);
    }
    book.status = Status::Running;
    book.runner_pid = Some(dead);
    job.save(&book).unwrap();
    std::fs::write(b.out("a.fcproj"), "file.import\n").unwrap();
    std::fs::write(job.lock_path(), dead.to_string()).unwrap();

    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::NeedsAttention, "{summary:?}");
    assert!(summary.reports.iter().any(|r| r.starts_with("a: step-1 was running")), "{:?}", summary.reports);
    let book = job.book().unwrap();
    let a = item(&book, "a");
    assert_eq!(a.status, Status::NeedsAttention);
    assert_eq!(a.steps[1].status, Status::Tripped);
    assert_eq!(a.error.as_ref().map(|e| e.category), Some(Category::Tripped));
    assert_eq!(b.read("a.fcproj"), "file.import\n", "the tripped step was not run again");
    assert_eq!(item(&book, "b").status, Status::Done, "the other item went on");

    // A person checked the project: retry runs the step again and finishes the item.
    assert_eq!(job.retry(&["a".to_string()]).unwrap(), 1);
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Done, "{summary:?}");
    let book = job.book().unwrap();
    assert_eq!(item(&book, "a").steps[1].attempts, 2);
    assert_eq!(b.read("a.fcproj"), "file.import\ncaptions.auto\n");
    assert!(b.out("exports/a.mp4").exists());
}

#[test]
fn a_second_runner_on_the_same_job_is_refused() {
    let b = Bench::new("lock", &["a"]);
    let job = Job::create(&b.pipeline(json!({})), Some("lock")).unwrap();
    // a live process that is not us holds the lock
    let script = b.dir.join("sleep.jsonl");
    std::fs::write(&script, "{\"id\":\"fake.sleep\",\"params\":{\"ms\":3000}}\n").unwrap();
    let mut holder = std::process::Command::new(FAKE).arg("run").arg(&script).spawn().unwrap();
    std::fs::write(job.lock_path(), holder.id().to_string()).unwrap();
    let r = run(&job, &b.opts());
    let _ = holder.kill();
    let _ = holder.wait();
    assert!(r.unwrap_err().contains("already running"));
}

/// One clip through `extra`, returning its item.
fn one(tag: &str, extra: Value, cli: Option<&str>) -> (ItemState, Duration) {
    let b = Bench::new(tag, &["x"]);
    let job = Job::create(&b.pipeline(extra), Some(tag)).unwrap();
    let mut opts = b.opts();
    if let Some(c) = cli {
        opts.cli = PathBuf::from(c);
    }
    let start = Instant::now();
    run(&job, &opts).unwrap();
    let book = job.book().unwrap();
    (item(&book, "x").clone(), start.elapsed())
}

#[test]
fn failures_carry_a_category() {
    let (i, took) = one("timeout", json!({"steps": [{"command": "fake.sleep", "params": {"ms": 5000}, "timeoutSeconds": 1}]}), None);
    assert_eq!(i.error.as_ref().map(|e| e.category), Some(Category::Timeout));
    assert!(took < Duration::from_secs(4));

    let (i, _) = one("crash", json!({"steps": [{"command": "fake.crash"}]}), None);
    let e = i.error.unwrap();
    assert_eq!(e.category, Category::Crashed);
    assert!(e.message.contains("exit 101"), "{}", e.message);

    let (i, _) = one("usage", json!({"steps": [{"command": "fake.usage"}]}), None);
    let e = i.error.unwrap();
    assert_eq!(e.category, Category::Usage);
    assert!(e.message.contains("bad params"), "{}", e.message);

    let (i, _) = one("offline", json!({"outputs": [{"name": "{name}.mp4", "preset": "offline"}]}), None);
    assert_eq!(i.error.as_ref().map(|e| e.category), Some(Category::OfflineMedia));

    let (i, _) = one("spawn", json!({}), Some("/no/such/filmcraft-cli"));
    assert_eq!(i.error.as_ref().map(|e| e.category), Some(Category::Spawn));
    assert_eq!(i.steps[0].status, Status::Failed);

    let (i, _) = one("handoff", json!({"steps": [{"plugin": {"argv": [FAKE, "plugin-handoff", "--project", "{project}"]}}]}), None);
    assert_eq!(i.status, Status::NeedsAttention);
    let e = i.error.unwrap();
    assert_eq!(e.category, Category::Handoff);
    assert!(e.message.contains("needs the agent"), "{}", e.message);
}

#[test]
fn input_projects_and_template_projects_are_copied() {
    let b = Bench::new("copy", &[]);
    std::fs::create_dir_all(b.dir.join("proj")).unwrap();
    std::fs::write(b.dir.join("proj/p.fcproj"), "orig\n").unwrap();
    let p = b.pipeline(json!({"inputs": ["proj/p.fcproj"], "steps": [{"command": "x.y"}]}));
    let job = Job::create(&p, Some("copy")).unwrap();
    assert_eq!(run(&job, &b.opts()).unwrap().status, Status::Done);
    assert_eq!(b.read("p.fcproj"), "orig\nx.y\n");
    assert_eq!(std::fs::read_to_string(b.dir.join("proj/p.fcproj")).unwrap(), "orig\n", "the input is untouched");

    let b = Bench::new("template", &["a"]);
    std::fs::write(b.dir.join("tpl.fcproj"), "tpl\n").unwrap();
    let p = b.pipeline(json!({"template": {"project": "tpl.fcproj"}, "steps": [{"command": "x.y"}]}));
    let job = Job::create(&p, Some("tpl")).unwrap();
    assert_eq!(run(&job, &b.opts()).unwrap().status, Status::Done);
    assert_eq!(b.read("a.fcproj"), "tpl\nx.y\n");
}

#[test]
fn an_existing_project_or_job_id_is_refused() {
    let b = Bench::new("exists", &["a"]);
    let p = b.pipeline(json!({}));
    std::fs::create_dir_all(b.dir.join("out")).unwrap();
    std::fs::write(b.out("a.fcproj"), "mine").unwrap();
    assert!(Job::create(&p, Some("x")).unwrap_err().contains("already exists"));
    std::fs::remove_file(b.out("a.fcproj")).unwrap();
    Job::create(&p, Some("x")).unwrap();
    assert!(Job::create(&p, Some("x")).unwrap_err().contains("already exists"));
}
