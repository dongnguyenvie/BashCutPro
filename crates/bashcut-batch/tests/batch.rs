//! The batch runner against a fake `filmcraft-cli` (`tests/support/fake_cli.rs`): parallel items,
//! failures with skip and stop, cancel, resume after a kill (tripwire), error categories.

use bashcut_batch::{Category, ItemState, Job, Pipeline, Status, run};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod support;

use support::{Bench, FAKE, max_concurrent, max_overlap};

fn bench(tag: &str, clips: &[&str]) -> Bench {
    Bench::new(&format!("bashcut-batch-{tag}"), clips.iter().map(|c| c.to_string()))
}

fn item<'a>(book: &'a bashcut_batch::Book, name: &str) -> &'a ItemState {
    book.items.iter().find(|i| i.name == name).unwrap()
}

#[test]
fn parallel_items_each_get_a_project_and_exports() {
    let b = bench("parallel", &["a", "b", "c", "d"]);
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
fn profile_outputs_export_through_the_engine_command() {
    let b = bench("profile", &["a"]);
    let p = b.pipeline(json!({"outputs": [{"name": "{name}-tiktok.mp4", "profile": "tiktok"}]}));
    let job = Job::create(&p, Some("profile")).unwrap();
    assert_eq!(run(&job, &b.opts()).unwrap().status, Status::Done);
    assert!(b.read("exports/a-tiktok.mp4").starts_with("export profile=tiktok\n"));
    let script = std::fs::read_to_string(job.item_dir("a").join("export-1.jsonl")).unwrap();
    assert!(script.contains("bashcut.profiles.apply") && script.contains("\"wait\":true"), "{script}");
    assert_eq!(b.read("a.fcproj"), "file.import\nfile.newSequenceFromClip\n", "an export does not save the project");
}

#[test]
fn plugin_steps_get_the_cli_and_plugins_placeholders() {
    let b = bench("vars", &["a"]);
    // the "plugin" is the fake CLI ({cli}) running a script from the plugins folder ({plugins})
    let plugins = b.dir.join("data").join("bashcut-plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::write(plugins.join("probe.jsonl"), "{\"id\":\"probe\"}\n").unwrap();
    let p = b.pipeline(json!({"steps": [{"plugin": {"argv": ["{cli}", "run", "{plugins}/probe.jsonl", "--project", "{project}", "--save"]}}]}));
    let job = Job::create(&p, Some("vars")).unwrap();
    let mut opts = b.opts();
    opts.data_dir = Some(b.dir.join("data"));
    assert_eq!(run(&job, &opts).unwrap().status, Status::Done);
    assert_eq!(b.read("a.fcproj"), "file.import\nfile.newSequenceFromClip\nprobe\n");
}

#[test]
fn a_failing_item_is_skipped_and_the_others_finish() {
    let b = bench("skip", &["a", "b", "c"]);
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
    let b = bench("stop", &["a", "b", "c"]);
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
    let b = bench("cancel", &["a", "b"]);
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
    let b = bench("trip", &["a", "b"]);
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
    let b = bench("lock", &["a"]);
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
    let b = bench(tag, &["x"]);
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
    let b = bench("copy", &[]);
    std::fs::create_dir_all(b.dir.join("proj")).unwrap();
    std::fs::write(b.dir.join("proj/p.fcproj"), "orig\n").unwrap();
    let p = b.pipeline(json!({"inputs": ["proj/p.fcproj"], "steps": [{"command": "x.y"}]}));
    let job = Job::create(&p, Some("copy")).unwrap();
    assert_eq!(run(&job, &b.opts()).unwrap().status, Status::Done);
    assert_eq!(b.read("p.fcproj"), "orig\nx.y\n");
    assert_eq!(std::fs::read_to_string(b.dir.join("proj/p.fcproj")).unwrap(), "orig\n", "the input is untouched");

    let b = bench("template", &["a"]);
    std::fs::write(b.dir.join("tpl.fcproj"), "tpl\n").unwrap();
    let p = b.pipeline(json!({"template": {"project": "tpl.fcproj"}, "steps": [{"command": "x.y"}]}));
    let job = Job::create(&p, Some("tpl")).unwrap();
    assert_eq!(run(&job, &b.opts()).unwrap().status, Status::Done);
    assert_eq!(b.read("a.fcproj"), "tpl\nx.y\n");
}

#[test]
fn an_existing_project_or_job_id_is_refused() {
    let b = bench("exists", &["a"]);
    let p = b.pipeline(json!({}));
    std::fs::create_dir_all(b.dir.join("out")).unwrap();
    std::fs::write(b.out("a.fcproj"), "mine").unwrap();
    assert!(Job::create(&p, Some("x")).unwrap_err().contains("already exists"));
    std::fs::remove_file(b.out("a.fcproj")).unwrap();
    Job::create(&p, Some("x")).unwrap();
    assert!(Job::create(&p, Some("x")).unwrap_err().contains("already exists"));
}

/// An agent step driven by the fake agent in `mode`.
fn agent_pipeline(b: &Bench, mode: &str, extra: Value) -> Pipeline {
    agent_pipeline_with(b, mode, extra, json!({}))
}

fn agent_pipeline_with(b: &Bench, mode: &str, extra: Value, more: Value) -> Pipeline {
    let mut agent = json!({"skill": "captions-text", "brief": "Fix the captions of {name}.", "argv": [FAKE, "fake-agent", "--mode", mode, "--prompt", "{prompt}", "--session", "{session}", "--project", "{project}"]});
    if let (Some(m), Value::Object(e)) = (agent.as_object_mut(), extra) {
        m.extend(e);
    }
    let mut p = json!({"steps": [{"agent": agent}]});
    if let (Some(m), Value::Object(e)) = (p.as_object_mut(), more) {
        m.extend(e);
    }
    b.pipeline(p)
}

#[test]
fn an_agent_step_is_reviewed_and_fixed_in_a_resumed_round() {
    let b = bench("agent-ok", &["a"]);
    let job = Job::create(&agent_pipeline(&b, "ok", json!({})), Some("agent")).unwrap();
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Done, "{summary:?}");
    let book = job.book().unwrap();
    let step = &item(&book, "a").steps[1];
    let usage = step.agent.as_ref().unwrap();
    assert_eq!((usage.agent.as_str(), usage.rounds, usage.turns, usage.input_tokens, usage.output_tokens), ("custom", 2, 6, 200, 40));
    assert!((usage.cost_usd - 0.02).abs() < 1e-9);
    assert_eq!(usage.transcripts.len(), 2);
    assert!(usage.session.as_deref().is_some_and(|s| s.starts_with("sess-")));
    assert_eq!(step.review.as_ref().map(|r| r["verdict"].clone()), Some(json!("pass")));
    let project = b.read("a.fcproj");
    assert!(project.contains("resume:sess-"), "the fix round resumed the session: {project}");
    assert!(project.contains("fixed\n"));
    assert_eq!((summary.agent.steps, summary.agent.rounds, summary.agent.turns), (1, 2, 6));
    // the fix prompt named the finding
    let fix = std::fs::read_to_string(job.item_dir("a").join("step-1.round-1.out")).unwrap_or_default();
    assert!(fix.contains("\"type\":\"result\""), "{fix}");
}

#[test]
fn a_clean_first_round_needs_no_fix_round() {
    let b = bench("agent-first", &["a"]);
    let job = Job::create(&agent_pipeline(&b, "fixfirst", json!({})), Some("agent")).unwrap();
    assert_eq!(run(&job, &b.opts()).unwrap().status, Status::Done);
    let book = job.book().unwrap();
    assert_eq!(item(&book, "a").steps[1].agent.as_ref().map(|u| u.rounds), Some(1));
}

#[test]
fn an_agent_out_of_turns_still_exports_its_last_save_and_needs_attention() {
    let b = bench("agent-loop", &["a"]);
    let job = Job::create(&agent_pipeline_with(&b, "loop", json!({"maxTurns": 3}), json!({"outputs": [{"name": "{name}.mp4"}]})), Some("agent")).unwrap();
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::NeedsAttention, "{summary:?}");
    let book = job.book().unwrap();
    let a = item(&book, "a");
    assert_eq!(a.status, Status::NeedsAttention);
    let e = a.error.clone().unwrap();
    assert_eq!(e.category, Category::Limit);
    assert!(e.message.contains("maxTurns (3)") && e.message.contains("last saved project"), "{}", e.message);
    // no review round on an unfinished project, but the export ran on what the agent saved
    assert_eq!(a.steps[1].status, Status::Done);
    assert!(a.steps[1].review.is_none());
    assert!(b.out("exports/a.mp4").exists(), "exported from the last save");
    assert!(b.read("a.fcproj").contains(
        "agent
"
    ));

    // retry runs the agent and the export again
    assert_eq!(job.retry(&[]).unwrap(), 1);
    let book = job.book().unwrap();
    assert!(item(&book, "a").steps[1..].iter().all(|s| s.status == Status::Pending && s.error.is_none()));
    run(&job, &b.opts()).unwrap();
    let book = job.book().unwrap();
    assert_eq!(item(&book, "a").steps[1].attempts, 2);
    assert_eq!(item(&book, "a").steps[2].attempts, 2);
    // a person accepts the result: mark-done finishes the item without running anything again
    assert_eq!(job.retry_with(&["a".to_string()], true).unwrap(), 1);
    assert_eq!(run(&job, &b.opts()).unwrap().status, Status::Done);
    let book = job.book().unwrap();
    assert_eq!(item(&book, "a").steps[1].attempts, 2);
}

