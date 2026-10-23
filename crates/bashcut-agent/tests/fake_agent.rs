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
    {"type": "tool_use", "id": "t1", "name": "mcp__bashcutpro__command_execute", "input": {"command": "edit.razor"}}]}})
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
    let ctx = Context { project: Some(dir.join("film.fcproj")), line: "Project film, playhead 00:00:01".into(), cli: None, bridge: None, control_token: None };
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

/// A scripted live Claude Code (`claude -p --input-format stream-json …`, AG2–AG4): it keeps reading
/// stdin, takes one message at a time (`system/init` with its capabilities, the `isReplay` echo with
/// the message's uuid, then the answer and a `result`), keeps messages read during a turn for later and
/// answers control requests (interrupt, `cancel_async_message`) while it works. A message containing
/// `slow` works 1.5 s, `hang` 60 s (an assistant line `working on …` and a tool call first); `please
/// sign in` answers like a signed-out Claude Code (`no result`: exits 1 before the result, `then exit`:
/// exits 1 after it); `please crash` exits 3 mid turn; any other `then exit` exits 0 after the answer.
/// `ask <n>` asks `can_use_tool` for `n` Bash calls (`perm-0` …, each suggesting the rule `Bash(rm:*)`)
/// and works until each is answered (at most 60 s); with `big` their input is over 256 KiB, with
/// `withdraw` it withdraws `perm-0` at once (`control_cancel_request`), with `end early` it answers
/// without waiting.
/// `FAKE_MODE` (comma separated): `receipt` / `cancel-queued` announce `interrupt_receipt_v1` /
/// `interrupt_cancel_queued_v1`; `refuse` answers the interrupt with an error, `silent` never answers
/// it, `ack-only` acknowledges it and keeps working; `keep` answers `cancel_async_message` with
/// `cancelled: false` and keeps the message; `outdated` refuses a flag like an old Claude Code.
/// A `FAKE_EARLY` file that exists is removed and the process exits 0 before reading anything. Every
/// start, line read, control request, control response (`answer`), taken message, SIGINT and EOF is a
/// JSON line in `FAKE_LOG` ([`LiveFake::notes`]).
const LIVE_FAKE: &str = r#"
import json, os, queue, re, signal, sys, threading, time
args = sys.argv[1:]
opts = set(os.environ.get("FAKE_MODE", "").split(","))
resumed = args[args.index("--resume") + 1] if "--resume" in args else ""
session = resumed or "live-1"
log = open(os.environ["FAKE_LOG"], "a")
def note(kind, **kw):
    log.write(json.dumps(dict(pid=os.getpid(), kind=kind, **kw)) + "\n")
    log.flush()
def out(v):
    sys.stdout.write(json.dumps(v) + "\n")
    sys.stdout.flush()
def on_int(*_):
    note("sigint")
    os._exit(130)
signal.signal(signal.SIGINT, on_int)
note("start", args=args, cwd=os.getcwd(), inApp=os.environ.get("BASHCUT_IN_APP_AGENT", ""),
     sidecar=os.environ.get("BASHCUT_PROJECT_SIDECAR", ""))
early = os.environ.get("FAKE_EARLY", "")
if early and os.path.exists(early):
    os.remove(early)
    note("early")
    sys.exit(0)
if "outdated" in opts:
    sys.stderr.write("error: unknown option '--replay-user-messages'\n")
    sys.exit(1)
caps = [c for o, c in (("receipt", "interrupt_receipt_v1"), ("cancel-queued", "interrupt_cancel_queued_v1")) if o in opts]
lines = queue.Queue()
def read():
    for line in sys.stdin:
        try:
            lines.put(json.loads(line))
        except ValueError:
            note("bad", line=line)
    lines.put(None)
threading.Thread(target=read, daemon=True).start()
waiting = []
answered = set()
eof = False
def text_of(m):
    return m["message"]["content"][0]["text"]
def control(m):
    r, rid = m.get("request", {}), m.get("request_id", "")
    note("control", request=r)
    def answer(subtype, **kw):
        out({"type": "control_response", "response": dict(subtype=subtype, request_id=rid, **kw)})
    if r.get("subtype") == "interrupt":
        if "refuse" in opts:
            return answer("error", error="interrupt refused")
        if "silent" in opts:
            return None
        answer("success")
        if "ack-only" in opts:
            return None
        if r.get("cancel_queued"):
            note("dropped", n=len(waiting))
            waiting.clear()
        return "stop"
    if r.get("subtype") == "cancel_async_message":
        hit = [w for w in waiting if w.get("uuid") and w.get("uuid") == r.get("message_uuid") and "keep" not in opts]
        for w in hit:
            waiting.remove(w)
        answer("success", response={"cancelled": bool(hit)})
    return None
def pump(timeout):
    global eof
    try:
        m = lines.get(timeout=timeout)
    except queue.Empty:
        return None
    if m is None:
        eof = True
        return None
    if m.get("type") == "control_request":
        return control(m)
    if m.get("type") == "control_response":
        note("answer", response=m.get("response"))
        answered.add(m.get("response", {}).get("request_id", ""))
        return None
    if m.get("type") == "user":
        note("read", text=text_of(m), uuid=m.get("uuid", ""))
        waiting.append(m)
    return None
