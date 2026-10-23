//! The agent session against a scripted fake agent (a Python script speaking Claude Code's
//! stream-json): a turn's events reach the transcript, the next turn resumes the session, gates are
//! answered and sent, a failing agent shows its error, Stop ends a hanging one, and conversations are
//! kept next to the project. Skipped when neither `python3` nor `python` runs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use bashcut_agent::{Agent, AgentSpec, Context, Item, Status, plan};

const FAKE: &str = r#"
import json, sys, time
args = sys.argv[1:]
resumed = args[args.index("--resume") + 1] if "--resume" in args else ""
msg = json.loads(sys.stdin.readline())
text = msg["message"]["content"][0]["text"]
def out(v):
    print(json.dumps(v), flush=True)
session = resumed or "sess-1"
out({"type": "system", "subtype": "init", "session_id": session, "model": "fake"})
if "please fail" in text:
    sys.stderr.write("fake agent: boom\n")
    sys.exit(3)
if "please hang" in text:
    out({"type": "assistant", "message": {"content": [{"type": "text", "text": "thinking"}]}})
    time.sleep(60)
out({"type": "assistant", "message": {"content": [
    {"type": "text", "text": ("resumed " + resumed + ": " if resumed else "") + "echo " + text},
    {"type": "tool_use", "id": "t1", "name": "mcp__filmcraft__command_execute", "input": {"command": "edit.razor"}}]}})
out({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}})
print("not json", flush=True)
out({"type": "result", "subtype": "success", "is_error": False, "result": "done", "session_id": session, "num_turns": 1})
"#;

/// `python3`, else `python` (Windows installs only the latter; its `python3` is a store stub that fails).
fn python() -> Option<String> {
    ["python3", "python"].into_iter().find(|p| Command::new(p).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)).map(str::to_string)
}

