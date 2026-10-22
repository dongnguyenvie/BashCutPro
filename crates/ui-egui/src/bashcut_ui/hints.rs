//! Setup hints (`bashcut/ux-audit-2026-10.md` X1): what the machine lacks, asked before the user
//! presses Run, and the known setup errors as one sentence each.
//!
//! - [`control_on`]: the app runs its control channel (on by default, see [`super::control`]).
//! - [`python_found`], [`cli_found`]: `python3` (`py -3` on Windows) and the engine start, the
//!   same lookups as `bashcut-doctor`; asked once per session (each is a process call).
//! - [`agent_found`]: an agent's program (its first argument) is a file or on `PATH`; a lookup,
//!   no process call.
//! - [`friendly`]: a raw error as a sentence plus "See Troubleshooting", or unchanged.
//! - [`CONTROL_OFF`], [`HOW_TO_TURN_ON`]: the "control channel is off" line and its tooltip
//!   (English keys for [`s`]).

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use super::plugins;
use super::strings::s;

/// The dim line shown while the control channel is off.
pub const CONTROL_OFF: &str = "Control channel is off: actions that edit the open project cannot run.";

/// How to turn the control channel on (the tooltip of [`CONTROL_OFF`] and of disabled actions).
pub const HOW_TO_TURN_ON: &str =
    "How to turn it on: tick “Allow AI agents to control BashCutPro” in the Agent section and restart BashCutPro (and unset BASHCUT_CONTROL=0 if it is set).";

/// The app has its control channel, so plugins can edit the open project.
pub fn control_on() -> bool {
    plugins::bridge_address().is_some()
}

/// `python3` (`py -3` on Windows) starts: plugins with a Python runtime can run.
pub fn python_found() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| if cfg!(windows) { starts("py", &["-3", "--version"]) } else { starts("python3", &["--version"]) })
}

/// The engine (`bashcutpro` or `filmcraft-cli`) that plugins, batches and agents call starts.
pub fn cli_found() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| starts(&bashcut_batch::cli::default_cli().to_string_lossy(), &["--version"]))
}

/// The agent's program (`claude`, `codex`, `BASHCUT_CLAUDE`'s path, a plugin's `argv[0]`) is a
/// file, or a command found on `PATH`.
pub fn agent_found(spec: &bashcut_agent::AgentSpec) -> bool {
    spec.argv.first().is_some_and(|p| program_found(p, std::env::var_os("PATH").as_deref()))
}

/// `program` is a path to a file, or a bare name found in one of `path`'s folders (with
/// `PATHEXT`'s extensions on Windows).
fn program_found(program: &str, path: Option<&std::ffi::OsStr>) -> bool {
    let program = program.trim();
    if program.is_empty() {
        return false;
    }
    let p = Path::new(program);
    if p.components().count() > 1 || p.is_absolute() {
        return p.is_file() || with_exts(p).iter().any(|c| c.is_file());
    }
    let Some(path) = path else { return false };
    std::env::split_paths(path).any(|dir| {
        let c = dir.join(program);
        c.is_file() || with_exts(&c).iter().any(|c| c.is_file())
    })
}

/// `p` with each Windows executable extension (none elsewhere).
fn with_exts(p: &Path) -> Vec<std::path::PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
    exts.split(';').filter(|e| !e.is_empty()).take(16).map(|e| p.with_extension(e.trim_start_matches('.'))).collect()
}

fn starts(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|st| st.success())
}

/// A known setup error as one sentence plus "See Troubleshooting" (in the UI language); any
/// other error comes back unchanged.
pub fn friendly(err: &str) -> String {
    let sentence = if err.contains("cannot start `python") || err.contains("cannot start `py`") {
        "Python 3 was not found: install Python 3.9 or newer, then run the action again."
    } else if err.contains("cannot start `claude`") {
        "Claude Code was not found: install it and sign in (run claude in a terminal), then try again."
    } else if err.contains("no_cli") {
        "The BashCutPro engine (bashcutpro or filmcraft-cli) was not found next to the app: reinstall BashCutPro or build it."
    } else if err.contains("needs the open project") {
        "This action edits the open project, so it needs the control channel: start BashCutPro with --control 9876."
    } else {
        return err.to_string();
    };
    format!("{} {}", s(sentence), s("See Troubleshooting."))
}

#[cfg(test)]
mod tests {
    use super::super::strings::set_language;
    use super::*;

    #[test]
    fn known_errors_become_one_sentence() {
        set_language(Some("en"));
        let python = friendly("bashcut.plugins.run: cannot start `python3`: No such file or directory (os error 2)");
        assert_eq!(python, "Python 3 was not found: install Python 3.9 or newer, then run the action again. See Troubleshooting.");
        assert!(friendly("cannot start `py`: program not found").starts_with("Python 3 was not found"));
        assert!(friendly("cannot start `claude`: No such file or directory (os error 2)").starts_with("Claude Code was not found"));
        assert!(friendly("no_cli: bashcutpro (or filmcraft-cli) not found: pass --cli <path>").starts_with("The BashCutPro engine"));
        let project = friendly("`Translate captions…` needs the open project: start BashCutPro with --control <port>");
        assert!(project.contains("--control 9876"), "{project}");
        for e in [python, project] {
            assert!(!e.contains("os error"), "{e}");
            assert!(e.ends_with("See Troubleshooting."), "{e}");
        }
        assert_eq!(friendly("bad_params: lines is not a file"), "bad_params: lines is not a file");
        assert_eq!(friendly("cannot start `git`"), "cannot start `git`");
        set_language(None);
    }

    #[test]
    fn agent_program_lookup() {
        let missing = bashcut_agent::AgentSpec::claude_code(Some("bashcut-no-such-agent-ux7".into()));
        assert!(!agent_found(&missing));
        assert!(!agent_found(&bashcut_agent::AgentSpec::default()));
        let Ok(exe) = std::env::current_exe() else { return };
        assert!(agent_found(&bashcut_agent::AgentSpec::codex(Some(exe.to_string_lossy().into_owned()))));
        let (Some(dir), Some(name)) = (exe.parent(), exe.file_stem()) else { return };
        let name = name.to_string_lossy();
        assert!(program_found(&name, Some(dir.as_os_str())), "bare name on PATH");
        assert!(!program_found(&name, None));
        assert!(!program_found("  ", Some(dir.as_os_str())));
    }

    #[test]
    fn hints_have_vietnamese() {
        set_language(Some("vi"));
        for e in ["cannot start `python3`: x", "cannot start `claude`: x", "no_cli: x", "`T` needs the open project: x"] {
            let vi = friendly(e);
            assert!(vi.ends_with(s("See Troubleshooting.")), "{vi}");
            assert_ne!(s("See Troubleshooting."), "See Troubleshooting.");
            assert!(!vi.starts_with(char::is_lowercase) && !vi.contains(" was not found"), "{vi}");
        }
        assert_ne!(s(CONTROL_OFF), CONTROL_OFF);
        assert_ne!(s(HOW_TO_TURN_ON), HOW_TO_TURN_ON);
        assert_ne!(s("needs the control channel"), "needs the control channel");
        set_language(None);
    }
}
