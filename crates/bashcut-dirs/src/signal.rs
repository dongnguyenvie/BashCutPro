//! Cleanups that run when the process is asked to end by a signal (AG1): SIGINT (Ctrl+C in the
//! terminal that started the app) and SIGTERM (`kill`, a logout). Without them a window killed
//! that way leaves `<data dir>/bashcut/automation.json` behind (readers skip a dead pid, but the
//! file stays). The first [`at_terminate`] installs one handler (`ctrlc`, its own thread, so a
//! cleanup may take locks and touch files); on a signal it runs every cleanup, then exits with
//! 130. Unix only: elsewhere [`at_terminate`] does nothing and returns `false`.
//!
//! A process has one such handler: an app that installs its own `ctrlc` handler (bashcut-batch)
//! does not use this module.

#[cfg(unix)]
use std::sync::{Mutex, PoisonError};

/// The exit status after the cleanups ran.
pub const EXIT: i32 = 130;

#[cfg(unix)]
type Cleanup = Box<dyn Fn() + Send>;

#[cfg(unix)]
static CLEANUPS: Mutex<Vec<(String, Cleanup)>> = Mutex::new(Vec::new());

/// Run `f` on SIGINT / SIGTERM, before the process exits. A second call with the same `name`
/// replaces the first. `false`: no handler (not Unix, or another handler was installed first).
#[cfg(unix)]
pub fn at_terminate(name: &str, f: impl Fn() + Send + 'static) -> bool {
    static INSTALLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let ok = *INSTALLED.get_or_init(|| match ctrlc::set_handler(|| std::process::exit(run())) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("bashcut signal cleanups not installed: {e}");
            false
        }
    });
    let mut list = CLEANUPS.lock().unwrap_or_else(PoisonError::into_inner);
    list.retain(|(n, _)| n != name);
    list.push((name.to_string(), Box::new(f)));
    ok
}

#[cfg(not(unix))]
pub fn at_terminate(_name: &str, _f: impl Fn() + Send + 'static) -> bool {
    false
}

/// Run (and drop) every registered cleanup, each under `catch_unwind`; returns [`EXIT`].
#[cfg(unix)]
pub fn run() -> i32 {
    let list = std::mem::take(&mut *CLEANUPS.lock().unwrap_or_else(PoisonError::into_inner));
    for (name, f) in list {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(&f)).is_err() {
            log::warn!("bashcut signal cleanup {name} panicked");
        }
    }
    EXIT
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{Duration, Instant};

    const CHILD_ENV: &str = "BASHCUT_SIGNAL_TEST_FILE";

    /// Not a test on its own: the child of [`term_runs_the_cleanups`]. Registers a cleanup that
    /// removes the file named in the environment, says it is ready, then waits to be killed.
    #[test]
    fn signal_child() {
        let Some(path) = std::env::var_os(CHILD_ENV).map(PathBuf::from) else { return };
        let target = path.clone();
        assert!(at_terminate("test", move || {
            let _ = std::fs::remove_file(&target);
        }));
        std::fs::write(path.with_extension("ready"), "ready").unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }

    /// AG1: SIGTERM and SIGINT run the cleanups (the file is removed) and exit with [`EXIT`].
    #[test]
    fn term_runs_the_cleanups() {
        for sig in ["-TERM", "-INT"] {
            let dir = std::env::temp_dir().join(format!("bashcut-signal{sig}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join("automation.json");
            std::fs::write(&file, "{}").unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "signal::tests::signal_child", "--nocapture", "--test-threads=1"])
                .env(CHILD_ENV, &file)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let start = Instant::now();
            while !file.with_extension("ready").exists() {
                assert!(start.elapsed() < Duration::from_secs(20), "child never got ready");
                std::thread::sleep(Duration::from_millis(20));
            }
            let sent = Command::new("kill").args([sig, &child.id().to_string()]).status().unwrap();
            assert!(sent.success());
            let status = child.wait().unwrap();
            assert_eq!(status.code(), Some(EXIT), "{sig}");
            assert!(!file.exists(), "{sig}: the cleanup removed the file");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
