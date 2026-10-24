//! Open the OS terminal on a command in a folder (AG5, `bashcut/in-app-ui.md` §6): the Agent
//! section's Open in Terminal, Install and Sign in. The user's own agent program runs there with its
//! own login; the app never reads, stores or passes its credentials.
//!
//! No text reaches a shell unquoted: the folder is the child's working directory (and a quoted
//! `cd` where the terminal starts elsewhere), POSIX words go through [`sh_quote`], and on Windows
//! the PowerShell script travels base64-encoded (`-EncodedCommand`), so spaces, Vietnamese letters,
//! `;`, `%` or quotes in a path stay one word. Each launcher is started through
//! [`bashcut_dirs::child::spawn`] and reaped on a background thread.
//!
//! AG9/AG16/PLK7: Download kit clones the agent kit at the tag pinned in `bashcut/kit.lock` into the
//! data folder, or checks out that tag when the folder already holds a clone ([`kit_job`]; no global
//! `claude plugin install`), and when the live session showed no kit, Open in Terminal passes the
//! app's MCP server from a config kept in the data folder ([`terminal_mcp_config`]) so the terminal
//! agent still reaches the app.
//!
//! [`stand_in`] replaces the window for this process (headless tests): [`open`] then hands the
//! folder, job and hint to it and starts nothing.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use bashcut_dirs::child;

/// The official installers (user-started only, never automatic).
pub const INSTALL_UNIX: &str = "curl -fsSL https://claude.ai/install.sh | bash";
pub const INSTALL_WINDOWS: &str = "irm https://claude.ai/install.ps1 | iex";

/// The agent kit's Claude Code plugin (`/bcp:<recipe>` skills) and its marketplace (agent-kit README).
pub const KIT_PLUGIN: &str = "bcp";
pub const KIT_MARKETPLACE: &str = "dongnguyenvie/bashcutpro-agent-kit";
pub const KIT_INSTALL: &str = "bcp@bashcutpro-agent-kit";
/// The kit's repository (AG16: `git clone` target).
pub const KIT_REPO_URL: &str = "https://github.com/dongnguyenvie/bashcutpro-agent-kit";

/// The kit tag this build is pinned to (PLK7), from `bashcut/kit.lock` (one `vX.Y.Z` line).
/// Download kit clones at this tag or checks it out in an existing clone.
pub const KIT_TAG_RAW: &str = include_str!("../../../../bashcut/kit.lock");

/// [`KIT_TAG_RAW`] with leading/trailing whitespace stripped (the `kit.lock` line ends with `\n`).
pub fn kit_tag() -> &'static str {
    static TAG: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    TAG.get_or_init(|| KIT_TAG_RAW.trim())
}

/// The zip URL for the pinned kit tag, for a machine without git (PLK7).
pub fn kit_zip_url() -> String {
    format!("{KIT_REPO_URL}/archive/refs/tags/{}.zip", kit_tag())
}

/// The OS a launch is built for (a parameter so every branch is testable on any OS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// What runs in the new window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    /// A program and its arguments, each one word.
    Program(Vec<String>),
    /// A shell line we wrote ourselves (the installers): POSIX `sh` on macOS/Linux, PowerShell on
    /// Windows. Never built from user or project text.
    Script(String),
}

/// One terminal program to try, with its arguments.
pub type Launch = (String, Vec<String>);

/// `text` as one POSIX shell word.
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `text` as one PowerShell single-quoted string.
pub fn ps_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The POSIX line the terminal runs: `cd`, the hint, the job, then (`keep`) an interactive shell so
/// the window stays open when the job ends.
pub fn sh_line(dir: &Path, job: &Job, hint: Option<&str>, keep: bool) -> String {
    let mut line = format!("cd {} || exit 1", sh_quote(&dir.to_string_lossy()));
    if let Some(h) = hint.filter(|h| !h.is_empty()) {
        line.push_str(&format!("; printf '%s\\n\\n' {}", sh_quote(h)));
    }
    let run = match job {
        Job::Program(argv) => argv.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" "),
        Job::Script(s) => s.clone(),
    };
    line.push_str("; ");
    line.push_str(&run);
    if keep {
        line.push_str("; exec \"${SHELL:-/bin/sh}\"");
    }
    line
}

/// The PowerShell script the terminal runs (the folder is its working directory already).
pub fn ps_script(job: &Job, hint: Option<&str>) -> String {
    let mut lines = Vec::new();
    if let Some(h) = hint.filter(|h| !h.is_empty()) {
        lines.push(format!("Write-Host {}", ps_quote(h)));
        lines.push("Write-Host ''".to_string());
    }
    lines.push(match job {
        Job::Program(argv) => {
            let words: Vec<String> = argv.iter().map(|a| ps_quote(a)).collect();
            format!("& {}", words.join(" "))
        }
        Job::Script(s) => s.clone(),
    });
    lines.join("\n")
}

/// Standard base64 of `bytes` (what `-EncodedCommand` takes, of the UTF-16LE script).
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for chunk in bytes.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ABC.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `powershell -EncodedCommand` arguments for `script`, left open when it ends.
fn powershell_args(script: &str) -> Vec<String> {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    ["-NoLogo", "-NoExit", "-EncodedCommand"].iter().map(|s| s.to_string()).chain([base64(&utf16)]).collect()
}

