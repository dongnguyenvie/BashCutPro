//! `bashcutpro` (alias `bcpro`), the BashCutPro command line (#154, `bashcut/rebrand.md`).
//!
//! Upstream's binary keeps its name (`filmcraft-cli`), so this is a thin wrapper: it finds
//! `filmcraft-cli` next to itself, else on `PATH`, and runs it with the same arguments, stdio and
//! exit code. On Unix it replaces itself with the engine (`exec`), so signals, process groups and
//! memory are the engine's own; on Windows it waits for the engine and exits with its code.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The names this program ships under.
pub const NAMES: &[&str] = &["bashcutpro", "bcpro"];
/// The engine binary behind it.
pub const ENGINE: &str = "filmcraft-cli";

/// `name` with the platform's executable suffix.
pub fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// Whether `p` names this wrapper (`bashcutpro`, `bcpro`), so it never runs itself in a loop.
fn is_wrapper(p: &Path) -> bool {
    p.file_stem().and_then(OsStr::to_str).is_some_and(|s| NAMES.iter().any(|n| s.eq_ignore_ascii_case(n)))
}

/// The engine: `filmcraft-cli` in `own_dir` (the folder of this program), else the first one in
/// `path` (a `PATH` value).
pub fn find_engine(own_dir: Option<&Path>, path: Option<&OsStr>) -> Option<PathBuf> {
    let name = exe(ENGINE);
    let beside = own_dir.map(|d| d.join(&name));
    let on_path = path.into_iter().flat_map(std::env::split_paths).map(|d| d.join(&name));
    beside.into_iter().chain(on_path).find(|p| p.is_file() && !is_wrapper(p))
}

/// Run the engine with `args`; returns the exit code to leave with (on Unix only when the engine
/// could not be started).
pub fn run(args: Vec<OsString>) -> i32 {
    let own = std::env::current_exe().ok();
    let own_dir = own.as_deref().and_then(Path::parent);
    let Some(engine) = find_engine(own_dir, std::env::var_os("PATH").as_deref()) else {
        eprintln!(
            "bashcutpro: {} was not found next to this program or on PATH; install BashCutPro again, or build it with `cargo build -p filmcraft-cli`",
            exe(ENGINE)
        );
        return 127;
    };
    let mut cmd = Command::new(&engine);
    cmd.args(args);
    exec(cmd, &engine)
}

#[cfg(unix)]
fn exec(mut cmd: Command, engine: &Path) -> i32 {
    use std::os::unix::process::CommandExt;
    // only returns when the engine could not be started
    let e = cmd.exec();
    eprintln!("bashcutpro: cannot start {}: {e}", engine.display());
    126
}

#[cfg(not(unix))]
fn exec(mut cmd: Command, engine: &Path) -> i32 {
    match cmd.status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("bashcutpro: cannot start {}: {e}", engine.display());
            126
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bashcut-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn beside_wins_over_path() {
        let (own, other, empty) = (tmp("own"), tmp("other"), tmp("empty"));
        std::fs::write(own.join(exe(ENGINE)), "").unwrap();
        std::fs::write(other.join(exe(ENGINE)), "").unwrap();
        let path = std::env::join_paths([&other]).unwrap();
        assert_eq!(find_engine(Some(&own), Some(&path)), Some(own.join(exe(ENGINE))));
        assert_eq!(find_engine(Some(&empty), Some(&path)), Some(other.join(exe(ENGINE))));
        assert_eq!(find_engine(Some(&empty), None), None);
    }

    #[test]
    fn never_itself() {
        assert!(is_wrapper(Path::new("/x/bashcutpro")));
        assert!(is_wrapper(Path::new("C:/x/BCPRO.exe")));
        assert!(!is_wrapper(Path::new("/x/filmcraft-cli")));
    }
}
