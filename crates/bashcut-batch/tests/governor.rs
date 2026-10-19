//! The resource governor (#61) against the fake `filmcraft-cli` and a [`FakeProbe`]: concurrency from
//! free RAM, a step over the memory cap stopped and retried later, a batch paused on low disk and going
//! on when space is freed, cancel while paused, the resource report.

use bashcut_batch::{Category, FakeProbe, Job, Options, Pipeline, SCHEMA, Status, run};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FAKE: &str = env!("CARGO_BIN_EXE_bashcut-batch-fake-cli");

struct Bench {
    dir: PathBuf,
}

impl Bench {
    fn new(tag: &str, clips: usize) -> Bench {
        let dir = std::env::temp_dir().join(format!("bashcut-governor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("clips")).unwrap();
        for i in 0..clips {
            std::fs::write(dir.join("clips").join(format!("c{i}.mp4")), b"media").unwrap();
        }
        Bench { dir }
    }

    fn job(&self, extra: Value) -> Job {
        let mut v = json!({"schema": SCHEMA, "name": "gov", "inputs": ["clips"], "projectsFolder": "out"});
        if let (Some(m), Value::Object(e)) = (v.as_object_mut(), extra) {
            m.extend(e);
        }
        let p = Pipeline::parse(&v.to_string(), &self.dir).unwrap();
        Job::create(&p, Some("gov")).unwrap()
    }

    fn opts(&self, probe: &Arc<FakeProbe>) -> Options {
        let mut o = Options::new(FAKE);
        o.poll = Duration::from_millis(10);
        o.watch_every = Duration::from_millis(20);
        o.probe = probe.clone();
        o.env.push(("FAKE_CLI_LOG".into(), self.dir.join("fake-cli.log").to_string_lossy().into_owned()));
        o
    }

    fn max_concurrent(&self) -> i32 {
        max_concurrent(&self.dir.join("fake-cli.log"))
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn max_concurrent(log: &Path) -> i32 {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let mut events: Vec<(u128, i32)> = text
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let kind = w.next()?;
            let t: u128 = w.next()?.parse().ok()?;
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

const GB: u64 = 1 << 30;

#[test]
fn concurrency_follows_free_ram() {
    let b = Bench::new("ram", 4);
    let job = b.job(json!({"concurrency": 4, "steps": [{"command": "fake.sleep", "params": {"ms": 200}}]}));
    let probe = Arc::new(FakeProbe::roomy());
    probe.set(|s| s.free_memory = Some(4 * GB));
    let sum = run(&job, &b.opts(&probe)).unwrap();
    assert_eq!(sum.status, Status::Done, "{sum:?}");
    assert_eq!(b.max_concurrent(), 1, "4 GB free at 3 GB per item: one at a time");
    assert_eq!(sum.resources.concurrency, 1);
    assert_eq!(sum.resources.items_finished, 4);
    assert!(sum.resources.steps.iter().any(|s| s.step == "fake.sleep" && s.count == 4), "{:?}", sum.resources.steps);
}

#[test]
fn a_step_over_the_memory_cap_is_stopped_and_retried_later() {
    let b = Bench::new("cap", 1);
    let job = b.job(json!({"steps": [{"command": "fake.sleep", "params": {"ms": 3000}}], "resources": {"maxItemMemoryGb": 8}}));
    let probe = Arc::new(FakeProbe::roomy());
    probe.set(|s| s.process_memory = Some(10 * GB));
    let start = Instant::now();
    let sum = run(&job, &b.opts(&probe)).unwrap();
    assert!(start.elapsed() < Duration::from_millis(2500), "the process was stopped, not waited for");
    let book = job.book().unwrap();
    let item = &book.items[0];
    assert_eq!(item.status, Status::NotRun, "{item:?}");
    assert_eq!(item.error.as_ref().map(|e| e.category), Some(Category::Resources));
    assert!(item.steps.iter().any(|s| s.status == Status::Pending), "the stopped step runs again");
    assert!(sum.reports.iter().any(|r| r.contains("memory cap")), "{:?}", sum.reports);
    assert!(sum.resources.peak_memory_mb >= 10 * 1024);

    // resume with normal memory: the item finishes
    probe.set(|s| s.process_memory = Some(500 << 20));
    let sum = run(&job, &b.opts(&probe)).unwrap();
    assert_eq!(sum.status, Status::Done, "{sum:?}");
}

#[test]
fn low_disk_pauses_the_batch_until_space_is_freed() {
    let b = Bench::new("disk", 2);
    let job = b.job(json!({"concurrency": 2, "steps": [{"command": "a.b"}], "outputs": [{"name": "{name}.mp4", "preset": "p"}]}));
    let probe = Arc::new(FakeProbe::roomy());
    probe.set(|s| s.free_disk = Some(GB));
    let opts = b.opts(&probe);
    let (job2, probe2) = (job.clone(), probe.clone());
    let watcher = std::thread::spawn(move || {
        let start = Instant::now();
        loop {
            if job2.book().ok().and_then(|bk| bk.paused).is_some_and(|m| m.contains("low disk space")) {
                probe2.set(|s| s.free_disk = Some(100 * GB));
                return true;
            }
            if start.elapsed() > Duration::from_secs(20) {
                probe2.set(|s| s.free_disk = Some(100 * GB));
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let sum = run(&job, &opts).unwrap();
    assert!(watcher.join().unwrap(), "the book said why the batch paused");
    assert_eq!(sum.status, Status::Done, "{sum:?}");
    assert_eq!(job.book().unwrap().paused, None);
    assert!(b.dir.join("out").join("exports").join("c0.mp4").is_file());
}

#[test]
fn cancel_while_paused_on_disk() {
    let b = Bench::new("diskcancel", 1);
    let job = b.job(json!({"steps": [{"command": "a.b"}]}));
    let probe = Arc::new(FakeProbe::roomy());
    probe.set(|s| s.free_disk = Some(0));
    let job2 = job.clone();
    let canceller = std::thread::spawn(move || {
        let start = Instant::now();
        while job2.book().ok().and_then(|bk| bk.paused).is_none() && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(10));
        }
        job2.request_cancel().unwrap();
    });
    let sum = run(&job, &b.opts(&probe)).unwrap();
    canceller.join().unwrap();
    assert_eq!(sum.status, Status::Cancelled, "{sum:?}");
    assert_eq!(job.book().unwrap().items[0].status, Status::Cancelled);
}

#[test]
fn hostile_resource_limits_are_refused() {
    let b = Bench::new("limits", 1);
    for r in [json!({"memoryPerItemGb": -1}), json!({"maxItemMemoryGb": 1e9}), json!({"agentConcurrency": 0})] {
        let v = json!({"schema": SCHEMA, "inputs": ["clips"], "projectsFolder": "out", "resources": r});
        assert!(Pipeline::parse(&v.to_string(), &b.dir).is_err(), "{r}");
    }
}
