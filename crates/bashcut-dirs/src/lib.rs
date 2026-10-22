//! BashCutPro's data folder (SB1, `bashcut/rebrand.md`): BashCutPro runs next to upstream FilmCraft
//! without sharing state, so its per-user folder is `BashCutPro` (macOS, Windows) or `bashcutpro`
//! (XDG) instead of FilmCraft's. Overrides and portable installs keep upstream's rules:
//! `BASHCUTPRO_DATA_DIR`, then `FILMCRAFT_DATA_DIR`, then `<exe dir>/data` when `portable.txt`
//! sits next to the executable (#139: a portable install never writes to `%APPDATA%`).
//!
//! No workspace dependencies: the engine (through a one-line hook in `autosave::default_data_dir`),
//! `bashcut-batch`, `bashcut-connect` and `bashcut-doctor` all ask [`data_dir`].

use std::path::{Path, PathBuf};

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
        match self.os {
            "macos" => self.env("HOME").map(|h| h.join("Library/Application Support").join(names.0)),
            "windows" => self.env("APPDATA").map(|a| a.join(names.0)),
            _ => self.env("XDG_DATA_HOME").map(|d| d.join(names.1)).or_else(|| self.env("HOME").map(|h| h.join(".local/share").join(names.1))),
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

/// BashCutPro's data folder for this process (see the crate docs).
pub fn data_dir() -> Option<PathBuf> {
    Host::current().data_dir()
}

/// Upstream FilmCraft's per-user folder, read only (see [`Host::filmcraft_dir`]).
pub fn filmcraft_dir() -> Option<PathBuf> {
    Host::current().filmcraft_dir()
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

#[cfg(test)]
mod tests;