#[test]
fn an_agent_past_max_minutes_is_stopped() {
    let b = bench("agent-sleep", &["a"]);
    let job = Job::create(&agent_pipeline(&b, "sleep", json!({"maxMinutes": 0.02})), Some("agent")).unwrap();
    let start = Instant::now();
    run(&job, &b.opts()).unwrap();
    assert!(start.elapsed() < Duration::from_secs(6), "the 10 s agent was killed");
    let book = job.book().unwrap();
    assert_eq!(item(&book, "a").error.as_ref().map(|e| e.category), Some(Category::Timeout));
}

#[test]
fn findings_left_after_the_fix_rounds_end_needs_fix() {
    let b = bench("agent-stubborn", &["a"]);
    let job = Job::create(&agent_pipeline(&b, "stubborn", json!({"fixRounds": 1})), Some("agent")).unwrap();
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::NeedsFix, "{summary:?}");
    assert_eq!(summary.counts.needs_fix, 1);
    let book = job.book().unwrap();
    let a = item(&book, "a");
    assert_eq!(a.status, Status::NeedsFix);
    let e = a.error.clone().unwrap();
    assert_eq!(e.category, Category::Review);
    assert!(e.message.contains("captions.readingSpeed") && e.message.contains("1 fix round"), "{}", e.message);
    assert_eq!(a.steps[1].agent.as_ref().map(|u| u.rounds), Some(2));
    // retry reopens a needs-fix item
    assert_eq!(job.retry(&[]).unwrap(), 1);
}