def ask(text, n):
    for i in range(n):
        tool, inp = ("Write", {"file_path": "/p/big.txt", "content": "x" * 300000}) if "big" in text else ("Bash", {"command": "rm take%d.mov" % i})
        out({"type": "control_request", "request_id": "perm-%d" % i, "request": {"subtype": "can_use_tool", "tool_name": tool,
             "input": inp, "tool_use_id": "tu%d" % i, "permission_suggestions": [{"type": "addRules",
             "rules": [{"toolName": "Bash", "ruleContent": "rm:*"}], "behavior": "allow", "destination": "localSettings"}]}})
    open_ = {"perm-%d" % i for i in range(n)}
    answered.difference_update(open_)
    if "withdraw" in text:
        out({"type": "control_cancel_request", "request_id": "perm-0"})
        open_.discard("perm-0")
    if "end early" in text:
        return True
    end = time.time() + 60
    while open_ - answered and time.time() < end:
        if pump(0.05) == "stop":
            return False
    return True
def turn(m):
    text = text_of(m)
    note("take", text=text, uuid=m.get("uuid", ""))
    out({"type": "system", "subtype": "init", "session_id": session, "model": "fake", "capabilities": caps})
    out({"type": "user", "isReplay": True, "uuid": m.get("uuid", ""), "message": m["message"]})
    asks = re.search(r"ask (\d+)", text)
    if asks and not ask(text, int(asks.group(1))):
        out({"type": "result", "subtype": "error_during_execution", "is_error": True, "session_id": session})
        return
    if "please crash" in text:
        sys.stderr.write("fake agent: boom\n")
        sys.exit(3)
    if "please sign in" in text:
        line = "Not logged in · Please run /login"
        out({"type": "assistant", "error": "authentication_failed", "message": {"content": [{"type": "text", "text": line}]}})
        if "no result" in text:
            sys.exit(1)
        out({"type": "result", "subtype": "success", "is_error": True, "result": line, "session_id": session})
        if "then exit" in text:
            sys.exit(1)
        return
    secs = 60 if "hang" in text else 1.5 if "slow" in text else 0
    if secs:
        out({"type": "assistant", "message": {"content": [{"type": "text", "text": "working on " + text},
             {"type": "tool_use", "id": "t1", "name": "mcp__bashcutpro__command_execute", "input": {"command": "edit.razor"}}]}})
        end = time.time() + secs
        while time.time() < end:
            if pump(0.05) == "stop":
                out({"type": "result", "subtype": "error_during_execution", "is_error": True, "session_id": session})
                return
        out({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}})
    out({"type": "assistant", "message": {"content": [{"type": "text", "text": ("resumed " + resumed + ": " if resumed else "") + "echo " + text}]}})
    out({"type": "result", "subtype": "success", "is_error": False, "result": "done", "session_id": session, "num_turns": 1})
    if "then exit" in text:
        sys.exit(0)
while True:
    if waiting:
        turn(waiting.pop(0))
    elif eof:
        note("eof")
        sys.exit(0)
    else:
        pump(0.2)
"#;

/// A [`LIVE_FAKE`] agent for one test (TA3/TA5 build on it): its folder, spec, context and log.
struct LiveFake {
    dir: PathBuf,
    spec: AgentSpec,
    ctx: Context,
    log: PathBuf,
}

impl LiveFake {
    /// The fake in `FAKE_MODE` `mode` (see [`LIVE_FAKE`]); `None` without Python.
    fn new(name: &str, mode: &str) -> Option<LiveFake> {
        let (dir, mut spec, ctx) = setup(&format!("live-{name}"))?;
        let script = dir.join("live.py");
        std::fs::write(&script, LIVE_FAKE).ok()?;
        let log = dir.join("fake.log");
        spec.argv.truncate(1);
        spec.argv.push(script.to_string_lossy().into_owned());
        spec.id = "fake-live".into();
        spec.env = vec![("FAKE_MODE".into(), mode.into()), ("FAKE_LOG".into(), log.to_string_lossy().into_owned())];
        Some(LiveFake { dir, spec, ctx, log })
    }

    /// An agent with only this fake.
    fn agent(&self) -> Agent {
        Agent::new(vec![self.spec.clone()])
    }

    /// The log lines of `kind` (`start`, `read`, `take`, `control`, `answer`, `dropped`, `sigint`, `eof`, `early`), oldest first.
    fn notes(&self, kind: &str) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        text.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).filter(|v| v["kind"] == kind).collect()
    }

    /// The pid of every process started, oldest first.
    fn pids(&self) -> Vec<u64> {
        self.notes("start").iter().filter_map(|n| n["pid"].as_u64()).collect()
    }

    /// The arguments the last process was started with.
    fn last_args(&self) -> Vec<String> {
        let starts = self.notes("start");
        let args = starts.last().and_then(|n| n["args"].as_array().cloned()).unwrap_or_default();
        args.iter().filter_map(|a| a.as_str().map(str::to_string)).collect()
    }
}

