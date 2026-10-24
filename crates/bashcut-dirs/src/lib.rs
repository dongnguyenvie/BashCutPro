//! BashCutPro's data folder (SB1, `bashcut/rebrand.md`): BashCutPro runs next to upstream FilmCraft
//! without sharing state, so its per-user folder is `BashCutPro` (macOS, Windows) or `bashcutpro`
//! (XDG) instead of FilmCraft's. Overrides and portable installs keep upstream's rules:
//! `BASHCUTPRO_DATA_DIR`, then `FILMCRAFT_DATA_DIR`, then `<exe dir>/data` when `portable.txt`
//! sits next to the executable (#139: a portable install never writes to `%APPDATA%`).
//!
//! No workspace dependencies: the engine (through a one-line hook in `autosave::default_data_dir`),
//! `bashcut-batch`, `bashcut-connect`, `bashcut-doctor` and the UI all ask [`data_dir`]. [`engine`] is
//! the one engine-binary lookup (`bashcutpro-cli` → `bcpro` → `filmcraft-cli`); [`pid_alive`] the one
//! "does this process run" check (`bashcut-connect`, `bashcut-batch`); [`fs`] the one `read_capped` /
//! `write_atomic` / `clip` and message error of every BashCutPro crate (RC4); [`Os`] the one OS mapping (RC5);
//! [`child`] the one child-process supervisor (plugin runs, agent turns, hooks, key store; RC3);
//! [`signal`] the cleanups a window runs on SIGINT / SIGTERM (AG1).

use std::path::{Path, PathBuf};

pub mod child;
pub mod engine;
pub mod fs;
pub mod kit;
pub mod os;
pub mod process;
pub mod signal;
pub mod token;

pub use os::Os;
pub use process::pid_alive;

/// The marker upstream's portable installs put next to the executable (`autosave::PORTABLE_MARKER`).
pub const PORTABLE_MARKER: &str = "portable.txt";

/// Where the process looks: environment variables, the OS family and the executable's folder.
/// Tests inject it; [`Host::current`] reads the real process.
pub struct Host<'a> {
    pub var: &'a dyn Fn(&str) -> Option<String>,
    /// `"macos"`, `"windows"` or anything else (XDG).
    pub os: &'a str,
    pub exe_dir: Option<PathBuf>,
}

fn real_var(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

impl Host<'static> {
    pub fn current() -> Self {
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        Host { var: &real_var, os: std::env::consts::OS, exe_dir }
    }
}

impl Host<'_> {
    fn env(&self, k: &str) -> Option<PathBuf> {
        (self.var)(k).filter(|v| !v.is_empty()).map(PathBuf::from)
    }

    /// `(macOS / Windows name, XDG name)` under the per-user folder.
    fn per_user(&self, names: (&str, &str)) -> Option<PathBuf> {
        match Os::from_name(self.os) {
            Os::Macos => self.env("HOME").map(|h| h.join("Library/Application Support").join(names.0)),
            Os::Windows => self.env("APPDATA").map(|a| a.join(names.0)),
            Os::Linux => self.env("XDG_DATA_HOME").map(|d| d.join(names.1)).or_else(|| self.env("HOME").map(|h| h.join(".local/share").join(names.1))),
        }
    }

    fn portable(&self) -> Option<PathBuf> {
        let exe = self.exe_dir.as_ref()?;
        exe.join(PORTABLE_MARKER).is_file().then(|| exe.join("data"))
    }

    /// BashCutPro's data folder.
    pub fn data_dir(&self) -> Option<PathBuf> {
        self.env("BASHCUTPRO_DATA_DIR")
            .or_else(|| self.env("FILMCRAFT_DATA_DIR"))
            .or_else(|| self.portable())
            .or_else(|| self.per_user(("BashCutPro", "bashcutpro")))
    }

    /// Upstream FilmCraft's per-user folder (read only: the one-time import and the models
    /// fallback). `None` when an override or a portable install picks the folder: then there is
    /// nothing of FilmCraft's to share.
    pub fn filmcraft_dir(&self) -> Option<PathBuf> {
        if self.env("BASHCUTPRO_DATA_DIR").is_some() || self.env("FILMCRAFT_DATA_DIR").is_some() || self.portable().is_some() {
            return None;
        }
        self.per_user(("FilmCraft", "filmcraft"))
    }
}

/// BashCutPro's data folder for this process (see the crate docs). The first call in a process
/// runs the one-time [`import_from`] FilmCraft's folder when ours does not exist yet.
pub fn data_dir() -> Option<PathBuf> {
    let host = Host::current();
    let ours = host.data_dir()?;
    static IMPORT: std::sync::Once = std::sync::Once::new();
    IMPORT.call_once(|| {
        if let Some(fc) = host.filmcraft_dir()
            && let Some(copied) = import_from(&fc, &ours)
        {
            eprintln!("BashCutPro: first launch, copied from {}: {}", fc.display(), copied.join(", "));
        }
    });
    Some(ours)
}

/// Written into our folder after the import; lists what was copied, so the import never runs twice.
pub const IMPORT_MARKER: &str = "bashcut/imported-from-filmcraft.txt";

/// The folder under the data folder where the plugin host installs plugins (`<data dir>/bashcut-plugins`).
pub const PLUGINS_DIR: &str = "bashcut-plugins";

