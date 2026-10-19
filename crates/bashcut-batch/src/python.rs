//! `python3` in a plugin step's argv, on Windows. There `python3` is usually the Microsoft Store
//! stub (it prints "Python was not found" and exits 9009), while the real interpreter is the `py`
//! launcher or `python`. Presets are written once for every OS, so the runner picks the one that runs.

use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// `argv` with a leading `python3` swapped for an interpreter that runs, on Windows only.
pub fn resolve(argv: Vec<String>) -> Vec<String> {
    if !cfg!(windows) || !argv.first().is_some_and(|p| is_python3(p)) {
        return argv;
    }
    let Some(found) = windows_python() else { return argv };
    found.iter().cloned().chain(argv.into_iter().skip(1)).collect()
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
    Command::new(program).args(args).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
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
