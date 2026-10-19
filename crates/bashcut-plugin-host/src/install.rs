//! Install, update and remove plugins: a registry entry's folder at a git ref, copied into
//! `<data dir>/bashcut-plugins/<id>/`. `git` runs as an external program (no git library, no
//! network crate); every call has a timeout and cannot read `--` options from the registry.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::manifest::{self, Manifest};
use crate::registry::{self, Entry, Registry};
use crate::{Error, Result, err, valid_id};

/// Where an installed plugin came from (`<id>/.bashcut-source.json`).
pub const SOURCE_FILE: &str = ".bashcut-source.json";
const GIT_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_FILES: usize = 20_000;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;
const MAX_DEPTH: usize = 32;

static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Origin {
    pub repo: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub path: String,
    pub commit: String,
}

/// One folder under `bashcut-plugins/`; a broken one is listed with its error.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub id: String,
    pub dir: PathBuf,
    pub manifest: Option<Manifest>,
    pub origin: Option<Origin>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Installer {
    /// `<data dir>/bashcut-plugins`.
    pub root: PathBuf,
    /// The git program (`git`; tests may point elsewhere).
    pub git: String,
}

impl Installer {
    pub fn new(data_dir: &Path) -> Self {
        Self { root: crate::plugins_dir(data_dir), git: "git".to_string() }
    }

    /// The plugin's folder, if `id` is a valid id.
    pub fn dir(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return err(format!("bad plugin id `{id}`"));
        }
        Ok(self.root.join(id))
    }

    /// Every installed plugin, sorted by id.
    pub fn list(&self) -> Vec<Installed> {
        let Ok(rd) = std::fs::read_dir(&self.root) else { return Vec::new() };
        let mut out: Vec<Installed> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|name| valid_id(name))
            .take(1024)
            .map(|id| self.read(&id))
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// One installed plugin (its manifest must name the folder's id).
    pub fn read(&self, id: &str) -> Installed {
        let dir = self.root.join(id);
        let origin = crate::read_capped(&dir.join(SOURCE_FILE), 64 * 1024).ok().and_then(|t| serde_json::from_str(&t).ok());
        let (manifest, error) = match Manifest::load(&dir) {
            Ok(m) if m.id == id => (Some(m), None),
            Ok(m) => (None, Some(format!("manifest id `{}` does not match the folder `{id}`", m.id))),
            Err(e) => (None, Some(e.0)),
        };
        Installed { id: id.to_string(), dir, manifest, origin, error }
    }

    pub fn is_installed(&self, id: &str) -> bool {
        self.dir(id).is_ok_and(|d| d.join(manifest::FILE).is_file())
    }

    /// Read `registry.json` from `repo` at `git_ref`.
    pub fn fetch_registry(&self, repo: &str, git_ref: &str) -> Result<Registry> {
        let local = Path::new(repo);
        if local.is_file() {
            return Registry::parse(&crate::read_capped(local, registry::MAX_BYTES)?);
        }
        let tmp = self.temp("registry")?;
        let r = self.clone_to(repo, git_ref, &tmp).and_then(|_| Registry::parse(&crate::read_capped(&tmp.join(registry::FILE), registry::MAX_BYTES)?));
        remove_dir(&tmp);
        r
    }

    /// Install or update `entry` from its source. Settings of an earlier install are kept.
    pub fn install(&self, entry: &Entry) -> Result<Installed> {
        let dest = self.dir(&entry.id)?;
        if !registry::safe_rel_path(&entry.source.path) {
            return err(format!("{}: bad source path", entry.id));
        }
        let tmp = self.temp(&entry.id)?;
        let staged = self.root.join(format!(".new-{}-{}", entry.id, nonce()));
        let r = (|| {
            let commit = self.clone_to(&entry.source.repo, &entry.source.git_ref, &tmp)?;
            let from = tmp.join(&entry.source.path);
            if !from.join(manifest::FILE).is_file() {
                return err(format!("{}: no {} at {}@{}:{}", entry.id, manifest::FILE, entry.source.repo, entry.source.git_ref, entry.source.path));
            }
            let mut budget = Budget::default();
            copy_tree(&from, &staged, 0, &mut budget)?;
            let m = Manifest::load(&staged)?;
            if m.id != entry.id {
                return err(format!("the manifest says `{}`, the registry `{}`", m.id, entry.id));
            }
            if !m.supports(crate::os()) {
                return err(format!("{} does not run on {}", m.id, crate::os()));
            }
            let origin = Origin { repo: entry.source.repo.clone(), git_ref: entry.source.git_ref.clone(), path: entry.source.path.clone(), commit };
            let text = serde_json::to_string_pretty(&origin).map_err(|e| Error(e.to_string()))?;
            std::fs::write(staged.join(SOURCE_FILE), text)?;
            let settings = crate::settings::file(&self.root, &entry.id);
            if settings.is_file() {
                std::fs::copy(&settings, staged.join(crate::settings::FILE))?;
            }
            swap_in(&staged, &dest)
        })();
        remove_dir(&tmp);
        remove_dir(&staged);
        r?;
        Ok(self.read(&entry.id))
    }

    /// Remove an installed plugin with its settings. Its secrets stay in the key store until the
    /// caller deletes them.
    pub fn remove(&self, id: &str) -> Result<()> {
        let dir = self.dir(id)?;
        if !dir.is_dir() {
            return err(format!("{id} is not installed"));
        }
        std::fs::remove_dir_all(&dir).map_err(|e| Error(format!("{}: {e}", dir.display())))
    }

    fn temp(&self, what: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.root).map_err(|e| Error(format!("{}: {e}", self.root.display())))?;
        Ok(self.root.join(format!(".tmp-{what}-{}-{}", std::process::id(), nonce())))
    }

    /// Shallow clone of `repo` at `git_ref` into `dest`; returns the commit.
    fn clone_to(&self, repo: &str, git_ref: &str, dest: &Path) -> Result<String> {
        if !registry::safe_ref(git_ref) {
            return err(format!("bad git ref `{git_ref}`"));
        }
        let url = registry::clone_url(repo)?;
        let mut cmd = Command::new(&self.git);
        cmd.args(["-c", "advice.detachedHead=false", "clone", "--quiet", "--depth", "1", "--branch", git_ref, "--"]).arg(&url).arg(dest);
        run_git(cmd).map_err(|e| Error(format!("git clone {url}@{git_ref}: {}", e.0)))?;
        let mut rev = Command::new(&self.git);
        rev.arg("-C").arg(dest).args(["rev-parse", "HEAD"]);
        Ok(run_git(rev).map(|o| o.trim().to_string()).unwrap_or_default())
    }
}