impl Drop for LiveFake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Poll `agent` until `done` (at most 15 s).
fn until(agent: &mut Agent, what: &str, mut done: impl FnMut(&Agent) -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while !done(agent) {
        assert!(Instant::now() < end, "waited too long for {what}: {:?}", agent.conversation.transcript.items);
        agent.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Poll until the fake says it works on the message containing `text` (its tool call is shown).
fn working(agent: &mut Agent, text: &str) {
    until(agent, "the turn to start", |a| {
        a.conversation.transcript.items.iter().any(|i| matches!(i, Item::Assistant { text: t } if t.starts_with("working on") && t.contains(text)))
    });
}

fn answers(agent: &Agent) -> Vec<String> {
    let items = &agent.conversation.transcript.items;
    items.iter().filter_map(|i| if let Item::Assistant { text } = i { Some(text.clone()) } else { None }).filter(|t| t.contains("echo ")).collect()
}

fn users(agent: &Agent) -> Vec<String> {
    agent.conversation.transcript.items.iter().filter_map(|i| if let Item::User { text } = i { Some(text.clone()) } else { None }).collect()
}

fn no_error(agent: &Agent) {
    let items = &agent.conversation.transcript.items;
    assert!(!items.iter().any(|i| matches!(i, Item::Error { .. })), "{items:?}");
}

fn stopped(agent: &Agent) -> bool {
    matches!(agent.conversation.transcript.items.last(), Some(Item::Notice { message }) if message == "Stopped.")
}

fn kill(pid: u64) {
    let pid = pid.to_string();
    #[cfg(unix)]
    let status = Command::new("kill").args(["-KILL", &pid]).status();
    #[cfg(not(unix))]
    let status = Command::new("taskkill").args(["/F", "/PID", &pid]).status();
    assert!(status.is_ok_and(|s| s.success()), "cannot kill {pid}");
}

#[test]
fn live_agent_keeps_one_process_for_three_messages() {
    let Some(f) = LiveFake::new("three", "cancel-queued") else { return };
    let mut agent = f.agent();
    assert!(agent.is_live());
    agent.send("slow first", &f.ctx).unwrap();
    working(&mut agent, "slow first");
    agent.send("second", &f.ctx).unwrap();
    agent.send("third", &f.ctx).unwrap();
    // written at once (with their uuids), shown queued until the agent echoes them
    assert_eq!(agent.queued(), ["second", "third"]);
    until(&mut agent, "the agent to read both", |_| f.notes("read").len() == 3);
    assert!(f.notes("read").iter().skip(1).all(|n| n["uuid"].as_str().is_some_and(|u| u.len() == 36)));
    assert_eq!(agent.queued(), ["second", "third"], "nothing taken while the first turn works");
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    assert!(agent.queued().is_empty());
    assert_eq!(f.pids().len(), 1, "one process answers all three");
    let users = users(&agent);
    assert_eq!(users, ["slow first", "second", "third"]);
    let answers = answers(&agent);
    assert_eq!(answers.len(), 3, "{answers:?}");
    for (a, u) in answers.iter().zip(&users) {
        assert!(a.ends_with(u.as_str()), "{a} answers {u}");
    }
    assert_eq!(agent.state_json()["live"], true, "the process is kept for the next message");
    no_error(&agent);
    agent.shutdown();
}

#[test]
fn live_agent_restarts_with_resume_after_kill() {
    let Some(f) = LiveFake::new("restart", "") else { return };
    let mut agent = f.agent();
    agent.send("first", &f.ctx).unwrap();
    run(&mut agent);
    assert_eq!(agent.conversation.session, "live-1");
    assert!(!f.last_args().contains(&"--resume".to_string()));

    kill(f.pids()[0]);
    until(&mut agent, "the killed agent to end", |a| a.state_json()["live"] == false);
    assert_eq!(agent.status, Status::Idle, "an idle agent that ends is no failure");
    agent.send("second", &f.ctx).unwrap();
    run(&mut agent);
    assert_eq!(f.pids().len(), 2);
    assert!(f.last_args().windows(2).any(|w| w == ["--resume", "live-1"]), "{:?}", f.last_args());
    assert!(answers(&agent).last().unwrap().starts_with("resumed live-1: "));

    // the next process exits before reading: started again once, resuming, and the message written again
    kill(f.pids()[1]);
    until(&mut agent, "the killed agent to end", |a| a.state_json()["live"] == false);
    let early = f.dir.join("early");
    std::fs::write(&early, "").unwrap();
    let spec = AgentSpec { env: [f.spec.env.clone(), vec![("FAKE_EARLY".into(), early.to_string_lossy().into_owned())]].concat(), ..f.spec.clone() };
    agent.agents = vec![spec.clone()];
    agent.agent = spec;
    agent.send("third", &f.ctx).unwrap();
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle, "{:?}", agent.conversation.transcript.items);
    assert_eq!(f.notes("early").len(), 1);
    assert_eq!(f.pids().len(), 4);
    assert!(f.last_args().windows(2).any(|w| w == ["--resume", "live-1"]));
    assert_eq!(f.notes("take").last().and_then(|n| n["text"].as_str()).map(|t| t.ends_with("third")), Some(true));
    assert_eq!(users(&agent), ["first", "second", "third"], "the message shows once");
    assert!(answers(&agent).last().unwrap().ends_with("third"));
    assert!(agent.take_unsent().is_empty());
    no_error(&agent);
    agent.shutdown();
}

#[test]
fn outdated_claude_code_is_reported() {
    let Some(f) = LiveFake::new("outdated", "outdated") else { return };
    // only Claude Code's refusal of a flag means "update Claude Code"
    for (id, outdated) in [("fake-live", false), ("claude-code", true)] {
        let mut agent = Agent::new(vec![AgentSpec { id: id.into(), ..f.spec.clone() }]);
        agent.send("hi", &f.ctx).unwrap();
        run(&mut agent);
        assert!(matches!(&agent.status, Status::Failed { message } if message.contains("unknown option")), "{:?}", agent.status);
        assert_eq!(agent.outdated(), outdated, "{id}");
        assert!(agent.conversation.transcript.items.iter().any(|i| matches!(i, Item::Error { message } if message.contains("unknown option"))));
    }
    assert_eq!(f.pids().len(), 2, "a refused flag is not retried");
}

#[test]
fn live_agent_stop_ends_the_turn_and_gives_back_queued_text() {
    let Some(f) = LiveFake::new("stop", "") else { return };
    let mut agent = f.agent();
    agent.send("hang a", &f.ctx).unwrap();
    working(&mut agent, "hang a");
    agent.send("queued b", &f.ctx).unwrap();
    assert_eq!(agent.queued(), ["queued b"]);
    agent.command("stop", &serde_json::json!({}), &f.ctx).unwrap();
    assert_eq!(agent.status, Status::Stopping);
    assert!(agent.send("more", &f.ctx).is_err(), "no message while it stops");
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    assert!(stopped(&agent), "{:?}", agent.conversation.transcript.items);
    assert_eq!(agent.take_unsent(), ["queued b"], "the queued text goes back to the message box");
    assert!(agent.queued().is_empty());
    #[cfg(unix)]
    assert_eq!(f.notes("sigint").len(), 1);
    assert!(f.notes("control").is_empty(), "no interrupt request without interrupt_receipt_v1");
    assert!(!f.notes("take").iter().any(|n| n["text"].as_str().is_some_and(|t| t.contains("queued b"))));
    no_error(&agent);

    agent.send("after", &f.ctx).unwrap();
    run(&mut agent);
    assert_eq!(f.pids().len(), 2);
    assert!(f.last_args().windows(2).any(|w| w == ["--resume", "live-1"]));
    assert!(answers(&agent).last().unwrap().starts_with("resumed live-1: "));
    agent.shutdown();
}

#[test]
fn live_agent_stop_keeps_the_process() {
    let Some(f) = LiveFake::new("interrupt", "receipt,cancel-queued") else { return };
    let mut agent = f.agent();
    agent.send("hang a", &f.ctx).unwrap();
    working(&mut agent, "hang a");
    assert!(agent.capabilities.iter().any(|c| c == "interrupt_receipt_v1"));
    assert!(agent.conversation.transcript.items.iter().any(|i| matches!(i, Item::Tool { output: None, .. })), "stopped mid tool call");
    agent.send("queued b", &f.ctx).unwrap();
    agent.send("queued c", &f.ctx).unwrap();
    until(&mut agent, "the agent to read both", |_| f.notes("read").len() == 3);
    agent.stop();
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    assert!(stopped(&agent), "{:?}", agent.conversation.transcript.items);
    let control = f.notes("control");
    assert_eq!(control.len(), 1);
    assert_eq!(control[0]["request"], serde_json::json!({"subtype": "interrupt", "cancel_queued": true}));
    assert_eq!(f.notes("dropped")[0]["n"], 2);
    assert_eq!(agent.take_unsent(), ["queued b", "queued c"], "the queued text goes back to the message box");
    assert!(f.notes("sigint").is_empty());
    assert_eq!(agent.state_json()["live"], true);
    no_error(&agent);

    agent.send("next", &f.ctx).unwrap();
    run(&mut agent);
    assert_eq!(f.pids().len(), 1, "the same process answers the next message");
    assert!(answers(&agent).last().unwrap().ends_with("next"));
    let taken: Vec<_> = f.notes("take").iter().filter_map(|n| n["text"].as_str().map(str::to_string)).collect();
    assert!(!taken.iter().any(|t| t.contains("queued")), "{taken:?}");
    agent.shutdown();
}

/// A live fake in `mode` stopped mid turn (with one message queued behind it when `queue`): the agent
/// once the stop is done, with how long it took.
fn stop_mid_turn(name: &str, mode: &str, queue: bool) -> Option<(LiveFake, Agent, Duration)> {
    let f = LiveFake::new(name, mode)?;
    let mut agent = f.agent();
    agent.send("hang a", &f.ctx).unwrap();
    working(&mut agent, "hang a");
    if queue {
        agent.send("queued b", &f.ctx).unwrap();
    }
    let t = Instant::now();
    agent.stop();
    run(&mut agent);
    let took = t.elapsed();
    assert_eq!(agent.status, Status::Idle, "{mode}");
    assert!(stopped(&agent), "{mode}: {:?}", agent.conversation.transcript.items);
    no_error(&agent);
    Some((f, agent, took))
}

/// SIGINT ended the process: the next message resumes the session in a new one.
fn ended_and_resumes(f: &LiveFake, agent: &mut Agent) {
    assert_eq!(agent.state_json()["live"], false);
    #[cfg(unix)]
    assert_eq!(f.notes("sigint").len(), 1);
    agent.send("after", &f.ctx).unwrap();
    run(agent);
    assert_eq!(f.pids().len(), 2);
    assert!(f.last_args().windows(2).any(|w| w == ["--resume", "live-1"]));
    assert!(answers(agent).last().unwrap().starts_with("resumed live-1: "));
    agent.shutdown();
}

#[test]
fn live_agent_stop_falls_back_to_sigint() {
    // no interrupt_receipt_v1: SIGINT at once
    let Some((f, mut agent, _)) = stop_mid_turn("no-receipt", "", false) else { return };
    assert!(f.notes("control").is_empty());
    ended_and_resumes(&f, &mut agent);

    // the request is refused
    let Some((f, mut agent, _)) = stop_mid_turn("refused", "receipt,refuse", false) else { return };
    assert_eq!(f.notes("control").len(), 1);
    ended_and_resumes(&f, &mut agent);

    // no answer within 3 s
    let Some((f, mut agent, took)) = stop_mid_turn("silent", "receipt,silent", false) else { return };
    assert_eq!(f.notes("control").len(), 1);
    assert!(took >= Duration::from_secs(3), "{took:?}");
    ended_and_resumes(&f, &mut agent);

    // acknowledged, but no result within 5 s
    let Some((f, mut agent, took)) = stop_mid_turn("no-result", "receipt,ack-only", false) else { return };
    assert_eq!(f.notes("control").len(), 1);
    assert!(took >= Duration::from_secs(5), "{took:?}");
    ended_and_resumes(&f, &mut agent);

    // without interrupt_cancel_queued_v1 a queued message is held in the app (AG4), so the request
    // (without cancel_queued) still works: the process stays and the held message goes back
    let Some((f, mut agent, _)) = stop_mid_turn("held", "receipt", true) else { return };
    let control = f.notes("control");
    assert_eq!(control.len(), 1);
    assert_eq!(control[0]["request"], serde_json::json!({"subtype": "interrupt"}));
    assert_eq!(agent.take_unsent(), ["queued b"]);
    assert!(f.notes("sigint").is_empty());
    agent.send("after", &f.ctx).unwrap();
    run(&mut agent);
    assert_eq!(f.pids().len(), 1);
    agent.shutdown();
}

#[test]
fn release_waits_for_the_live_agent_to_exit() {
    let Some(f) = LiveFake::new("release", "receipt") else { return };
    let mut agent = f.agent();
    assert!(agent.release(), "nothing runs: released at once");
    agent.send("hang a", &f.ctx).unwrap();
    working(&mut agent, "hang a");
    assert!(!agent.release(), "a running turn is stopped first");
    assert_eq!(agent.status, Status::Stopping);
    let end = Instant::now() + Duration::from_secs(15);
    while !agent.release() {
        assert!(Instant::now() < end, "the agent did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(agent.status, Status::Idle);
    assert!(stopped(&agent));
    assert_eq!(agent.state_json()["live"], false);
    assert_eq!(f.notes("eof").len(), 1, "the process exited (stdin closed) before release said so");
    #[cfg(unix)]
    {
        let pid = f.pids()[0].to_string();
        let alive = Command::new("kill").args(["-0", &pid]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
        assert!(!alive, "process {pid} still runs");
    }

    agent.continued_in_terminal();
    assert!(matches!(agent.conversation.transcript.items.last(), Some(Item::Notice { message }) if message == "Continued in Terminal."));
    // a conversation without a session has nothing to continue
    agent.new_conversation().unwrap();
    agent.continued_in_terminal();
    assert!(agent.conversation.transcript.items.is_empty());
    agent.shutdown();
}

#[test]
fn auth_failure_closes_the_live_agent_without_a_generic_error() {
    let Some(f) = LiveFake::new("auth", "") else { return };
    let mut agent = f.agent();
    agent.send("please sign in then exit", &f.ctx).unwrap();
    run(&mut agent);
    assert!(agent.conversation.transcript.items.iter().any(|i| matches!(i, Item::Auth { category, .. } if category == "authentication_failed")));
    assert_eq!(agent.state_json()["live"], false, "closed at its result");
    // its exit (status 1) adds nothing
    std::thread::sleep(Duration::from_millis(300));
    agent.poll();
    no_error(&agent);

    agent.send("hello", &f.ctx).unwrap();
    run(&mut agent);
    assert_eq!(f.pids().len(), 2, "the next message starts a new process");
    assert!(answers(&agent).last().unwrap().ends_with("hello"));

    // the agent exits on the sign-in problem before its result
    agent.send("please sign in no result", &f.ctx).unwrap();
    run(&mut agent);
    assert!(matches!(agent.status, Status::Failed { .. }), "{:?}", agent.status);
    no_error(&agent);
    agent.send("hello again", &f.ctx).unwrap();
    run(&mut agent);
    assert_eq!(f.pids().len(), 3);

    // any other exit mid turn is an error (it once left the panel Running with nothing shown)
    agent.send("please crash", &f.ctx).unwrap();
    run(&mut agent);
    assert!(matches!(&agent.status, Status::Failed { message } if message.contains("boom")), "{:?}", agent.status);
    assert!(agent.conversation.transcript.items.iter().any(|i| matches!(i, Item::Error { message } if message.contains("boom"))));
    agent.shutdown();
}

#[test]
fn unsaved_project_runs_in_the_data_dir_agent_folder() {
    const CHILD: &str = "BASHCUT_TEST_UNSAVED_CHILD";
    if python().is_none() {
        return;
    }
    let Some(data) = std::env::var_os("BASHCUTPRO_DATA_DIR").filter(|_| std::env::var_os(CHILD).is_some()).map(PathBuf::from) else {
        // the data folder comes from the environment: run this test again in a child with its own
        let data = std::env::temp_dir().join(format!("bashcut-fake-agent-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let exe = std::env::current_exe().unwrap();
        let out = Command::new(exe)
            .args(["--exact", "unsaved_project_runs_in_the_data_dir_agent_folder", "--nocapture", "--test-threads", "1"])
            .env(CHILD, "1")
            .env("BASHCUTPRO_DATA_DIR", &data)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&data);
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && text.contains("1 passed"), "{text}");
        return;
    };
    let f = LiveFake::new("unsaved", "").unwrap();
    let ctx = Context { project: None, ..f.ctx.clone() };
    assert_eq!(bashcut_agent::session::turn_dir(None), data.join("agent"));
    // a bare file name has the parent "": it once gave an empty cwd and a misleading spawn error
    assert_eq!(bashcut_agent::session::turn_dir(Some(Path::new("film.fcproj"))), data.join("agent"));
    let mut agent = f.agent();
    agent.send("where am I", &ctx).unwrap();
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    let start = &f.notes("start")[0];
    let cwd = PathBuf::from(start["cwd"].as_str().unwrap());
    assert_eq!(cwd.canonicalize().unwrap(), data.join("agent").canonicalize().unwrap(), "never the app's or the test's folder");
    assert_eq!(start["inApp"], "1", "the kit's MCP launcher steps aside");
    assert_eq!(start["sidecar"], "", "an unsaved project has no sidecar");
    agent.shutdown();
}

/// The text of each `kind` note in the fake's log (`read`, `take`), oldest first.
fn texts(f: &LiveFake, kind: &str) -> Vec<String> {
    f.notes(kind).iter().filter_map(|n| n["text"].as_str().map(str::to_string)).collect()
}

/// Send `slow zero` to a fake in `mode`, then `one`, `two`, `three` while it works.
fn three_queued(name: &str, mode: &str) -> Option<(LiveFake, Agent)> {
    let f = LiveFake::new(name, mode)?;
    let mut agent = f.agent();
    agent.send("slow zero", &f.ctx).unwrap();
    working(&mut agent, "slow zero");
    for t in ["one", "two", "three"] {
        agent.send(t, &f.ctx).unwrap();
    }
    assert_eq!(agent.queued(), ["one", "two", "three"]);
    Some((f, agent))
}

#[test]
fn a_queued_message_is_withdrawn_with_cancel_async_message() {
    let Some((f, mut agent)) = three_queued("withdraw", "cancel-queued") else { return };
    agent.cancel(1).unwrap();
    assert_eq!(agent.queued_items()[1], bashcut_agent::session::Queued { text: "two".into(), cancelling: true }, "Removing… until the answer");
    assert!(agent.cancel(1).is_ok(), "a second ✕ while it is being removed changes nothing");
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    let reads = f.notes("read");
    let two = reads.iter().find(|n| n["text"].as_str().is_some_and(|t| t.ends_with("two"))).and_then(|n| n["uuid"].as_str()).unwrap();
    let control = f.notes("control");
    assert_eq!(control.len(), 1, "{control:?}");
    assert_eq!(control[0]["request"], serde_json::json!({"subtype": "cancel_async_message", "message_uuid": two}));
    assert_eq!(users(&agent), ["slow zero", "one", "three"]);
    let got = answers(&agent);
    assert_eq!(got.len(), 3, "{got:?}");
    assert!(got[1].ends_with("one") && got[2].ends_with("three"), "{got:?}");
    assert!(!texts(&f, "take").iter().any(|t| t.ends_with("two")), "the withdrawn message never runs");
    assert!(agent.take_unsent().is_empty() && agent.queued().is_empty());
    assert_eq!(f.pids().len(), 1);
    no_error(&agent);
    agent.shutdown();

    // `cancelled: false` (taken already): it stays queued until its echo and is answered
    let Some((f, mut agent)) = three_queued("withdraw-late", "cancel-queued,keep") else { return };
    agent.cancel(1).unwrap();
    until(&mut agent, "the agent to answer the cancel", |a| a.queued_items().iter().all(|q| !q.cancelling));
    assert_eq!(agent.queued(), ["one", "two", "three"], "kept until the agent takes it");
    run(&mut agent);
    assert_eq!(users(&agent), ["slow zero", "one", "two", "three"]);
    assert_eq!(answers(&agent).len(), 4);
    assert_eq!(f.notes("control").len(), 1);
    no_error(&agent);
    agent.shutdown();
}

#[test]
fn without_cancel_queued_messages_are_held_in_the_app() {
    let Some((f, mut agent)) = three_queued("held", "") else { return };
    // held in the app: the agent reads nothing while the first turn works
    std::thread::sleep(Duration::from_millis(300));
    agent.poll();
    assert_eq!(texts(&f, "read").len(), 1, "{:?}", texts(&f, "read"));
    agent.cancel(1).unwrap();
    assert_eq!(agent.queued(), ["one", "three"], "a held message is dropped at once");
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    assert!(f.notes("control").is_empty(), "nothing to withdraw from the agent");
    assert_eq!(users(&agent), ["slow zero", "one", "three"]);
    let got = answers(&agent);
    assert_eq!(got.len(), 3, "{got:?}");
    assert!(got[1].ends_with("one") && got[2].ends_with("three"), "{got:?}");
    // one per turn, in order: each message is written only after the turn before it ended
    let log = std::fs::read_to_string(&f.log).unwrap();
    let steps: Vec<String> = log
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "read" || v["kind"] == "take")
        .map(|v| format!("{} {}", v["kind"].as_str().unwrap_or_default(), v["text"].as_str().unwrap_or_default().rsplit("\n\n").next().unwrap_or_default()))
        .collect();
    assert_eq!(steps, ["read slow zero", "take slow zero", "read one", "take one", "read three", "take three"]);
    assert!(f.notes("read").iter().all(|n| n["uuid"] == ""), "no uuid without interrupt_cancel_queued_v1");
    assert_eq!(f.pids().len(), 1);
    no_error(&agent);
    agent.shutdown();

    // Stop gives the held ones back to the message box; the agent never saw them
    let Some(f) = LiveFake::new("held-stop", "") else { return };
    let mut agent = f.agent();
    agent.send("hang a", &f.ctx).unwrap();
    working(&mut agent, "hang a");
    agent.send("queued b", &f.ctx).unwrap();
    agent.send("queued c", &f.ctx).unwrap();
    agent.stop();
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    assert!(stopped(&agent), "{:?}", agent.conversation.transcript.items);
    assert_eq!(agent.take_unsent(), ["queued b", "queued c"]);
    assert!(!texts(&f, "read").iter().any(|t| t.contains("queued")));
    no_error(&agent);
    agent.shutdown();
}

/// The fake's `answer` notes: (request id, the answer's `response`), oldest first.
fn permission_answers(f: &LiveFake) -> Vec<(String, serde_json::Value)> {
    f.notes("answer").iter().map(|n| (n["response"]["request_id"].as_str().unwrap_or_default().to_string(), n["response"]["response"].clone())).collect()
}

/// The answers of the transcript's permission items, in order.
fn permission_items(agent: &Agent) -> Vec<String> {
    agent.conversation.transcript.items.iter().filter_map(|i| if let Item::Permission { answer, .. } = i { Some(answer.clone()) } else { None }).collect()
}

fn asking(agent: &mut Agent, n: usize) {
    until(agent, "the permission requests", |a| a.permissions().len() == n);
}

#[test]
fn permission_requests_are_answered_once_always_or_deny() {
    let Some(f) = LiveFake::new("permissions", "") else { return };
    let mut agent = f.agent();
    agent.send("ask 3", &f.ctx).unwrap();
    asking(&mut agent, 3);
    let waiting = agent.permissions();
    assert_eq!((waiting[0].tool.as_str(), waiting[0].detail.as_str()), ("Bash", "rm take0.mov"));
    assert!(waiting.iter().all(|p| p.always), "a saved project with a suggested rule offers Always");
    assert_eq!(agent.state_json()["permissions"].as_array().map(Vec::len), Some(3));
    assert!(agent.answer_permission(3, bashcut_agent::stream_json::Answer::Once).is_err());

    agent.answer_permission(0, bashcut_agent::stream_json::Answer::Once).unwrap();
    agent.answer_permission(0, bashcut_agent::stream_json::Answer::Always).unwrap();
    agent.command("permission", &serde_json::json!({"answer": "deny", "index": 0}), &f.ctx).unwrap();
    assert!(agent.permissions().is_empty());
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    assert!(answers(&agent).last().unwrap().ends_with("ask 3"), "the turn goes on once all are answered");

    let got = permission_answers(&f);
    let ids: Vec<&str> = got.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["perm-0", "perm-1", "perm-2"]);
    assert_eq!(got[0].1, serde_json::json!({"behavior": "allow", "updatedInput": {"command": "rm take0.mov"}}));
    assert_eq!(
        got[1].1,
        serde_json::json!({"behavior": "allow", "updatedInput": {"command": "rm take1.mov"},
            "updatedPermissions": [{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "rm:*"}], "behavior": "allow", "destination": "localSettings"}]})
    );
    assert_eq!(got[2].1, serde_json::json!({"behavior": "deny", "message": "The user denied this in BashCutPro."}));
    assert_eq!(permission_items(&agent), ["allowed", "always", "denied"]);
    no_error(&agent);
    agent.shutdown();
}