/// The terminals to try for `job` in `dir` on `os`, in order (the first that starts wins).
pub fn launches(os: Os, dir: &Path, job: &Job, hint: Option<&str>) -> Vec<Launch> {
    let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match os {
        Os::Mac => {
            // the line is osascript's argument, not part of the AppleScript source: no escaping there
            let line = sh_line(dir, job, hint, false);
            let script = ["on run argv", "tell application \"Terminal\"", "activate", "do script (item 1 of argv)", "end tell", "end run"];
            let mut args: Vec<String> = script.iter().flat_map(|l| ["-e".to_string(), l.to_string()]).collect();
            args.push(line);
            vec![("osascript".into(), args)]
        }
        Os::Windows => {
            let ps = powershell_args(&ps_script(job, hint));
            // `-d .`: the working directory we start it in (a `;` in the path would split wt's line)
            let mut wt = own(&["-d", ".", "powershell.exe"]);
            wt.extend(ps.iter().cloned());
            // `start` opens its own console; the empty "" is its window title
            let mut start = own(&["/c", "start", "\"\"", "powershell.exe"]);
            start.extend(ps);
            vec![("wt.exe".into(), wt), ("cmd.exe".into(), start)]
        }
        Os::Linux => {
            let line = sh_line(dir, job, hint, true);
            let d = dir.to_string_lossy().into_owned();
            vec![
                ("x-terminal-emulator".into(), [own(&["-e", "sh", "-c"]), vec![line.clone()]].concat()),
                ("gnome-terminal".into(), [vec![format!("--working-directory={d}")], own(&["--", "sh", "-c"]), vec![line.clone()]].concat()),
                ("konsole".into(), [own(&["--workdir"]), vec![d], own(&["-e", "sh", "-c"]), vec![line.clone()]].concat()),
                ("xterm".into(), [own(&["-e", "sh", "-c"]), vec![line]].concat()),
            ]
        }
    }
}

/// `program` with `args` in `dir`. `cmd /c start` reads its own line, so it gets the words verbatim
/// (each one is a fixed word or base64).
fn command(program: &str, args: &[String], dir: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(dir);
    #[cfg(windows)]
    if program == "cmd.exe" {
        use std::os::windows::process::CommandExt;
        for a in args {
            cmd.raw_arg(a);
        }
        return cmd;
    }
    cmd.args(args);
    cmd
}

/// What [`open`] calls instead of starting a terminal, once [`stand_in`] set it.
pub type Opener = fn(&Path, &Job, Option<&str>) -> Result<(), String>;

static STAND_IN: OnceLock<Opener> = OnceLock::new();

/// From now on [`open`] calls `f` instead of starting a terminal, for the rest of this process
/// (headless tests: no window opens). `false` when one was set already (the first one stays).
pub fn stand_in(f: Opener) -> bool {
    STAND_IN.set(f).is_ok()
}

/// Open a terminal window running `job` in `dir` (`hint`: one line printed first). Errors when no
/// terminal could be started.
pub fn open(dir: &Path, job: &Job, hint: Option<&str>) -> Result<(), String> {
    if let Some(f) = STAND_IN.get() {
        return f(dir, job, hint);
    }
    let mut last = String::new();
    for (program, args) in launches(Os::current(), dir, job, hint) {
        let cmd = command(&program, &args, dir);
        match child::spawn(cmd, &child::Opts { group: true, ..Default::default() }) {
            Ok(mut c) => {
                let out = c.stdout.take();
                let spawned = std::thread::Builder::new().name("bashcut-terminal".into()).spawn(move || {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if let Some(o) = out {
                            child::tail(o, 4096);
                        }
                        let _ = c.wait();
                    }));
                });
                if let Err(e) = spawned {
                    log::warn!("bashcut terminal: {e}");
                }
                return Ok(());
            }
            Err(e) => last = format!("{program}: {e}"),
        }
    }
    Err(last)
}

/// The official installer as a [`Job`] for `os`.
pub fn install_job(os: Os) -> Job {
    Job::Script(if os == Os::Windows { INSTALL_WINDOWS } else { INSTALL_UNIX }.to_string())
}

/// The kit's download (AG16/PLK7) into `dir` (`<data dir>/bashcut-kit`): `git clone --branch <tag>
/// --depth 1` of [`KIT_REPO_URL`] for a fresh directory, or `git fetch --tags && git checkout <tag>`
/// when `dir` already holds a clone (`cloned`). `git` is the program, quoted. User-started only.
/// The tag comes from `bashcut/kit.lock` via [`kit_tag`]; this pins the kit to the build.
pub fn kit_job(os: Os, git: &str, dir: &Path, cloned: bool) -> Job {
    let dir = dir.to_string_lossy();
    let tag = kit_tag();
    Job::Script(if os == Os::Windows {
        let (g, d, t) = (ps_quote(git), ps_quote(&dir), ps_quote(tag));
        if cloned {
            format!("& {g} -C {d} fetch --tags; & {g} -C {d} checkout {t}")
        } else {
            format!("& {g} clone --branch {t} --depth 1 {} {d}", ps_quote(KIT_REPO_URL))
        }
    } else {
        let (g, d, t) = (sh_quote(git), sh_quote(&dir), sh_quote(tag));
        if cloned {
            format!("{g} -C {d} fetch --tags && {g} -C {d} checkout {t}")
        } else {
            format!("{g} clone --branch {t} --depth 1 {} {d}", sh_quote(KIT_REPO_URL))
        }
    })
}

/// The MCP config Open in Terminal passes when the kit is missing: the app's server
/// ([`bashcut_agent::process::SERVER`]: `<engine> mcp --bridge <address>`) in the data folder, so
/// it outlives the live session's temp copy. Rewritten on each click (the port can change).
pub fn terminal_mcp_config(cli: &str, bridge: &str) -> Result<std::path::PathBuf, String> {
    let dir = bashcut_dirs::data_dir().ok_or("no data folder")?;
    let path = dir.join("agent").join("terminal-mcp.json");
    let v = serde_json::json!({"mcpServers": {bashcut_agent::process::SERVER: {"command": cli, "args": ["mcp", "--bridge", bridge]}}});
    bashcut_dirs::fs::write_atomic(&path, v.to_string().as_bytes()).map_err(|e| e.0)?;
    Ok(path)
}

/// The control command the hook commands post to (`agent::invoke`, `"terminalEvent"`).
pub const EVENT_COMMAND: &str = "bashcut.agent.terminalEvent";