#[test]
fn every_sequence_the_agent_made_is_reviewed_and_fixed() {
    // #195: the agent cuts three clips and leaves the clean one active; the gate reviews all three
    let b = bench("agent-clips", &["a"]);
    let job = Job::create(&agent_pipeline(&b, "clips", json!({})), Some("agent")).unwrap();
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Done, "{summary:?}");
    let book = job.book().unwrap();
    let step = &item(&book, "a").steps[1];
    assert_eq!(step.agent.as_ref().map(|u| u.rounds), Some(2), "the clip findings got a fix round");
    let first = std::fs::read_to_string(job.item_dir("a").join("step-1.review-0.jsonl")).unwrap();
    assert_eq!(first.matches("bashcut.review.run").count(), 3, "{first}");
    let review = step.review.clone().unwrap();
    assert_eq!(review["verdict"], "pass");
    let names: Vec<&str> = review["sequences"].as_array().unwrap().iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(names, vec!["clip-1", "clip-2", "clip-3"], "the source sequence is not an output");
    let project = b.read("a.fcproj");
    // the fix prompt named the clips with findings, not the clean one; the active clip is active again
    assert!(project.contains("fixed:clip-1\n") && project.contains("fixed:clip-3\n") && !project.contains("fixed:clip-2"), "{project}");
    assert!(project.trim_end().ends_with("sequence.open item=4"), "{project}");
}

