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
//! AG9/AG16: Download kit clones the agent kit into the data folder, or pulls it when it is already
//! there ([`kit_job`]; no global `claude plugin install`), and when the live session showed no kit,
//! Open in Terminal passes the app's MCP server from a config kept in the data folder
//! ([`terminal_mcp_config`]) so the terminal agent still reaches the app.
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
/// The kit's repository (AG16: `git clone` target) and its zip, for a machine without git.
pub const KIT_REPO_URL: &str = "https://github.com/dongnguyenvie/bashcutpro-agent-kit";
pub const KIT_ZIP_URL: &str = "https://github.com/dongnguyenvie/bashcutpro-agent-kit/archive/refs/heads/main.zip";

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

/// The kit's download (AG16) into `dir` (`<data dir>/bashcut-kit`): `git clone` of [`KIT_REPO_URL`], or
/// `git pull` when `dir` already holds a clone (`cloned`). `git` is the program, quoted. User-started only.
pub fn kit_job(os: Os, git: &str, dir: &Path, cloned: bool) -> Job {
    let dir = dir.to_string_lossy();
    Job::Script(if os == Os::Windows {
        let (g, d) = (ps_quote(git), ps_quote(&dir));
        if cloned { format!("& {g} -C {d} pull") } else { format!("& {g} clone {} {d}", ps_quote(KIT_REPO_URL)) }
    } else {
        let (g, d) = (sh_quote(git), sh_quote(&dir));
        if cloned { format!("{g} -C {d} pull") } else { format!("{g} clone {} {d}", sh_quote(KIT_REPO_URL)) }
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
        let (url_sh, url_ps) = (sh_quote(KIT_REPO_URL), ps_quote(KIT_REPO_URL));
        assert_eq!(script(Os::Linux, "git", "/d/bashcut-kit", false), format!("'git' clone {url_sh} '/d/bashcut-kit'"));
        assert_eq!(script(Os::Linux, "git", "/d/bashcut-kit", true), "'git' -C '/d/bashcut-kit' pull");
        assert_eq!(script(Os::Windows, "git", r"C:\d\bashcut-kit", false), format!(r"& 'git' clone {url_ps} 'C:\d\bashcut-kit'"));
        assert_eq!(script(Os::Windows, "git", r"C:\d\bashcut-kit", true), r"& 'git' -C 'C:\d\bashcut-kit' pull");
        // spaces and quotes stay one word
        assert_eq!(script(Os::Mac, "/opt/it's git/git", "/Users/a b/it's", true), r"'/opt/it'\''s git/git' -C '/Users/a b/it'\''s' pull");
        assert_eq!(script(Os::Windows, "git", r"C:\Users\a b\it's", true), r"& 'git' -C 'C:\Users\a b\it''s' pull");
    }
}