#[test]
fn stop_denies_waiting_permission_requests() {
    let Some(f) = LiveFake::new("perm-stop", "receipt") else { return };
    let mut agent = f.agent();
    // over 16 waiting: the rest are denied at once
    agent.send("ask 18", &f.ctx).unwrap();
    asking(&mut agent, 16);
    until(&mut agent, "the extra requests to be denied", |_| f.notes("answer").len() == 2);
    let got = permission_answers(&f);
    assert_eq!(got.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["perm-16", "perm-17"]);
    assert!(got.iter().all(|(_, r)| r["behavior"] == "deny" && r["message"] == "BashCutPro could not show this request."), "{got:?}");
    assert_eq!(agent.permissions().len(), 16);

    agent.stop();
    assert!(agent.permissions().is_empty(), "Stop answers them at once");
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    assert!(stopped(&agent), "{:?}", agent.conversation.transcript.items);
    let got = permission_answers(&f);
    assert_eq!(got.len(), 18);
    assert!(got[2..].iter().all(|(_, r)| r["behavior"] == "deny" && r["message"] == "The user stopped the turn in BashCutPro."), "{got:?}");
    assert_eq!(permission_items(&agent), vec!["denied"; 18]);
    assert_eq!(f.notes("control").len(), 1, "then the interrupt request");
    assert_eq!(agent.state_json()["live"], true, "stopped in place");
    no_error(&agent);
    agent.shutdown();
}