#[test]
fn findings_left_in_any_sequence_end_needs_fix_and_survive_a_retry() {
    let b = bench("agent-clips-stubborn", &["a"]);
    let job = Job::create(&agent_pipeline(&b, "clipsstubborn", json!({"fixRounds": 1})), Some("agent")).unwrap();
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::NeedsFix, "{summary:?}");
    let book = job.book().unwrap();
    let e = item(&book, "a").error.clone().unwrap();
    assert_eq!(e.category, Category::Review);
    assert!(
        e.message.starts_with("step-1: 2 blocking")
            && e.message.contains("clip-1: captions.readingSpeed")
            && e.message.contains("clip-3: captions.readingSpeed"),
        "{}",
        e.message
    );
    // a retry runs the agent on the same project: its clips are still outputs, not sources
    assert_eq!(job.retry(&[]).unwrap(), 1);
    assert_eq!(run(&job, &b.opts()).unwrap().status, Status::NeedsFix);
    let book = job.book().unwrap();
    let review = item(&book, "a").steps[1].review.clone().unwrap();
    assert_eq!(review["blocking"], 2, "{review}");
}

#[test]
fn the_kit_reaches_agent_and_plugin_steps() {
    // #204: `--kit` is `{kit}`, `BASHCUT_KIT` and a line of the agent's prompt
    let b = bench("kit", &["a"]);
    let kit = b.dir.join("kit");
    std::fs::create_dir_all(kit.join("skills")).unwrap();
    let mut p = agent_pipeline(&b, "fixfirst", json!({"review": false}));
    p.steps.push(
        serde_json::from_value(
            json!({"plugin": {"argv": [FAKE, "fake-agent", "--mode", "x", "--prompt", "{kit}/skills/<name>/SKILL.md", "--project", "{project}"]}}),
        )
        .unwrap(),
    );
    let mut o = b.opts();
    o.kit = Some(kit.clone());
    let summary = run(&Job::create(&p, Some("kit")).unwrap(), &o).unwrap();
    assert_eq!(summary.status, Status::Done, "{summary:?}");
    let project = b.read("a.fcproj");
    assert_eq!(project.matches(&format!("kit-env:{}\n", kit.display())).count(), 2, "{project}");
    assert_eq!(project.matches("kit-in-prompt\n").count(), 2, "{project}");
    // #223: one interpreter name for the kit's scripts, as BASHCUT_PYTHON and in the agent's prompt
    let python = bashcut_batch::python::agent_python();
    assert!(!python.contains(' '), "{python}");
    assert_eq!(project.matches(&format!("python-env:{python}\n")).count(), 2, "{project}");
    assert_eq!(project.matches("python-in-prompt\n").count(), 1, "the agent's prompt only: {project}");
    // without a kit, a step that needs one says how to give it
    let b = bench("nokit", &["a"]);
    let p = b.pipeline(json!({"steps": [{"plugin": {"argv": ["python3", "{kit}/skills/captions-text/captions_fit.py", "apply"]}}]}));
    let summary = run(&Job::create(&p, Some("nokit")).unwrap(), &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Failed, "{summary:?}");
    let book = Job::open(b.dir.join("out/.bashcut-batch/nokit")).unwrap().book().unwrap();
    let err = item(&book, "a").steps[1].error.clone().unwrap();
    assert_eq!(err.category, Category::Usage);
    assert!(err.message.contains("--kit"), "{}", err.message);
}

#[test]
fn agent_steps_respect_agent_concurrency() {
    let b = bench("agent-cap", &["a", "b", "c"]);
    let mut p = agent_pipeline(&b, "fixfirst", json!({"review": false}));
    if let Some(a) = p.steps[0].agent.as_mut() {
        a.argv.extend(["--sleep-ms".to_string(), "300".to_string()]);
    }
    p.concurrency = 3;
    p.agent_concurrency = 1;
    let job = Job::create(&p, Some("cap")).unwrap();
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Done);
    assert_eq!(max_overlap(&b.log(), "agent-start", "agent-end"), 1, "one agent at a time");
    let book = job.book().unwrap();
    assert!(book.items.iter().all(|i| i.steps[1].review.is_none()), "review: false");
}