fn setup(name: &str) -> Option<(PathBuf, AgentSpec, Context)> {
    let py = python()?;
    let dir = std::env::temp_dir().join(format!("bashcut-fake-agent-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let script = dir.join("fake.py");
    std::fs::write(&script, FAKE).ok()?;
    let spec = AgentSpec {
        id: "fake".into(),
        name: "Fake".into(),
        argv: vec![py, script.to_string_lossy().into_owned()],
        resume_args: vec!["--resume".into(), "{session}".into()],
        timeout_secs: 30,
        ..AgentSpec::default()
    };
    let ctx = Context { project: Some(dir.join("film.fcproj")), line: "Project film, playhead 00:00:01".into(), cli: None, bridge: None };
    Some((dir, spec, ctx))
}

fn run(agent: &mut Agent) {
    let end = Instant::now() + Duration::from_secs(20);
    while agent.poll() {
        assert!(Instant::now() < end, "the fake agent took too long");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn sidecar(dir: &Path) -> PathBuf {
    bashcut_agent::sidecar(&dir.join("film.fcproj"))
}

#[test]
fn turns_resume_and_are_kept() {
    let Some((dir, spec, ctx)) = setup("turns") else { return };
    let mut agent = Agent::new(vec![spec]);
    assert!(agent.send("  ", &ctx).is_err());
    agent.send("cut the intro", &ctx).unwrap();
    assert_eq!(agent.status, Status::Running);
    assert!(agent.send("again", &ctx).is_err(), "one turn at a time");
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    assert_eq!(agent.conversation.session, "sess-1");
    let items = &agent.conversation.transcript.items;
    assert_eq!(items[0], Item::User { text: "cut the intro".into() });
    let answer = agent.conversation.transcript.last_answer().unwrap();
    assert!(answer.contains("[Project film, playhead 00:00:01]") && answer.contains("cut the intro"), "{answer}");
    assert!(items.iter().any(|i| matches!(i, Item::Tool { output: Some(o), .. } if o == "ok")));
    assert!(items.iter().any(|i| matches!(i, Item::Notice { message } if message == "not json")));

    agent.send("shorter", &ctx).unwrap();
    run(&mut agent);
    assert!(agent.conversation.transcript.last_answer().unwrap().starts_with("resumed sess-1:"));

    let listed = agent.conversations();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "cut the intro");
    // a new conversation starts fresh; opening the old one brings its session back
    let old = agent.conversation.id.clone();
    agent.command("new", &serde_json::json!({}), &ctx).unwrap();
    assert!(agent.conversation.transcript.items.is_empty());
    agent.command("open", &serde_json::json!({"id": old}), &ctx).unwrap();
    assert_eq!(agent.conversation.session, "sess-1");
    let state = agent.command("state", &serde_json::json!({}), &ctx).unwrap();
    assert_eq!(state["status"]["state"], "idle");
    assert!(agent.command("nope", &serde_json::json!({}), &ctx).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gates_are_answered_and_sent() {
    let Some((dir, spec, ctx)) = setup("gates") else { return };
    let side = sidecar(&dir);
    std::fs::create_dir_all(&side).unwrap();
    std::fs::write(plan::file(&side), r#"{"gates":[{"stage":"rough cut","summary":"9 clips","evidence":["cut.png"],"state":"pending"}]}"#).unwrap();
    let mut agent = Agent::new(vec![spec]);
    agent.set_project(ctx.project.as_deref());
    assert_eq!(agent.gate().unwrap().stage, "rough cut");
    assert!(agent.command("change", &serde_json::json!({"note": ""}), &ctx).is_err(), "a change needs a note");
    agent.command("approve", &serde_json::json!({}), &ctx).unwrap();
    run(&mut agent);
    assert!(agent.gate().is_none());
    assert!(agent.conversation.transcript.items.iter().any(|i| matches!(i, Item::Gate { state, .. } if state == "approved")));
    assert!(agent.conversation.transcript.last_answer().unwrap().contains("Gate `rough cut` approved"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failure_and_stop_end_the_turn() {
    let Some((dir, spec, ctx)) = setup("stop") else { return };
    let mut agent = Agent::new(vec![spec]);
    agent.send("please fail", &ctx).unwrap();
    run(&mut agent);
    assert!(matches!(&agent.status, Status::Failed { message } if message.contains("boom")), "{:?}", agent.status);

    agent.send("please hang", &ctx).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while !agent.conversation.transcript.items.iter().any(|i| matches!(i, Item::Assistant { text } if text == "thinking")) {
        assert!(Instant::now() < end, "no output before the hang");
        agent.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    agent.command("stop", &serde_json::json!({}), &ctx).unwrap();
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    assert!(matches!(agent.conversation.transcript.items.last(), Some(Item::Notice { message }) if message == "Stopped."));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn claude_code_needs_the_bridge() {
    let mut agent = Agent::new(Vec::new());
    assert_eq!(agent.agent.id, "claude-code");
    let ctx = Context::default();
    let e = agent.send("hi", &ctx).unwrap_err();
    assert!(e.0.contains("filmcraft-cli") || e.0.contains("--control"), "{e}");
    assert!(agent.conversation.transcript.items.is_empty(), "nothing is shown for a turn that never started");
}

/// A scripted Codex (`codex exec --json [resume <id>] -`): plain-text prompt on stdin, item events out.
const FAKE_CODEX: &str = r#"
import json, sys
args = sys.argv[1:]
assert args[-1] == "-", args
resumed = args[args.index("resume") + 1] if "resume" in args else ""
text = sys.stdin.read()
def out(v):
    print(json.dumps(v), flush=True)
thread = resumed or "th-1"
out({"type": "thread.started", "thread_id": thread})
out({"type": "turn.started"})
out({"type": "item.started", "item": {"id": "i1", "type": "mcp_tool_call", "server": "filmcraft", "tool": "command_execute",
     "arguments": {"command": "edit.razor"}, "status": "in_progress"}})
out({"type": "item.completed", "item": {"id": "i1", "type": "mcp_tool_call", "server": "filmcraft", "tool": "command_execute",
     "arguments": {"command": "edit.razor"}, "result": {"content": [{"type": "text", "text": "ok"}]}, "status": "completed"}})
if "please fail" in text:
    out({"type": "turn.failed", "error": {"message": "quota reached"}})
    sys.exit(1)
out({"type": "item.completed", "item": {"id": "i2", "type": "agent_message", "text": ("resumed " + resumed + ": " if resumed else "") + "echo " + text}})
out({"type": "turn.completed", "usage": {"input_tokens": 1, "output_tokens": 1}})
"#;

#[test]
fn codex_turns_resume_and_show_tool_calls() {
    let Some((dir, mut spec, ctx)) = setup("codex") else { return };
    let script = dir.join("codex.py");
    std::fs::write(&script, FAKE_CODEX).unwrap();
    let codex = AgentSpec::codex(None);
    spec.argv.truncate(1);
    spec.argv.push(script.to_string_lossy().into_owned());
    spec = AgentSpec { resume_args: codex.resume_args, prompt_args: codex.prompt_args, events: codex.events, ..spec };
    let mut agent = Agent::new(vec![spec]);
    agent.send("cut the intro", &ctx).unwrap();
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle, "{:?}", agent.conversation.transcript.items);
    assert_eq!(agent.conversation.session, "th-1");
    let items = &agent.conversation.transcript.items;
    assert!(items.iter().any(|i| matches!(i, Item::Tool { output: Some(o), is_error: false, .. } if o == "ok")));
    assert!(items.iter().any(|i| i.tool_key().as_deref() == Some("edit.razor")));
    assert!(agent.conversation.transcript.last_answer().unwrap().contains("cut the intro"));

    agent.send("shorter", &ctx).unwrap();
    run(&mut agent);
    assert!(agent.conversation.transcript.last_answer().unwrap().starts_with("resumed th-1:"));

    agent.send("please fail", &ctx).unwrap();
    run(&mut agent);
    assert!(agent.conversation.transcript.items.iter().any(|i| matches!(i, Item::Error { message } if message == "quota reached")));
    assert!(matches!(agent.status, Status::Failed { .. }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn codex_json_agent_plugin_runs_a_turn() {
    let Some((dir, spec, ctx)) = setup("plugin") else { return };
    let py = spec.argv[0].clone();
    let data = dir.join("data");
    let folder = bashcut_plugin_host::plugins_dir(&data).join("test.codex");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("codex.py"), FAKE_CODEX).unwrap();
    let manifest = serde_json::json!({
        "schema": "bashcut-plugin/1", "id": "test.codex", "name": "Fake Codex", "version": "0.1.0", "summary": "test agent",
        "kind": "agent",
        "entry": {"command": [py, folder.join("codex.py").to_string_lossy()]},
        "settings": [{"id": "model", "title": "Model", "type": "string", "default": "m1", "env": ["FAKE_MODEL"]}],
        "agent": {"events": "codex-json", "resume": ["resume", "{session}"], "prompt": ["-"], "timeoutSecs": 30}
    });
    std::fs::write(folder.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    // only saved values reach the env (a default the user never saved is the plugin's to apply)
    std::fs::write(folder.join("settings.json"), r#"{"model":"m2"}"#).unwrap();
    let host = bashcut_plugin_host::Host::new(Some(data), std::sync::Arc::new(bashcut_plugin_host::settings::MemoryStore::default()));
    let launches = host.agents();
    assert_eq!(launches.len(), 1, "{:?}", host.installed().iter().map(|p| &p.error).collect::<Vec<_>>());
    let spec = AgentSpec::from(launches[0].clone());
    assert_eq!((spec.id.as_str(), spec.name.as_str(), spec.events.as_str(), spec.timeout_secs), ("test.codex", "Fake Codex", "codex-json", 30));
    assert!(spec.env.iter().any(|(k, v)| k == "FAKE_MODEL" && v == "m2"), "{:?}", spec.env);

    let mut agent = Agent::new(vec![AgentSpec::claude_code(None), spec]);
    agent.use_agent("test.codex").unwrap();
    agent.send("cut the intro", &ctx).unwrap();
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle, "{:?}", agent.conversation.transcript.items);
    assert_eq!(agent.conversation.agent, "test.codex");
    assert_eq!(agent.conversation.session, "th-1");
    assert!(agent.conversation.transcript.items.iter().any(|i| matches!(i, Item::Tool { output: Some(o), .. } if o == "ok")));
    assert!(agent.conversation.transcript.last_answer().unwrap().contains("cut the intro"));
    let _ = std::fs::remove_dir_all(&dir);
}