/// What the import copies: settings (with the recent projects list), export presets, our `bashcut/`
/// files (profiles, templates, looks, batch presets, agent and side-panel settings) and installed
/// plugins. Not `Auto-Save`, `Recovery`, `Logs` or graphics state (FilmCraft's own), nor `models`
/// (gigabytes: [`models_dir`] links FilmCraft's files per file).
pub const IMPORTED: &[&str] = &["preferences.json", "export-presets.json", "bashcut", PLUGINS_DIR];

/// Runtime files of the open window inside `bashcut/` that must not follow (a stale port and pid).
const NOT_IMPORTED: &[&str] = &["automation.json"];

/// The one-time import (SB1): when `ours` does not exist and `filmcraft` does, copies [`IMPORTED`]
/// and writes [`IMPORT_MARKER`]. Returns what was copied, `None` when it did not run. Best effort:
/// a file that fails to copy is left out of the list, never an error.
pub fn import_from(filmcraft: &Path, ours: &Path) -> Option<Vec<String>> {
    if ours.exists() || !filmcraft.is_dir() || ours.join(IMPORT_MARKER).exists() {
        return None;
    }
    std::fs::create_dir_all(ours.join("bashcut")).ok()?;
    let mut copied = Vec::new();
    for name in IMPORTED {
        let from = filmcraft.join(name);
        if copy_tree(&from, &ours.join(name), 0) {
            copied.push((*name).to_string());
        }
    }
    let log = format!("BashCutPro copied these from {} on its first launch:\n{}\n", filmcraft.display(), copied.join("\n"));
    let _ = std::fs::write(ours.join(IMPORT_MARKER), log);
    Some(copied)
}

/// Copies a file or a folder (no symlinks, at most 16 levels deep); whether anything was copied.
fn copy_tree(from: &Path, to: &Path, depth: u32) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(from) else { return false };
    if meta.is_file() {
        return to.parent().is_some_and(|p| std::fs::create_dir_all(p).is_ok()) && std::fs::copy(from, to).is_ok();
    }
    if !meta.is_dir() || depth > 16 {
        return false;
    }
    let Ok(rd) = std::fs::read_dir(from) else { return false };
    let _ = std::fs::create_dir_all(to);
    let mut any = false;
    for e in rd.flatten() {
        let name = e.file_name();
        if depth == 0 && NOT_IMPORTED.iter().any(|n| name == *n) {
            continue;
        }
        any |= copy_tree(&e.path(), &to.join(&name), depth + 1);
    }
    any
}

/// Where speech and voice models live: always ours (`<data dir>/models`, created), so a download
/// lands in BashCutPro's folder (SB3). Each FilmCraft model file ours lacks is linked in first
/// ([`link_missing`]): reads find ours, else FilmCraft's file, per file, without copying gigabytes.
pub fn models_dir_in(ours: &Path, filmcraft: Option<&Path>) -> PathBuf {
    let mine = ours.join("models");
    let _ = std::fs::create_dir_all(&mine);
    if let Some(theirs) = filmcraft.map(|f| f.join("models")) {
        link_missing(&theirs, &mine, 0);
    }
    mine
}

/// Links every file under `theirs` that `mine` does not have (hard link, else a symlink on Unix;
/// best effort, at most 8 levels, no `.part` leftovers, never replaces a file of ours). FilmCraft's
/// files stay read only: the downloader writes `<name>.part` and renames it over the link, it never
/// writes through it.
pub fn link_missing(theirs: &Path, mine: &Path, depth: u32) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(theirs) else { return };
    for e in rd.flatten() {
        let (from, to) = (e.path(), mine.join(e.file_name()));
        let Ok(meta) = std::fs::symlink_metadata(&from) else { continue };
        if meta.is_dir() {
            if std::fs::create_dir_all(&to).is_ok() {
                link_missing(&from, &to, depth + 1);
            }
        } else if meta.is_file()
            && from.extension().is_none_or(|x| x != "part")
            && std::fs::symlink_metadata(&to).is_err()
            && std::fs::hard_link(&from, &to).is_err()
        {
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(&from, &to);
        }
    }
}

/// [`models_dir_in`] for this process. The panels ask every frame, so FilmCraft's folder is
/// walked once per process; later calls only make sure ours exists.
pub fn models_dir() -> Option<PathBuf> {
    let ours = data_dir()?;
    static LINK: std::sync::Once = std::sync::Once::new();
    let mut dir = None;
    LINK.call_once(|| dir = Some(models_dir_in(&ours, Host::current().filmcraft_dir().as_deref())));
    Some(dir.unwrap_or_else(|| models_dir_in(&ours, None)))
}

/// The hook in the engine's `autosave::default_data_dir`: returns [`data_dir`] early, except in the
/// engine's own unit tests, which assert upstream's folder. A macro so the hook stays one line.
#[macro_export]
macro_rules! data_dir_first {
    () => {
        if cfg!(not(test)) {
            return $crate::data_dir();
        }
    };
}

/// The hook in the engine's `transcript::models_dir`: returns [`models_dir`] early, except in the
/// engine's own unit tests. A macro so the hook stays one line.
#[macro_export]
macro_rules! models_dir_first {
    () => {
        if cfg!(not(test)) {
            return $crate::models_dir();
        }
    };
}

#[cfg(test)]
mod tests;