/// Claude Code's hook events for Open in Terminal (AGT1) with the first version that knows each (AGT4). An event the
/// installed `claude` does not know is NOT ignored: 1.0.23 - 2.1.100 discard the whole settings file over a hook key
/// outside their event list, so a floor that is too low loses every hook. Sources: the `@anthropic-ai/claude-code`
/// changelog (`PermissionRequest` 2.0.45, `SessionEnd` 1.0.85, `StopFailure` 2.1.78 - each "Added ... hook" entry) and
/// Orca's `claude-hook-event-versions.ts`, whose floors come from the hook enum in each npm release's `cli.js`
/// (`PostToolUseFailure` is in no changelog entry; that enum puts it at 2.0.56, not 2.0.45).
const HOOK_EVENTS: &[(&str, (u32, u32, u32))] = &[
    ("PreToolUse", (1, 0, 23)),
    ("PostToolUse", (1, 0, 23)),
    ("Stop", (1, 0, 31)),
    ("UserPromptSubmit", (1, 0, 53)),
    ("SessionStart", (1, 0, 62)),
    ("SessionEnd", (1, 0, 85)),
    ("PermissionRequest", (2, 0, 45)),
    ("PostToolUseFailure", (2, 0, 56)),
    ("StopFailure", (2, 1, 78)),
];

/// What an unknown version (`claude --version` failed) is assumed to be: old enough to keep to events known since
/// 1.0.62 (Orca's floor for the same case), so settings are never rejected.
const UNKNOWN_VERSION: (u32, u32, u32) = (1, 0, 64);

/// The hook events to pass to a Claude Code of `version` (`None`: unknown, see [`UNKNOWN_VERSION`]).
pub fn hook_events(version: Option<(u32, u32, u32)>) -> Vec<&'static str> {
    let v = version.unwrap_or(UNKNOWN_VERSION);
    HOOK_EVENTS.iter().filter(|(_, since)| v >= *since).map(|(e, _)| *e).collect()
}

/// `(major, minor, patch)` from `claude --version` output ("2.1.310 (Claude Code)").
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.split_whitespace().next()?.split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// The version of the agent program at `program`; asked once per program (it starts the program).
pub fn claude_version(program: &str) -> Option<(u32, u32, u32)> {
    static SEEN: OnceLock<std::sync::Mutex<std::collections::HashMap<String, Option<(u32, u32, u32)>>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if let Some(v) = seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(program) {
        return *v;
    }
    let out = Command::new(program).arg("--version").output().ok().filter(|o| o.status.success());
    let v = out.and_then(|o| parse_version(&String::from_utf8_lossy(&o.stdout)));
    seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(program.to_string(), v);
    v
}

/// The shell line every hook runs (Claude Code runs hooks with `sh`, also in Git Bash on Windows): the hook's JSON from
/// stdin becomes the `event` of one `run -` request to the app through `cli --bridge`; the token comes from the data
/// folder like for every `bashcutpro-cli`. Output is dropped (a `PermissionRequest` hook's stdout is read as a
/// decision) and a failure is ignored: the status is for the panel, it never changes the session.
pub fn hook_command(cli: &str, bridge: &str, data_dir_env: Option<&str>) -> String {
    let env = data_dir_env.map(|d| format!("BASHCUTPRO_DATA_DIR={} ", sh_quote(d))).unwrap_or_default();
    format!(
        "printf '{{\"id\":\"{EVENT_COMMAND}\",\"params\":{{\"event\":%s}}}}\\n' \"$(cat)\" | {env}{} --bridge {} run - >/dev/null 2>&1 || true",
        sh_quote(cli),
        sh_quote(bridge)
    )
}

/// The `--settings` file Open in Terminal passes (AGT1): the hooks for `events` in the data folder, rewritten on each
/// click (the port can change); the user's own `~/.claude` is never touched.
pub fn terminal_hooks_config(cli: &str, bridge: &str, events: &[&str]) -> Result<std::path::PathBuf, String> {
    let dir = bashcut_dirs::data_dir().ok_or("no data folder")?;
    let path = dir.join("agent").join("terminal-hooks.json");
    let data_env = std::env::var("BASHCUTPRO_DATA_DIR").ok().filter(|d| !d.trim().is_empty());
    let v = hooks_json(cli, bridge, events, data_env.as_deref());
    bashcut_dirs::fs::write_atomic(&path, v.to_string().as_bytes()).map_err(|e| e.0)?;
    Ok(path)
}

/// The content of the `--settings` file: one command hook per event.
fn hooks_json(cli: &str, bridge: &str, events: &[&str], data_dir_env: Option<&str>) -> serde_json::Value {
    let entry = serde_json::json!([{"hooks": [{"type": "command", "command": hook_command(cli, bridge, data_dir_env), "timeout": 5}]}]);
    let hooks: serde_json::Map<String, serde_json::Value> = events.iter().map(|e| (e.to_string(), entry.clone())).collect();
    serde_json::json!({"hooks": hooks})
}

/// Where the terminal's Claude Code is (AGT1), from its hook events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// The window was opened; no event yet.
    #[default]
    Opened,
    Working,
    /// A permission request is open (sticky, see [`Watch`]).
    Waiting,
    Done,
    /// The turn ended with an API error.
    Failed,
    /// The session ended: the panel takes the conversation back.
    Closed,
}

/// The tool a permission request waits for: its `tool_use_id` when the event has one, else its tool name.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Waiting {
    id: Option<String>,
    tool: Option<String>,
}

/// The state machine of the terminal session's hook events (rules as Orca's Claude status): a `PermissionRequest` stays
/// [`Phase::Waiting`] until the same tool runs (`PreToolUse` / `PostToolUse` / `PostToolUseFailure`) or the turn ends
/// (`UserPromptSubmit`, `Stop`, `StopFailure`); other tools' events do not clear it. `PreCompact` and every event we
/// do not know leave the state alone, `SessionEnd` closes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watch {
    pub phase: Phase,
    waiting: Option<Waiting>,
    /// The session's `.jsonl` as the last event named it (`transcript_path`); only ever read, for its mtime.
    transcript: Option<std::path::PathBuf>,
}