#[test]
fn transcribe_steps_use_the_settings_speech_model_when_downloaded() {
    // filmcraft-cli does not read preferences.json: the runner passes the Settings model (#172)
    let b = bench("speech", &["a"]);
    let data = b.dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("preferences.json"), r#"{"mediaAnalysis": {"whisperModel": "whisper-small"}}"#).unwrap();
    let models = |installed: &[&str]| {
        let list: Vec<Value> =
            ["whisper-tiny", "whisper-base", "whisper-small"].iter().map(|id| json!({"id": id, "installed": installed.contains(id)})).collect();
        json!({"available": true, "default": "whisper-base", "models": list}).to_string()
    };
    let p = b.pipeline(json!({"steps": [
        {"command": "sequence.transcribe", "params": {"language": "auto"}},
        {"commands": [{"id": "sequence.transcribe", "params": {"model": "whisper-base"}}]}
    ]}));
    let run_with = |id: &str, installed: &[&str], flag: Option<&str>| {
        let _ = std::fs::remove_file(b.out("a.fcproj"));
        let job = Job::create(&p, Some(id)).unwrap();
        let mut o = b.opts();
        o.data_dir = Some(data.clone());
        o.env.push(("FAKE_CLI_MODELS".into(), models(installed)));
        o.speech_model = flag.map(str::to_string);
        assert_eq!(run(&job, &o).unwrap().status, Status::Done);
        b.read("a.fcproj")
    };
    // each transcribe line is followed by the doubtful-text clean (#177)
    let steps = |model: &str| {
        format!(
            "file.import\nfile.newSequenceFromClip\nsequence.transcribe model={model}\nbashcut.transcript.clean\nsequence.transcribe model=whisper-base\nbashcut.transcript.clean\n"
        )
    };
    // the Settings choice is downloaded: used; a step that names its model keeps it
    assert_eq!(run_with("s1", &["whisper-base", "whisper-small"], None), steps("whisper-small"));
    // only another model is downloaded: that one
    assert_eq!(run_with("s2", &["whisper-tiny"], None), steps("whisper-tiny"));
    // --speech-model wins
    assert_eq!(run_with("s3", &["whisper-small"], Some("whisper-tiny")), steps("whisper-tiny"));
}

#[test]
fn resources_agent_concurrency_raises_the_agent_cap() {
    let b = bench("agent-res-cap", &["a", "b", "c"]);
    let mut p = agent_pipeline(&b, "fixfirst", json!({"review": false}));
    if let Some(a) = p.steps[0].agent.as_mut() {
        a.argv.extend(["--sleep-ms".to_string(), "800".to_string()]);
    }
    p.concurrency = 3;
    assert_eq!(p.agent_concurrency, 1, "the top-level default");
    p.resources.agent_concurrency = 2;
    let job = Job::create(&p, Some("rescap")).unwrap();
    let summary = run(&job, &b.opts()).unwrap();
    assert_eq!(summary.status, Status::Done, "{summary:?}");
    assert_eq!(max_overlap(&b.log(), "agent-start", "agent-end"), 2, "two agents at a time");
}

/// Every process the fake CLI ran, from `FAKE_CLI_ARGS`: (argv without the program, BASHCUT_BRIDGE).
fn children(log: &std::path::Path) -> Vec<(Vec<String>, Option<String>)> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| {
            let argv = v["argv"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
            (argv, v["bridge"].as_str().map(str::to_string))
        })
        .collect()
}

