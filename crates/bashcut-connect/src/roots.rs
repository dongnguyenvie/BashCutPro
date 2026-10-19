//! Where an agent may write: the project's folder, the folders in `BASHCUTPRO_ROOTS` (the OS path
//! list separator: `:` on Unix, `;` on Windows) and any given with `--roots`. A path is inside a
//! root after `..` is resolved and symlinks in its existing part are followed, so neither can be
//! used to step out.

use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roots(Vec<PathBuf>);

impl Roots {
    /// The project's folder (when there is a project), then `env` (a `BASHCUTPRO_ROOTS` value), then
    /// `extra`; relative entries are taken from `cwd`.
    pub fn new(project: Option<&Path>, env: Option<&std::ffi::OsStr>, extra: &[PathBuf], cwd: &Path) -> Roots {
        let mut v: Vec<PathBuf> = Vec::new();
        if let Some(folder) = project.and_then(|p| absolute(p, cwd).parent().map(Path::to_path_buf)) {
            v.push(folder);
        }
        if let Some(list) = env {
            v.extend(std::env::split_paths(list).filter(|p| !p.as_os_str().is_empty()));
        }
        v.extend(extra.iter().cloned());
        let mut roots: Vec<PathBuf> = Vec::new();
        for r in v {
            let r = resolve(&absolute(&r, cwd));
            if !roots.contains(&r) {
                roots.push(r);
            }
        }
        Roots(roots)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn list(&self) -> &[PathBuf] {
        &self.0
    }

    /// Whether writing `path` (relative to `cwd`) stays inside a root. With no roots, everything is.
    pub fn allows(&self, path: &str, cwd: &Path) -> bool {
        if self.0.is_empty() {
            return true;
        }
        if path.trim().is_empty() {
            return false;
        }
        let p = resolve(&absolute(Path::new(path), cwd));
        self.0.iter().any(|r| p.starts_with(r))
    }
}

/// `p` against `cwd`, with `.` and `..` resolved lexically.
pub fn absolute(p: &Path, cwd: &Path) -> PathBuf {
    let joined = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The longest existing ancestor of `p` canonicalized (symlinks followed), the rest appended.
fn resolve(p: &Path) -> PathBuf {
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(c) = std::fs::canonicalize(&cur) {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        let Some(name) = cur.file_name().map(|n| n.to_os_string()) else { return p.to_path_buf() };
        rest.push(name);
        if !cur.pop() {
            return p.to_path_buf();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bashcut-connect-roots-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("proj")).unwrap();
        std::fs::create_dir_all(d.join("shared")).unwrap();
        d
    }

    #[test]
    fn writes_stay_in_the_project_folder_and_listed_roots() {
        let d = tmp("in");
        let env = d.join("shared").into_os_string();
        let roots = Roots::new(Some(&d.join("proj/a.fcproj")), Some(&env), &[], &d);
        assert_eq!(roots.list().len(), 2);
        assert!(roots.allows(&d.join("proj/exports/a.mp4").to_string_lossy(), &d));
        assert!(roots.allows("proj/new/b.mp4", &d), "relative to cwd");
        assert!(roots.allows(&d.join("shared/x.srt").to_string_lossy(), &d));
        assert!(!roots.allows(&d.join("elsewhere/a.mp4").to_string_lossy(), &d));
        assert!(!roots.allows(&d.join("proj/../elsewhere/a.mp4").to_string_lossy(), &d), "no .. escape");
        assert!(!roots.allows("", &d));
        assert!(Roots::new(None, None, &[], &d).allows("/anything", &d), "no roots: no limit");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_root_does_not_count_as_inside() {
        let d = tmp("link");
        std::os::unix::fs::symlink(d.join("shared"), d.join("proj/out")).unwrap();
        let roots = Roots::new(Some(&d.join("proj/a.fcproj")), None, &[], &d);
        assert!(!roots.allows(&d.join("proj/out/a.mp4").to_string_lossy(), &d));
        let _ = std::fs::remove_dir_all(&d);
    }
}