impl Watch {
    /// Take one hook event (the JSON Claude Code sent); `true` when it closed the session.
    pub fn apply(&mut self, event: &serde_json::Value) -> bool {
        let ended = self.step(event);
        let path = event.get("transcript_path").and_then(serde_json::Value::as_str).filter(|s| !s.is_empty());
        if let Some(path) = path.filter(|_| self.phase != Phase::Closed) {
            self.transcript = Some(path.into());
        }
        ended
    }

    fn step(&mut self, event: &serde_json::Value) -> bool {
        let text = |k: &str| event.get(k).and_then(serde_json::Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let (id, tool) = (text("tool_use_id"), text("tool_name"));
        let Some(name) = text("hook_event_name") else { return false };
        let name = name.as_str();
        if self.phase == Phase::Closed && name != "SessionStart" {
            return false;
        }
        match name {
            "SessionStart" => *self = Watch::default(),
            "UserPromptSubmit" => self.set(Phase::Working),
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                if self.phase == Phase::Waiting && !self.waits_for(id.as_deref(), tool.as_deref()) {
                    return false;
                }
                self.set(Phase::Working);
            }
            "PermissionRequest" => {
                self.phase = Phase::Waiting;
                self.waiting = Some(Waiting { id, tool });
            }
            "Stop" => self.set(Phase::Done),
            "StopFailure" => self.set(Phase::Failed),
            // /clear and resume end one session and start the next in the same window
            "SessionEnd" if matches!(text("reason").as_deref(), Some("clear" | "resume")) => {}
            "SessionEnd" => {
                self.set(Phase::Closed);
                return true;
            }
            _ => {}
        }
        false
    }

    /// The session file was not written to within `window` (`true` too when no event named it or it cannot be read, so the
    /// hook silence alone decides, as before).
    pub fn session_untouched(&self, window: std::time::Duration) -> bool {
        let modified = self.transcript.as_deref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok());
        modified.and_then(|t| t.elapsed().ok()).is_none_or(|age| age >= window)
    }

    fn set(&mut self, phase: Phase) {
        self.phase = phase;
        self.waiting = None;
    }

    /// The event is the tool the open permission request waits for (ids when both have one, else tool names; an event
    /// with neither clears it rather than leave the status stuck).
    fn waits_for(&self, id: Option<&str>, tool: Option<&str>) -> bool {
        let Some(w) = &self.waiting else { return true };
        match (w.id.as_deref(), id, w.tool.as_deref(), tool) {
            (Some(a), Some(b), ..) => a == b,
            (_, _, Some(a), Some(b)) => a == b,
            _ => true,
        }
    }
}

