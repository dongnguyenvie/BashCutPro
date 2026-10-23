//! `python3` in a plugin step's argv, on Windows. There `python3` is usually the Microsoft Store
//! stub (it prints "Python was not found" and exits 9009), while the real interpreter is the `py`
//! launcher or `python`. Presets are written once for every OS, so the runner picks the one that runs.

use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// What an agent step's kit scripts may run with, in probe order ([`agent_python`]); the agent's
/// allow rules (`crate::agent::kit_tools`) are built from it.
pub const AGENT_PYTHONS: &[&str] = &["python3", "python", "py"];

/// `argv` with a leading `python3` swapped for an interpreter that runs, on Windows only.
pub fn resolve(argv: Vec<String>) -> Vec<String> {
    if !cfg!(windows) || !argv.first().is_some_and(|p| is_python3(p)) {
        return argv;
    }
    let Some(found) = windows_python() else { return argv };
    found.iter().cloned().chain(argv.into_iter().skip(1)).collect()
}

/// The interpreter an agent step's kit scripts run with (#223): `$BASHCUT_PYTHON` when set, else on
/// Windows the first of `python3`, `python`, `py` that answers `--version`, else `python3`. One word,
/// because Claude Code's allow rules (`Bash(python:*)`) match the command's first word.
pub fn agent_python() -> String {
    if let Some(p) = std::env::var("BASHCUT_PYTHON").ok().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        return p;
    }
    if !cfg!(windows) {
        return "python3".into();
    }
    static FOUND: OnceLock<String> = OnceLock::new();
    FOUND.get_or_init(|| AGENT_PYTHONS.iter().copied().find(|p| runs(&[p.to_string()])).unwrap_or("python3").to_string()).clone()
}

fn is_python3(program: &str) -> bool {
    let p = program.to_ascii_lowercase();
    p == "python3" || p == "python3.exe"
}

/// The first of `python3`, `py -3`, `python` that answers `--version` (probed once per run).
fn windows_python() -> Option<&'static Vec<String>> {
    static FOUND: OnceLock<Option<Vec<String>>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            [vec!["python3"], vec!["py", "-3"], vec!["python"]].into_iter().map(|c| c.into_iter().map(str::to_string).collect::<Vec<_>>()).find(|c| runs(c))
        })
        .as_ref()
}

fn runs(cmd: &[String]) -> bool {
    let Some((program, args)) = cmd.split_first() else { return false };
    bashcut_dirs::child::no_window(&mut Command::new(program))
        .args(args)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_leading_python3_is_touched() {
        let other = vec!["node".to_string(), "python3".into()];
        assert_eq!(resolve(other.clone()), other);
        let py = resolve(vec!["python3".into(), "plugin.py".into()]);
        assert_eq!(py.last().map(String::as_str), Some("plugin.py"));
        if !cfg!(windows) {
            assert_eq!(py, ["python3", "plugin.py"]);
        }
    }
}