fn nonce() -> u64 {
    NONCE.fetch_add(1, Ordering::Relaxed)
}

fn remove_dir(p: &Path) {
    if p.exists() {
        let _ = std::fs::remove_dir_all(p);
    }
}

/// Move `staged` to `dest`, keeping the old folder until the new one is in place.
fn swap_in(staged: &Path, dest: &Path) -> Result<()> {
    let old = dest.with_file_name(format!(".old-{}-{}", dest.file_name().and_then(|n| n.to_str()).unwrap_or("plugin"), nonce()));
    let had = dest.exists();
    if had {
        std::fs::rename(dest, &old)?;
    }
    if let Err(e) = std::fs::rename(staged, dest) {
        if had {
            let _ = std::fs::rename(&old, dest);
        }
        return Err(e.into());
    }
    remove_dir(&old);
    Ok(())
}

/// Run git with no prompt and a timeout; stdout on success, stderr's tail on failure.
fn run_git(mut cmd: Command) -> Result<String> {
    cmd.env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| Error(format!("cannot start git ({e}); install git and try again")))?;
    let out = child.stdout.take();
    let errs = child.stderr.take();
    let read_out = std::thread::spawn(move || read_all(out, 1024 * 1024));
    let read_err = std::thread::spawn(move || read_all(errs, 64 * 1024));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() > GIT_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return err("timed out");
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.into()),
        }
    };
    let stdout = read_out.join().unwrap_or_default();
    let stderr = read_err.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else {
        let tail: String = stderr.trim().chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        err(if tail.is_empty() { format!("exit {status}") } else { tail })
    }
}

fn read_all<R: std::io::Read>(r: Option<R>, cap: u64) -> String {
    let Some(r) = r else { return String::new() };
    let mut buf = Vec::new();
    let mut r = r.take(cap);
    let _ = std::io::Read::read_to_end(&mut r, &mut buf);
    // drain the rest so the child never blocks on a full pipe
    let _ = std::io::copy(&mut r.into_inner(), &mut std::io::sink());
    String::from_utf8_lossy(&buf).into_owned()
}

#[derive(Default)]
struct Budget {
    files: usize,
    bytes: u64,
}

/// Copy a plugin folder: regular files and folders only (no symlinks, no `.git`), capped.
fn copy_tree(from: &Path, to: &Path, depth: usize, budget: &mut Budget) -> Result<()> {
    if depth > MAX_DEPTH {
        return err("plugin folder nested too deep");
    }
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let name = e.file_name();
        if name == ".git" {
            continue;
        }
        let ty = e.file_type()?;
        let (src, dst) = (e.path(), to.join(&name));
        if ty.is_dir() {
            copy_tree(&src, &dst, depth + 1, budget)?;
        } else if ty.is_file() {
            budget.files += 1;
            budget.bytes = budget.bytes.saturating_add(e.metadata()?.len());
            if budget.files > MAX_FILES || budget.bytes > MAX_TOTAL {
                return err("plugin folder is too large");
            }
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_skips_git_and_caps_depth() {
        let base = std::env::temp_dir().join(format!("bashcut-copy-{}-{}", std::process::id(), nonce()));
        let src = base.join("src");
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::create_dir_all(src.join("a/b")).unwrap();
        std::fs::write(src.join("a/b/f.txt"), "x").unwrap();
        std::fs::write(src.join(".git/HEAD"), "x").unwrap();
        let mut budget = Budget::default();
        copy_tree(&src, &base.join("dst"), 0, &mut budget).unwrap();
        assert!(base.join("dst/a/b/f.txt").is_file());
        assert!(!base.join("dst/.git").exists());
        assert!(copy_tree(&src, &base.join("deep"), MAX_DEPTH + 1, &mut Budget::default()).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_bad_ids_and_refs() {
        let i = Installer::new(&std::env::temp_dir().join("bashcut-none"));
        assert!(i.dir("../etc").is_err());
        assert!(i.clone_to("o/r", "--upload-pack=x", Path::new("/nonexistent")).is_err());
        assert!(i.remove("not.installed").is_err());
    }
}
