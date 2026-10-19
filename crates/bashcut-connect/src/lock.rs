//! One engine per project. A headless engine (`filmcraft-cli mcp --project P`) holds
//! `<P>.bashcut/engine.lock` (the project's sidecar folder, `bashcut/extending-upstream.md` §2) for
//! as long as it runs; a second one on the same project is refused. A lock whose process is gone
//! is taken over.

use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<name>.fcproj.bashcut/`, next to the project.
pub fn sidecar(project: &Path) -> PathBuf {
    let mut s = project.as_os_str().to_owned();
    s.push(".bashcut");
    PathBuf::from(s)
}

pub fn lock_path(project: &Path) -> PathBuf {
    sidecar(project).join("engine.lock")
}

/// The pid holding the project's lock, if that process is alive.
pub fn holder(project: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(lock_path(project)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let pid = u32::try_from(v.get("pid")?.as_u64()?).ok()?;
    crate::pid_alive(pid).then_some(pid)
}

/// Held while it lives; removed on drop.
#[derive(Debug)]
pub struct ProjectLock {
    path: PathBuf,
}

impl ProjectLock {
    /// Take the project's lock for this process (`mode`: what holds it, e.g. `headless`).
    pub fn acquire(project: &Path, mode: &str) -> Result<ProjectLock, String> {
        let dir = sidecar(project);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = lock_path(project);
        for _ in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let body = json!({"pid": std::process::id(), "mode": mode});
                    let _ = f.write_all(body.to_string().as_bytes());
                    return Ok(ProjectLock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Some(pid) = holder(project).filter(|p| *p != std::process::id()) {
                        return Err(format!("{} is already open in another engine (pid {pid}); stop it or connect to it", project.display()));
                    }
                    let _ = std::fs::remove_file(&path);
                }
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        }
        Err(format!("{}: could not take the project lock", path.display()))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_engine_is_refused_and_a_stale_lock_taken_over() {
        let dir = std::env::temp_dir().join(format!("bashcut-connect-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.join("a.fcproj");
        assert_eq!(lock_path(&project), dir.join("a.fcproj.bashcut").join("engine.lock"));

        // held by a live process that is not us
        let mut other = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sleep" })
            .args(if cfg!(windows) { vec!["/C", "ping -n 4 127.0.0.1 >NUL"] } else { vec!["3"] })
            .spawn()
            .unwrap();
        std::fs::create_dir_all(sidecar(&project)).unwrap();
        std::fs::write(lock_path(&project), json!({"pid": other.id(), "mode": "headless"}).to_string()).unwrap();
        let err = ProjectLock::acquire(&project, "headless").unwrap_err();
        assert!(err.contains("already open in another engine"), "{err}");
        let _ = other.kill();
        let _ = other.wait();

        // the holder is gone: taken over, released on drop
        let lock = ProjectLock::acquire(&project, "headless").unwrap();
        assert_eq!(holder(&project), Some(std::process::id()));
        drop(lock);
        assert!(!lock_path(&project).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
