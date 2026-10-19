//! BashCutPro's plugin host (fork-only; design: `bashcut/plugin-host.md`, UI: `bashcut/in-app-ui.md`).
//!
//! Plugins are separate programs (`bashcut-plugin/1` manifests, listed in a `bashcut-registry/1`
//! registry). This crate installs them from a git ref into `<data dir>/bashcut-plugins/<id>/`,
//! keeps their settings, and runs their actions as child processes connected to the open app
//! (`--bridge 127.0.0.1:<control port>`), reading stderr progress and one `bashcut-result/1` object
//! from stdout. It depends on no workspace crate and on no OS crate; the egui side lives in
//! `filmcraft-ui-egui`'s `bashcut_ui`.
//!
//! Everything a plugin, a registry or a settings file says is hostile input: sizes are capped,
//! values are checked, and nothing here panics (AGENTS.md §0).

pub mod host;
pub mod install;
pub mod manifest;
pub mod registry;
pub mod runner;
pub mod settings;

pub use host::{AgentLaunch, Host, Job, JobKind, JobState};
pub use manifest::Manifest;
pub use registry::Registry;
pub use runner::{Outcome, PluginResult, RunSpec};

use std::fmt;

/// Every error of the host, as a message a person can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

/// The folder plugins are installed in: `<data dir>/bashcut-plugins`.
pub fn plugins_dir(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("bashcut-plugins")
}

/// The OS name manifests use (`macos`, `windows`, `linux`).
pub fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// A plugin or setting id: lower-case ASCII letters, digits, `.`, `-`, `_`; no leading dot; 1–64 chars.
/// Ids name folders and environment entries, so nothing else gets through.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('.')
        && !id.contains("..")
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-' || b == b'_')
}

/// Read a text file of at most `cap` bytes.
pub(crate) fn read_capped(path: &std::path::Path, cap: u64) -> Result<String> {
    let meta = std::fs::metadata(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    if meta.len() > cap {
        return err(format!("{}: larger than {cap} bytes", path.display()));
    }
    std::fs::read_to_string(path).map_err(|e| Error(format!("{}: {e}", path.display())))
}