#[test]
fn permission_requests_nobody_can_answer_are_withdrawn() {
    let Some(f) = LiveFake::new("perm-withdraw", "") else { return };
    let mut agent = f.agent();
    // the agent withdraws one (control_cancel_request)
    agent.send("ask 2 withdraw", &f.ctx).unwrap();
    until(&mut agent, "the withdrawal", |a| a.permissions().len() == 1 && permission_items(a) == ["withdrawn"]);
    assert_eq!(agent.permissions()[0].detail, "rm take1.mov");
    agent.answer_permission(0, bashcut_agent::stream_json::Answer::Once).unwrap();
    run(&mut agent);
    assert_eq!(permission_answers(&f).iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["perm-1"]);

    // the turn's result: nothing waits on an answer any more
    agent.send("ask 2 end early", &f.ctx).unwrap();
    run(&mut agent);
    assert!(agent.permissions().is_empty());
    assert_eq!(permission_items(&agent), ["withdrawn", "allowed", "withdrawn", "withdrawn"]);

    // an input over 256 KiB cannot be shown: denied at once
    agent.send("ask 1 big", &f.ctx).unwrap();
    run(&mut agent);
    assert_eq!(agent.status, Status::Idle);
    let (id, r) = permission_answers(&f).pop().unwrap();
    assert_eq!((id.as_str(), &r), ("perm-0", &serde_json::json!({"behavior": "deny", "message": "BashCutPro could not show this request."})));
    assert_eq!(permission_items(&agent).last().map(String::as_str), Some("denied"));
    no_error(&agent);

    // the process ends while they wait
    agent.send("ask 2", &f.ctx).unwrap();
    asking(&mut agent, 2);
    kill(*f.pids().last().unwrap());
    until(&mut agent, "the killed agent to end", |a| a.state_json()["live"] == false);
    assert!(agent.permissions().is_empty());
    assert_eq!(permission_items(&agent).iter().rev().take(2).collect::<Vec<_>>(), ["withdrawn", "withdrawn"]);
    assert!(matches!(agent.status, Status::Failed { .. }), "{:?}", agent.status);
    agent.shutdown();
}

#[test]
fn claude_code_needs_the_bridge() {
    let mut agent = Agent::new(Vec::new());
    assert_eq!(agent.agent.id, "claude-code");
    let ctx = Context::default();
    let e = agent.send("hi", &ctx).unwrap_err();
    assert!(e.0.contains("bashcutpro-cli") || e.0.contains("--control"), "{e}");
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
