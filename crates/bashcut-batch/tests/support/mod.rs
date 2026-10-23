//! What `tests/batch.rs` and `tests/governor.rs` share: a scratch [`Bench`] wired to the fake
//! `filmcraft-cli` (`fake_cli.rs`), and concurrency counts from the fake's `start` / `end` log.

#![allow(dead_code)]

use bashcut_batch::{FakeProbe, Job, Options, Pipeline, SCHEMA};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const FAKE: &str = env!("CARGO_BIN_EXE_bashcut-batch-fake-cli");

/// A scratch folder `<temp>/<name>-<pid>` with `clips/<clip>.mp4` files, removed on drop.
pub struct Bench {
    pub dir: PathBuf,
}

impl Bench {
    pub fn new(name: &str, clips: impl IntoIterator<Item = String>) -> Bench {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("clips")).unwrap();
        for c in clips {
            std::fs::write(dir.join("clips").join(format!("{c}.mp4")), b"media").unwrap();
        }
        Bench { dir }
    }

    /// A pipeline named `name` over `clips` into `out`, with `extra`'s fields added.
    pub fn pipeline_named(&self, name: &str, extra: Value) -> Pipeline {
        let mut v = json!({"schema": SCHEMA, "name": name, "inputs": ["clips"], "projectsFolder": "out"});
        if let (Some(m), Value::Object(e)) = (v.as_object_mut(), extra) {
            m.extend(e);
        }
        Pipeline::parse(&v.to_string(), &self.dir).unwrap()
    }

    pub fn pipeline(&self, extra: Value) -> Pipeline {
        self.pipeline_named("test", extra)
    }

    /// A job `gov` of the pipeline `gov` (governor tests).
    pub fn job(&self, extra: Value) -> Job {
        Job::create(&self.pipeline_named("gov", extra), Some("gov")).unwrap()
    }

    /// Options for the fake engine with `probe`, polling every 10 ms, logging to [`Bench::log`].
    pub fn opts_with(&self, probe: Arc<FakeProbe>) -> Options {
        let mut o = Options::new(FAKE);
        o.poll = Duration::from_millis(10);
        o.probe = probe;
        o.env.push(("FAKE_CLI_LOG".into(), self.log().to_string_lossy().into_owned()));
        o
    }

    /// [`Bench::opts_with`] a roomy probe: the machine's real RAM and cores must not change the
    /// batch tests (governor: `tests/governor.rs`).
    pub fn opts(&self) -> Options {
        self.opts_with(Arc::new(FakeProbe::roomy()))
    }

    /// [`Bench::opts_with`] `probe`, memory checked every 20 ms (governor tests).
    pub fn governed(&self, probe: &Arc<FakeProbe>) -> Options {
        let mut o = self.opts_with(probe.clone());
        o.watch_every = Duration::from_millis(20);
        o
    }

    pub fn log(&self) -> PathBuf {
        self.dir.join("fake-cli.log")
    }

    pub fn out(&self, rel: &str) -> PathBuf {
        self.dir.join("out").join(rel)
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.out(rel)).unwrap_or_default()
    }

    pub fn max_concurrent(&self) -> i32 {
        max_concurrent(&self.log())
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The most fake-cli processes that ran at the same time, from the log.
pub fn max_concurrent(log: &Path) -> i32 {
    max_overlap(log, "start", "end")
}

/// The most `start` … `end` stretches of the log (by kind) that overlapped; other kinds are ignored.
pub fn max_overlap(log: &Path, start: &str, end: &str) -> i32 {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let mut events: Vec<(u128, i32)> = text
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let kind = w.next()?;
            let t: u128 = w.next()?.parse().ok()?;
            // at the same millisecond an end comes before a start
            if kind == start {
                Some((t, 1))
            } else if kind == end {
                Some((t, -1))
            } else {
                None
            }
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