#[test]
fn engine_children_get_the_data_dir_and_every_child_the_bridge_off() {
    let b = bench("data-dir", &["a"]);
    let data = b.dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let args = b.dir.join("fake-cli-args.log");
    let mut agent = json!({"skill": "captions-text", "argv": [FAKE, "fake-agent", "--mode", "fixfirst", "--prompt", "{prompt}", "--session", "{session}", "--project", "{project}"]});
    agent["fixRounds"] = json!(0);
    let p = b.pipeline(json!({
        "steps": [
            {"command": "sequence.transcribe"},
            {"commands": [{"id": "captions.auto"}, {"id": "captions.style"}]},
            {"plugin": {"argv": [FAKE, "plugin-ok", "--project", "{project}"]}},
            {"agent": agent}
        ],
        "outputs": [{"name": "{name}.mp4", "preset": "TikTok"}, {"name": "{name}-reels.mp4", "profile": "reels"}]
    }));
    let job = Job::create(&p, Some("dd")).unwrap();
    let mut o = b.opts();
    o.data_dir = Some(data.clone());
    o.env.push(("FAKE_CLI_ARGS".into(), args.to_string_lossy().into_owned()));
    o.env.push((
        "FAKE_CLI_MODELS".into(),
        json!({"available": true, "default": "whisper-base", "models": [{"id": "whisper-base", "installed": true}]}).to_string(),
    ));
    let summary = run(&job, &o).unwrap();
    assert_eq!(summary.status, Status::Done, "{summary:?}");

    let all = children(&args);
    let data_arg = data.to_string_lossy().into_owned();
    let (engine, others): (Vec<_>, Vec<_>) = all.iter().partition(|(argv, _)| !matches!(argv.first().map(String::as_str), Some("plugin-ok" | "fake-agent")));
    let first = |c: &&(Vec<String>, Option<String>)| c.0.first().cloned().unwrap_or_default();
    // the speech-model probe, create, two engine steps, the agent's inspect and review, two exports
    assert_eq!(engine.iter().filter(|c| first(c) == "exec").count(), 1, "{engine:?}");
    assert_eq!(engine.iter().filter(|c| first(c) == "export").count(), 1, "{engine:?}");
    assert!(engine.iter().filter(|c| first(c) == "run").count() >= 6, "{engine:?}");
    for (argv, _) in &engine {
        assert_eq!(argv[argv.len().saturating_sub(2)..], ["--data-dir".to_string(), data_arg.clone()], "{argv:?}");
    }
    assert_eq!(others.iter().map(first).collect::<Vec<_>>(), vec!["plugin-ok", "fake-agent"]);
    for (argv, bridge) in &all {
        assert_eq!(bridge.as_deref(), Some("off"), "{argv:?}");
    }
}

#[test]
fn step_children_get_the_data_dir() {
    // FX9: plugin and agent steps see BASHCUTPRO_DATA_DIR = --data-dir, else the app's data folder;
    // a plugin step's own env value for it wins (the agent step next to it keeps the folder)
    let data = std::env::temp_dir().join(format!("bashcut-batch-step-data-dir-{}", std::process::id()));
    let seen = |tag: &str, data_dir: Option<&PathBuf>, step_env: Value| {
        let b = bench(&format!("step-data-dir-{tag}"), &["a"]);
        let args = b.dir.join(format!("fake-cli-args-{tag}.log"));
        let mut agent = json!({"skill": "captions-text", "argv": [FAKE, "fake-agent", "--mode", "fixfirst", "--prompt", "{prompt}", "--session", "{session}", "--project", "{project}"]});
        agent["fixRounds"] = json!(0);
        let p = b.pipeline(json!({"steps": [{"plugin": {"argv": [FAKE, "plugin-ok", "--project", "{project}"], "env": step_env}}, {"agent": agent}]}));
        let job = Job::create(&p, Some(tag)).unwrap();
        let mut o = b.opts();
        o.data_dir = data_dir.cloned();
        o.env.push(("FAKE_CLI_ARGS".into(), args.to_string_lossy().into_owned()));
        let summary = run(&job, &o).unwrap();
        assert_eq!(summary.status, Status::Done, "{tag}: {summary:?}");
        let lines = std::fs::read_to_string(&args).unwrap_or_default();
        let by = |first: &str| {
            lines
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(|v| v["argv"][0] == json!(first))
                .map(|v| v["dataDir"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
        };
        (by("plugin-ok"), by("fake-agent"))
    };
    let ours = Some(data.to_string_lossy().into_owned());
    assert_eq!(seen("given", Some(&data), json!({})), (vec![ours.clone()], vec![ours.clone()]), "--data-dir");
    let app = bashcut_dirs::data_dir().map(|d| d.to_string_lossy().into_owned());
    assert!(app.is_some(), "the test process has an app data folder");
    assert_eq!(seen("app", None, json!({})), (vec![app.clone()], vec![app]), "no --data-dir: the app's folder");
    let own = json!({"BASHCUTPRO_DATA_DIR": "/step/own", "OTHER": "x"});
    assert_eq!(seen("own", Some(&data), own), (vec![Some("/step/own".to_string())], vec![ours]), "the step's own value wins");
}