/// The agent program followed by its resume arguments for `session` (when the id looks like one).
pub fn resume_argv(program: &str, resume_args: &[String], session: &str) -> Vec<String> {
    let mut v = vec![program.to_string()];
    let safe = !session.is_empty() && session.len() <= 128 && session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if safe {
        v.extend(resume_args.iter().take(16).map(|a| a.replace("{session}", session)));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder with spaces, both quotes, `;`, `%` and Vietnamese letters.
    const DIR: &str = "/Users/me/Phim của tôi/it's \"Đà Lạt\"; 100%";

    /// The words a POSIX shell reads from `line`: single quotes, `\` outside quotes, whitespace and
    /// an unquoted `;` as its own word only (enough for what [`sh_line`] writes).
    fn sh_words(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut word: Option<String> = None;
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\'' => {
                    let w = word.get_or_insert_with(String::new);
                    for q in chars.by_ref() {
                        if q == '\'' {
                            break;
                        }
                        w.push(q);
                    }
                }
                '\\' => word.get_or_insert_with(String::new).extend(chars.next()),
                c if c.is_whitespace() => words.extend(word.take()),
                ';' => {
                    words.extend(word.take());
                    words.push(";".into());
                }
                c => word.get_or_insert_with(String::new).push(c),
            }
        }
        words.extend(word);
        words
    }

    /// Standard base64 back to bytes (`None` on a character outside the alphabet).
    fn unbase64(text: &str) -> Option<Vec<u8>> {
        const ABC: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bits = 0u32;
        let mut n = 0;
        let mut out = Vec::new();
        for c in text.trim_end_matches('=').chars() {
            bits = (bits << 6) | ABC.find(c)? as u32;
            n += 6;
            if n >= 8 {
                n -= 8;
                out.push((bits >> n) as u8);
            }
        }
        Some(out)
    }

    /// The script a `-EncodedCommand` argument carries (UTF-16LE).
    fn decoded(arg: &str) -> String {
        let bytes = unbase64(arg).unwrap();
        assert_eq!(bytes.len() % 2, 0);
        let units: Vec<u16> = bytes.chunks(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        String::from_utf16(&units).unwrap()
    }

    fn job() -> Job {
        Job::Program(vec!["/opt/my tools/it's/claude".into(), "--resume".into(), "abc-1".into()])
    }

    #[test]
    fn launches_quote_folder_and_program_per_os() {
        let dir = Path::new(DIR);
        let hint = "Gõ /login; xong thì đóng cửa sổ 'này'";
        let program_words = ["/opt/my tools/it's/claude", "--resume", "abc-1"];

        // macOS: osascript gets the line as its argument (not AppleScript source); a shell reads it back word for word
        let mac = launches(Os::Mac, dir, &job(), Some(hint));
        assert_eq!(mac.len(), 1);
        let (program, args) = &mac[0];
        assert_eq!(program, "osascript");
        assert!(args.iter().rev().skip(1).all(|a| !a.contains("Phim")), "the folder is only in the last argument: {args:?}");
        let words = sh_words(args.last().unwrap());
        assert_eq!(words[..2], ["cd", DIR], "{words:?}");
        assert!(words.iter().any(|w| w == hint), "the hint is one word: {words:?}");
        assert!(words.windows(3).any(|w| w == program_words), "program and args are one word each: {words:?}");
        assert!(!words.iter().any(|w| w.contains("SHELL")), "Terminal keeps its own shell open");

        // Linux: every terminal runs `sh -c <line>`; the line keeps the window open afterwards
        let linux = launches(Os::Linux, dir, &job(), None);
        let names: Vec<&str> = linux.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"]);
        for (program, args) in &linux {
            let line = args.last().unwrap();
            assert_eq!(args[args.len() - 3..args.len() - 1], ["sh", "-c"], "{program}: {args:?}");
            let words = sh_words(line);
            assert_eq!(words[..2], ["cd", DIR], "{program}: {words:?}");
            assert!(words.windows(3).any(|w| w == program_words), "{program}: {words:?}");
            assert!(line.ends_with("; exec \"${SHELL:-/bin/sh}\""), "{program}: {line}");
        }
        assert!(linux[1].1.contains(&format!("--working-directory={DIR}")), "gnome-terminal gets the folder as one argument");
        assert!(linux[2].1.windows(2).any(|w| w == ["--workdir", DIR]), "konsole gets the folder as one argument");

        // Windows: the folder is only the working directory; the script travels as base64
        let win = launches(Os::Windows, Path::new(r"C:\Users\Tôi\Phim của tôi; 100% 'x'"), &job(), Some(hint));
        assert_eq!(win.len(), 2);
        let (wt, wt_args) = &win[0];
        assert_eq!(wt, "wt.exe");
        assert_eq!(wt_args[..6], ["-d", ".", "powershell.exe", "-NoLogo", "-NoExit", "-EncodedCommand"]);
        let (cmd, cmd_args) = &win[1];
        assert_eq!(cmd, "cmd.exe");
        assert_eq!(cmd_args[..7], ["/c", "start", "\"\"", "powershell.exe", "-NoLogo", "-NoExit", "-EncodedCommand"]);
        for (program, args) in &win {
            assert_eq!(args.iter().position(|a| a == "-EncodedCommand").map(|i| i + 2), Some(args.len()), "{program}: the script is the last word");
            assert!(args.iter().all(|a| !a.contains("Phim") && !a.contains(';') && !a.contains('%') && !a.contains(' ')), "{program}: {args:?}");
            let script = decoded(args.last().unwrap());
            assert_eq!(script, "Write-Host 'Gõ /login; xong thì đóng cửa sổ ''này'''\nWrite-Host ''\n& '/opt/my tools/it''s/claude' '--resume' 'abc-1'");
        }
    }

    #[test]
    fn powershell_script_round_trips_through_base64() {
        // RFC 4648 §10 vectors
        for (plain, coded) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(plain.as_bytes()), coded);
            assert_eq!(unbase64(coded).unwrap(), plain.as_bytes());
        }
        let script = ps_script(&install_job(Os::Windows), Some("Đang cài Claude Code; 100% \"xong\" 'ngay' 🎬"));
        assert!(script.ends_with(INSTALL_WINDOWS), "{script}");
        let args = powershell_args(&script);
        assert_eq!(args[..3], ["-NoLogo", "-NoExit", "-EncodedCommand"]);
        assert_eq!(args.len(), 4);
        assert_eq!(decoded(&args[3]), script);
        // UTF-16LE: 2 bytes per unit (a surrogate pair for the emoji)
        assert_eq!(args[3].len(), (script.encode_utf16().count() * 2).div_ceil(3) * 4);
    }

    #[test]
    fn resume_argv_keeps_only_safe_session_ids() {
        let resume = vec!["--resume".to_string(), "{session}".to_string()];
        let id = "0b6c3f1e-9d2a-4c55-b1e8_x";
        assert_eq!(resume_argv("claude", &resume, id), ["claude", "--resume", id]);
        assert_eq!(resume_argv("codex", &["resume".to_string(), "{session}".to_string()], "abc"), ["codex", "resume", "abc"]);
        let long = "a".repeat(129);
        for bad in ["", "a b", "a;rm -rf ~", "$(touch x)", "`id`", "a'b", "a\"b", "a|b", "a&b", "x\ny", "phiên-ệ", "../x", "a%PATH%", long.as_str()] {
            assert_eq!(resume_argv("claude", &resume, bad), ["claude"], "{bad:?} is dropped");
        }
        assert_eq!(resume_argv("claude", &resume, &"a".repeat(128)).len(), 3, "128 characters is still an id");
        // at most 16 resume arguments
        let many: Vec<String> = (0..40).map(|i| format!("--x{i}")).collect();
        assert_eq!(resume_argv("claude", &many, "abc").len(), 17);
    }

    #[test]
    fn kit_job_clones_or_pulls() {
        let script = |os, git: &str, dir: &str, cloned| match kit_job(os, git, Path::new(dir), cloned) {
            Job::Script(l) => l,
            other => panic!("{other:?}"),
        };
        let tag = kit_tag();
        let (url_sh, url_ps) = (sh_quote(KIT_REPO_URL), ps_quote(KIT_REPO_URL));
        let (tag_sh, tag_ps) = (sh_quote(tag), ps_quote(tag));
        // fresh clone: --branch <tag> --depth 1
        assert_eq!(script(Os::Linux, "git", "/d/bashcut-kit", false), format!("'git' clone --branch {tag_sh} --depth 1 {url_sh} '/d/bashcut-kit'"));
        // existing clone: fetch tags then checkout the pinned tag
        assert_eq!(
            script(Os::Linux, "git", "/d/bashcut-kit", true),
            format!("'git' -C '/d/bashcut-kit' fetch --tags && 'git' -C '/d/bashcut-kit' checkout {tag_sh}")
        );
        assert_eq!(script(Os::Windows, "git", r"C:\d\bashcut-kit", false), format!(r"& 'git' clone --branch {tag_ps} --depth 1 {url_ps} 'C:\d\bashcut-kit'"));
        assert_eq!(
            script(Os::Windows, "git", r"C:\d\bashcut-kit", true),
            format!(r"& 'git' -C 'C:\d\bashcut-kit' fetch --tags; & 'git' -C 'C:\d\bashcut-kit' checkout {tag_ps}")
        );
        // spaces and quotes in git path stay one word (cloned path)
        let g_sh = sh_quote("/opt/it's git/git");
        let d_sh = sh_quote("/Users/a b/it's");
        assert_eq!(
            script(Os::Mac, "/opt/it's git/git", "/Users/a b/it's", true),
            format!("{g_sh} -C {d_sh} fetch --tags && {g_sh} -C {d_sh} checkout {tag_sh}")
        );
        let d_ps = ps_quote(r"C:\Users\a b\it's");
        assert_eq!(script(Os::Windows, "git", r"C:\Users\a b\it's", true), format!("& 'git' -C {d_ps} fetch --tags; & 'git' -C {d_ps} checkout {tag_ps}"));
        // the zip URL includes the pinned tag
        assert!(kit_zip_url().contains(tag), "zip URL contains the tag");
        assert!(kit_zip_url().starts_with(KIT_REPO_URL), "zip URL starts with the repo URL");
    }

    /// PLK7: `kit_job` reads the tag from `bashcut/kit.lock` (not a hardcoded string); the locked tag
    /// appears in both the fresh-clone (`--branch <tag>`) and existing-clone (`checkout <tag>`) forms;
    /// Windows uses PowerShell `;` to separate the two git commands (not `&&`); `kit_zip_url` contains
    /// the same tag.
    #[test]
    fn kit_job_pinned_tag_unit() {
        let tag = kit_tag();
        // the tag comes from kit.lock (compile-time include_str!), not empty and not a raw path
        assert!(!tag.is_empty(), "kit_tag() is non-empty");
        assert!(!tag.contains('\n') && !tag.contains('\r'), "kit_tag() is a single line (no newline): {tag:?}");
        assert!(!tag.contains('/') && !tag.contains('\\'), "kit_tag() looks like a version tag, not a path: {tag:?}");

        let script = |os, git: &str, dir: &str, cloned| match kit_job(os, git, Path::new(dir), cloned) {
            Job::Script(l) => l,
            other => panic!("{other:?}"),
        };

        // POSIX fresh clone: --branch <tag> --depth 1
        let posix_clone = script(Os::Linux, "git", "/kit", false);
        assert!(posix_clone.contains(&format!("--branch {}", sh_quote(tag))), "POSIX clone has --branch <tag>: {posix_clone}");
        assert!(posix_clone.contains("--depth 1"), "POSIX clone has --depth 1: {posix_clone}");
        assert!(!posix_clone.contains("fetch --tags"), "POSIX clone does not fetch tags: {posix_clone}");

        // POSIX existing clone: fetch --tags && checkout <tag>   (&&, not ;)
        let posix_pull = script(Os::Linux, "git", "/kit", true);
        assert!(posix_pull.contains("fetch --tags"), "POSIX pull fetches tags: {posix_pull}");
        assert!(posix_pull.contains(&format!("checkout {}", sh_quote(tag))), "POSIX pull checks out <tag>: {posix_pull}");
        assert!(posix_pull.contains("&&") && !posix_pull.contains(';'), "POSIX uses && not ;: {posix_pull}");

        // Windows fresh clone: & <git> clone --branch <tag> --depth 1
        let win_clone = script(Os::Windows, "git", r"C:\kit", false);
        assert!(win_clone.contains(&format!("--branch {}", ps_quote(tag))), "Windows clone has --branch <tag>: {win_clone}");
        assert!(win_clone.contains("--depth 1"), "Windows clone has --depth 1: {win_clone}");

        // Windows existing clone: & <git> fetch --tags; & <git> checkout <tag>  (;, not &&)
        let win_pull = script(Os::Windows, "git", r"C:\kit", true);
        assert!(win_pull.contains("fetch --tags"), "Windows pull fetches tags: {win_pull}");
        assert!(win_pull.contains(&format!("checkout {}", ps_quote(tag))), "Windows pull checks out <tag>: {win_pull}");
        // PowerShell uses ; to chain commands, not &&
        assert!(win_pull.contains("; &") && !win_pull.contains("&&"), "Windows uses PowerShell ; not &&: {win_pull}");

        // zip URL contains the same locked tag (no-git fallback, PLK7)
        let zip = kit_zip_url();
        assert!(zip.contains(tag), "zip URL contains the locked tag: {zip}");
        assert!(zip.starts_with(KIT_REPO_URL), "zip URL starts with the repo URL: {zip}");
    }

    #[test]
    fn hook_events_per_version_never_exceed_the_detected_one() {
        let v = |s: &str| parse_version(s);
        // every event is written only from the version that knows it (older CLIs discard the whole file otherwise)
        for (version, expected) in [
            ((1, 0, 22), &[][..]),
            ((1, 0, 23), &["PreToolUse", "PostToolUse"][..]),
            ((1, 0, 31), &["PreToolUse", "PostToolUse", "Stop"][..]),
            ((1, 0, 62), &["PreToolUse", "PostToolUse", "Stop", "UserPromptSubmit", "SessionStart"][..]),
            ((1, 0, 84), &["PreToolUse", "PostToolUse", "Stop", "UserPromptSubmit", "SessionStart"][..]),
            ((1, 0, 85), &["PreToolUse", "PostToolUse", "Stop", "UserPromptSubmit", "SessionStart", "SessionEnd"][..]),
            ((2, 0, 45), &["PreToolUse", "PostToolUse", "Stop", "UserPromptSubmit", "SessionStart", "SessionEnd", "PermissionRequest"][..]),
            ((2, 0, 55), &["PreToolUse", "PostToolUse", "Stop", "UserPromptSubmit", "SessionStart", "SessionEnd", "PermissionRequest"][..]),
            (
                (2, 0, 56),
                &["PreToolUse", "PostToolUse", "Stop", "UserPromptSubmit", "SessionStart", "SessionEnd", "PermissionRequest", "PostToolUseFailure"][..],
            ),
            (
                (2, 1, 77),
                &["PreToolUse", "PostToolUse", "Stop", "UserPromptSubmit", "SessionStart", "SessionEnd", "PermissionRequest", "PostToolUseFailure"][..],
            ),
            (
                (2, 1, 78),
                &[
                    "PreToolUse",
                    "PostToolUse",
                    "Stop",
                    "UserPromptSubmit",
                    "SessionStart",
                    "SessionEnd",
                    "PermissionRequest",
                    "PostToolUseFailure",
                    "StopFailure",
                ][..],
            ),
            (
                (3, 0, 0),
                &[
                    "PreToolUse",
                    "PostToolUse",
                    "Stop",
                    "UserPromptSubmit",
                    "SessionStart",
                    "SessionEnd",
                    "PermissionRequest",
                    "PostToolUseFailure",
                    "StopFailure",
                ][..],
            ),
        ] {
            assert_eq!(hook_events(Some(version)), expected, "{version:?}");
            for e in hook_events(Some(version)) {
                let since = HOOK_EVENTS.iter().find(|(n, _)| *n == e).map(|(_, s)| *s).unwrap();
                assert!(since <= version, "{e} needs {since:?}, detected {version:?}");
            }
        }
        // unknown version: the 1.0.64 floor (no SessionEnd, no PermissionRequest)
        assert_eq!(hook_events(None), ["PreToolUse", "PostToolUse", "Stop", "UserPromptSubmit", "SessionStart"]);
        assert_eq!(hook_events(None), hook_events(Some((1, 0, 64))));
        // the version comes from `claude --version`; anything else is unknown
        assert_eq!(v("2.1.310 (Claude Code)"), Some((2, 1, 310)));
        assert_eq!(v("1.0.64"), Some((1, 0, 64)));
        for bad in ["", "claude", "2.1", "2.x.3", "v2.1.3", "-1.0.0"] {
            assert_eq!(v(bad), None, "{bad:?}");
        }
        assert_eq!(hook_events(v("garbage")), hook_events(None));
    }

    #[test]
    fn hook_command_quotes_every_path_and_drops_output() {
        let cmd = hook_command("/opt/it's cli/bashcutpro-cli", "127.0.0.1:9876", Some("/Users/a b/Phim; $HOME/it's"));
        let words = sh_words(&cmd);
        // `printf ... "$(cat)" | BASHCUTPRO_DATA_DIR=<dir> <cli> --bridge <addr> run - >/dev/null 2>&1 || true`
        let pipe = words.iter().position(|w| w == "|").expect("pipes the event into the cli");
        assert_eq!(words[pipe + 1], "BASHCUTPRO_DATA_DIR=/Users/a b/Phim; $HOME/it's");
        assert_eq!(words[pipe + 2], "/opt/it's cli/bashcutpro-cli");
        assert_eq!(words[pipe + 3..pipe + 7], ["--bridge", "127.0.0.1:9876", "run", "-"]);
        assert!(cmd.contains(&format!("\"id\":\"{EVENT_COMMAND}\"")), "{cmd}");
        assert!(cmd.contains("\"params\":{\"event\":%s}"), "{cmd}");
        assert!(cmd.ends_with(">/dev/null 2>&1 || true"), "stdout dropped (a PermissionRequest hook's stdout is a decision), failure ignored: {cmd}");
        // no data folder env: no assignment word
        let plain = hook_command("cli", "b:1", None);
        assert!(!plain.contains("BASHCUTPRO_DATA_DIR"), "{plain}");
        assert!(plain.contains("| 'cli' --bridge 'b:1' run -"), "{plain}");
    }

    #[test]
    fn terminal_hooks_json_has_one_command_hook_per_event() {
        for version in [None, Some((1, 0, 85)), Some((2, 0, 45)), Some((2, 1, 78))] {
            let events = hook_events(version);
            let v = hooks_json("/bin/cli", "127.0.0.1:1", &events, Some("/d"));
            let hooks = v["hooks"].as_object().unwrap();
            assert_eq!(hooks.len(), events.len(), "{version:?}");
            assert_eq!(v.as_object().unwrap().len(), 1, "only `hooks`");
            for e in &events {
                let groups = hooks[*e].as_array().unwrap();
                assert_eq!(groups.len(), 1, "{e}");
                let list = groups[0]["hooks"].as_array().unwrap();
                assert_eq!(list.len(), 1, "{e}: one hook");
                assert_eq!(list[0]["type"], "command");
                assert_eq!(list[0]["timeout"], 5);
                assert_eq!(list[0]["command"], hook_command("/bin/cli", "127.0.0.1:1", Some("/d")).as_str());
            }
        }
        // SessionEnd / PermissionRequest only when the version knows them
        let old = hooks_json("c", "b", &hook_events(None), None);
        assert!(old["hooks"].get("SessionEnd").is_none() && old["hooks"].get("PermissionRequest").is_none());
        let new = hooks_json("c", "b", &hook_events(Some((2, 1, 310))), None);
        assert!(new["hooks"].get("SessionEnd").is_some() && new["hooks"].get("PermissionRequest").is_some());
    }

    fn ev(v: serde_json::Value) -> serde_json::Value {
        v
    }

    #[test]
    fn watch_permission_is_sticky_until_the_same_tool_runs() {
        use serde_json::json;
        let mut w = Watch::default();
        assert_eq!(w.phase, Phase::Opened);
        assert!(!w.apply(&ev(json!({"hook_event_name": "UserPromptSubmit"}))));
        assert_eq!(w.phase, Phase::Working);
        // permission request by id: other tools' events keep it Waiting
        w.apply(&ev(json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_use_id": "t1"})));
        assert_eq!(w.phase, Phase::Waiting);
        for name in ["PreToolUse", "PostToolUse", "PostToolUseFailure"] {
            w.apply(&ev(json!({"hook_event_name": name, "tool_name": "Bash", "tool_use_id": "t2"})));
            assert_eq!(w.phase, Phase::Waiting, "{name} of another tool_use_id (same tool name)");
        }
        w.apply(&ev(json!({"hook_event_name": "PreCompact"})));
        w.apply(&ev(json!({"hook_event_name": "Notification"})));
        w.apply(&ev(json!({"no": "name"})));
        assert_eq!(w.phase, Phase::Waiting, "PreCompact, unknown events and nameless events leave it");
        w.apply(&ev(json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": "t1"})));
        assert_eq!(w.phase, Phase::Working, "the same tool_use_id ran");
        // each of the three tool events of the same id clears it
        for name in ["PreToolUse", "PostToolUse", "PostToolUseFailure"] {
            w.apply(&ev(json!({"hook_event_name": "PermissionRequest", "tool_name": "Edit", "tool_use_id": "e1"})));
            w.apply(&ev(json!({"hook_event_name": name, "tool_use_id": "e1"})));
            assert_eq!(w.phase, Phase::Working, "{name}");
        }
        // no ids: compared by tool name
        w.apply(&ev(json!({"hook_event_name": "PermissionRequest", "tool_name": "Write"})));
        w.apply(&ev(json!({"hook_event_name": "PreToolUse", "tool_name": "Read"})));
        assert_eq!(w.phase, Phase::Waiting, "another tool name");
        w.apply(&ev(json!({"hook_event_name": "PreToolUse", "tool_name": "Write"})));
        assert_eq!(w.phase, Phase::Working);
        // id wins over the name: same name, different id stays; different name, same id clears
        w.apply(&ev(json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_use_id": "a"})));
        w.apply(&ev(json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_use_id": "b"})));
        assert_eq!(w.phase, Phase::Waiting);
        w.apply(&ev(json!({"hook_event_name": "PreToolUse", "tool_name": "Other", "tool_use_id": "a"})));
        assert_eq!(w.phase, Phase::Working);
        // Stop / StopFailure / UserPromptSubmit end the wait
        for (name, phase) in [("Stop", Phase::Done), ("StopFailure", Phase::Failed), ("UserPromptSubmit", Phase::Working)] {
            w.apply(&ev(json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_use_id": "z"})));
            w.apply(&ev(json!({"hook_event_name": name})));
            assert_eq!(w.phase, phase, "{name}");
            // the wait is gone: another tool's event now moves to Working
            w.apply(&ev(json!({"hook_event_name": "PreToolUse", "tool_name": "Read", "tool_use_id": "q"})));
            assert_eq!(w.phase, Phase::Working, "after {name}");
        }
    }

    #[test]
    fn watch_session_end_closes_unless_clear_or_resume_and_start_restarts() {
        use serde_json::json;
        let mut w = Watch::default();
        w.apply(&ev(json!({"hook_event_name": "UserPromptSubmit"})));
        for reason in ["clear", "resume"] {
            assert!(!w.apply(&ev(json!({"hook_event_name": "SessionEnd", "reason": reason}))), "{reason}");
            assert_eq!(w.phase, Phase::Working, "{reason} keeps the window's session");
        }
        for reason in [json!("logout"), json!("prompt_input_exit"), json!("other"), json!(""), json!(null)] {
            let mut w = Watch::default();
            w.apply(&ev(json!({"hook_event_name": "Stop"})));
            assert!(w.apply(&ev(json!({"hook_event_name": "SessionEnd", "reason": reason}))), "{reason}");
            assert_eq!(w.phase, Phase::Closed);
        }
        assert!(w.apply(&ev(json!({"hook_event_name": "SessionEnd"}))), "no reason closes");
        assert_eq!(w.phase, Phase::Closed);
        // closed ignores everything but SessionStart (and does not report a second close)
        for name in ["UserPromptSubmit", "Stop", "PermissionRequest", "PreToolUse", "SessionEnd"] {
            assert!(!w.apply(&ev(json!({"hook_event_name": name}))), "{name}");
            assert_eq!(w.phase, Phase::Closed, "{name}");
        }
        assert!(!w.apply(&ev(json!({"hook_event_name": "SessionStart", "source": "startup"}))));
        assert_eq!(w, Watch::default(), "SessionStart restarts");
        // a pending permission does not survive a restart
        w.apply(&ev(json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_use_id": "x"})));
        w.apply(&ev(json!({"hook_event_name": "SessionStart"})));
        w.apply(&ev(json!({"hook_event_name": "PreToolUse", "tool_name": "Read", "tool_use_id": "y"})));
        assert_eq!(w.phase, Phase::Working);
    }

    #[test]
    fn session_untouched_follows_the_session_file() {
        use std::time::Duration;

        // --- unnamed: no transcript path set → always untouched (falls back to hook-silence rule) ---
        let w = Watch::default();
        assert!(w.session_untouched(Duration::from_secs(600)), "unnamed: no path → true");
        assert!(w.session_untouched(Duration::ZERO), "unnamed: zero window → true");

        // --- missing file: path set but the file does not exist → always untouched (unreadable = old rule) ---
        let mut w = Watch::default();
        let tmp = std::env::temp_dir().join(format!("bcp-tfx39-missing-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&tmp); // ensure it does not exist
        w.apply(&serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "transcript_path": tmp.to_string_lossy().as_ref(),
        }));
        assert!(w.session_untouched(Duration::from_secs(600)), "missing file → true");
        assert!(w.session_untouched(Duration::ZERO), "missing file, zero window → true");

        // --- touched file: write it now; a large window (1 h) means age < window → false ---
        let mut w = Watch::default();
        let touched = std::env::temp_dir().join(format!("bcp-tfx39-touched-{}.jsonl", std::process::id()));
        std::fs::write(&touched, b"{}").expect("write touched file");
        w.apply(&serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "transcript_path": touched.to_string_lossy().as_ref(),
        }));
        // just written: its age is tiny; a 1-hour window is larger → not untouched
        assert!(!w.session_untouched(Duration::from_secs(3600)), "touched file, 1 h window → false");
        let _ = std::fs::remove_file(&touched);

        // --- old file: any file's age is >= 0; a zero window means age >= 0 always → true ---
        let mut w = Watch::default();
        let old = std::env::temp_dir().join(format!("bcp-tfx39-old-{}.jsonl", std::process::id()));
        std::fs::write(&old, b"{}").expect("write old-scenario file");
        w.apply(&serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "transcript_path": old.to_string_lossy().as_ref(),
        }));
        // zero window: age >= 0 is always true → untouched (the file's age exceeds the window)
        assert!(w.session_untouched(Duration::ZERO), "old-scenario file, zero window → true");
        let _ = std::fs::remove_file(&old);
    }
}
