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
//! AG9: Install kit runs the agent-kit README's two `claude plugin` commands ([`kit_job`]), and
//! when the live session showed no kit, Open in Terminal passes the app's MCP server from a config
//! kept in the data folder ([`terminal_mcp_config`]) so the terminal agent still reaches the app.

use std::path::Path;
use std::process::Command;

use bashcut_dirs::child;

/// The official installers (user-started only, never automatic).
pub const INSTALL_UNIX: &str = "curl -fsSL https://claude.ai/install.sh | bash";
pub const INSTALL_WINDOWS: &str = "irm https://claude.ai/install.ps1 | iex";

/// The agent kit's Claude Code plugin (`/bcp:<recipe>` skills) and its marketplace (agent-kit README).
pub const KIT_PLUGIN: &str = "bcp";
pub const KIT_MARKETPLACE: &str = "dongnguyenvie/bashcutpro-agent-kit";
pub const KIT_INSTALL: &str = "bcp@bashcutpro-agent-kit";

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

/// Open a terminal window running `job` in `dir` (`hint`: one line printed first). Errors when no
/// terminal could be started.
pub fn open(dir: &Path, job: &Job, hint: Option<&str>) -> Result<(), String> {
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

/// The kit's install for `program` (`claude`, or `BASHCUT_CLAUDE`, quoted): add the marketplace, then
/// install the plugin when that worked. User-started only, never automatic.
pub fn kit_job(os: Os, program: &str) -> Job {
    let add = ["plugin", "marketplace", "add", KIT_MARKETPLACE];
    let install = ["plugin", "install", KIT_INSTALL];
    Job::Script(if os == Os::Windows {
        let p = ps_quote(program);
        format!("& {p} {}; if ($LASTEXITCODE -eq 0) {{ & {p} {} }}", add.join(" "), install.join(" "))
    } else {
        let p = sh_quote(program);
        format!("{p} {} && {p} {}", add.join(" "), install.join(" "))
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
    #[test]
    #[ignore = "test ticket: per-OS command lines quote folders with spaces, quotes and Vietnamese letters as one word"]
    fn launches_quote_folder_and_program_per_os() {}

    #[test]
    #[ignore = "test ticket: -EncodedCommand decodes back to the PowerShell script (base64 of UTF-16LE)"]
    fn powershell_script_round_trips_through_base64() {}

    #[test]
    #[ignore = "test ticket: resume_argv drops a session id with shell characters"]
    fn resume_argv_keeps_only_safe_session_ids() {}

    #[test]
    #[ignore = "test ticket: kit_job quotes the program and runs marketplace add then plugin install, per OS"]
    fn kit_job_adds_marketplace_then_installs_bcp() {}
}
