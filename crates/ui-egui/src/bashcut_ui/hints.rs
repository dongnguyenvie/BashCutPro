//! Setup hints (`bashcut/ux-audit-2026-10.md` X1): what the machine lacks, asked before the user
//! presses Run, and the known setup errors as one sentence each.
//!
//! - [`control_on`]: the app was started with its control channel (`--control <port>`).
//! - [`python_found`], [`cli_found`]: `python3` (`py -3` on Windows) and the engine start, the
//!   same lookups as `bashcut-doctor`; asked once per session (each is a process call).
//! - [`friendly`]: a raw error as a sentence plus "See Troubleshooting", or unchanged.
//! - [`CONTROL_OFF`], [`HOW_TO_TURN_ON`]: the "control channel is off" line and its tooltip
//!   (English keys for [`s`]).

use std::process::{Command, Stdio};
use std::sync::OnceLock;

use super::plugins;
use super::strings::s;

/// The dim line shown while the control channel is off.
pub const CONTROL_OFF: &str = "Control channel is off: actions that edit the open project cannot run.";

/// How to turn the control channel on (the tooltip of [`CONTROL_OFF`] and of disabled actions).
pub const HOW_TO_TURN_ON: &str = "How to turn it on: quit BashCutPro and start it with its control channel. Mac: open -a BashCutPro --args --control 9876. Or set FILMCRAFT_CONTROL_PORT=9876 first.";

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
