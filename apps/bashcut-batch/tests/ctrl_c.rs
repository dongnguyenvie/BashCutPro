//! Ctrl+C on `bashcut-batch run` cancels the job cleanly (FX8): the runner stops the running step
//! with its process group, so what the step started (an agent's python, ffmpeg) goes too, and the
//! book ends cancelled. Unix only: a test cannot send a console Ctrl+C on Windows without `unsafe`
//! (there the step's tree kill is tested in `bashcut-batch` `process.rs`).

#![cfg(unix)]

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BATCH: &str = env!("CARGO_BIN_EXE_bashcut-batch");

fn wait_for(limit: Duration, mut ok: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    ok()
}

fn signal(sig: &str, pid: u32) {
    let _ = Command::new("kill").args([sig, &pid.to_string()]).status();
}

#[test]
fn ctrl_c_cancels_the_job_and_ends_the_steps_grandchildren() {
    let dir = std::env::temp_dir().join(format!("bashcut-batch-ctrl-c-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("clips")).unwrap();
    std::fs::write(dir.join("clips/a.mp4"), b"media").unwrap();
    // an engine that saves the project it is asked for, so the item reaches its step
    let engine = dir.join("engine.sh");
    std::fs::write(&engine, "#!/bin/sh\nwhile [ $# -gt 0 ]; do [ \"$1\" = \"--save-as\" ] && echo project > \"$2\"; shift; done\necho '{\"ok\":true}'\n")
        .unwrap();
    let _ = Command::new("chmod").arg("+x").arg(&engine).status();
    // the step (a fake agent) starts a grandchild, writes its pid and waits for it
    let pid_file = dir.join("grandchild.pid");
    let script = format!("sleep 30 & echo $! > '{}'; wait", pid_file.display());
    let pipeline = serde_json::json!({
        "schema": bashcut_batch::SCHEMA, "name": "t", "inputs": ["clips"], "projectsFolder": "out",
        "steps": [{"plugin": {"argv": ["sh", "-c", script]}}],
    });
    std::fs::write(dir.join("p.json"), pipeline.to_string()).unwrap();

    let mut runner = Command::new(BATCH)
        .args(["run", "p.json", "--compact", "--no-keep-awake", "--cli"])
        .arg(&engine)
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let read_pid = || std::fs::read_to_string(&pid_file).ok().and_then(|s| s.trim().parse::<u32>().ok());
    if !wait_for(Duration::from_secs(20), || read_pid().is_some()) {
        let _ = runner.kill();
        let out = runner.wait_with_output().unwrap();
        panic!("the step never started: {}", String::from_utf8_lossy(&out.stderr));
    }
    let grandchild = read_pid().unwrap();

    // what the terminal sends on Ctrl+C (the step is in its own group, so only the runner gets it)
    signal("-INT", runner.id());
    let ended = wait_for(Duration::from_secs(15), || runner.try_wait().ok().flatten().is_some());
    if !ended {
        let _ = runner.kill();
    }
    let out = runner.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(ended, "the runner did not stop after Ctrl+C: {stderr}");
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("cancelling"), "{stderr}");
    let summary: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(summary["status"], "cancelled", "{summary}");
    assert_eq!(summary["counts"]["cancelled"], 1, "{summary}");

    let gone = wait_for(Duration::from_secs(5), || !bashcut_dirs::pid_alive(grandchild));
    if !gone {
        signal("-KILL", grandchild);
    }
    assert!(gone, "the step's grandchild {grandchild} outlived Ctrl+C");
    let _ = std::fs::remove_dir_all(Path::new(&dir));
}
