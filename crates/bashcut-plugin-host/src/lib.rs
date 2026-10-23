//! BashCutPro's plugin host (fork-only; design: `bashcut/plugin-host.md`, UI: `bashcut/in-app-ui.md`).
//!
//! Plugins are separate programs (`bashcut-plugin/1` manifests, listed in a `bashcut-registry/1`
//! registry). This crate installs them from a git ref into `<data dir>/bashcut-plugins/<id>/`,
//! keeps their settings, and runs their actions as child processes connected to the open app
//! (`--bridge 127.0.0.1:<control port>`), reading stderr progress, live `bashcut-view/1` forms and
//! one `bashcut-result/1` object from stdout. It depends on no workspace crate and on no OS crate; the egui side lives in
//! `filmcraft-ui-egui`'s `bashcut_ui`.
//!
//! Everything a plugin, a registry or a settings file says is hostile input: sizes are capped,
//! values are checked, and nothing here panics (AGENTS.md §0).

pub mod host;
pub mod install;
pub mod registry;
pub mod runner;
pub mod settings;
pub mod view;

/// The manifest reader shared with the engine's hooks and the doctor (RC5).
pub use bashcut_manifest as manifest;
pub use bashcut_manifest::{Os, valid_id};
pub use host::{AgentLaunch, Host, Job, JobKind, JobState};
pub use manifest::Manifest;
pub use registry::Registry;
pub use runner::{Outcome, PluginResult, RunSpec};
pub use view::View;

/// Every error of the host, as a message a person can act on (shared with `bashcut-agent`, RC4).
pub use bashcut_dirs::fs::{Error, Result};

pub(crate) use bashcut_dirs::fs::{err, read_capped, write_atomic};

/// The folder plugins are installed in: `<data dir>/bashcut-plugins`.
pub fn plugins_dir(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join(bashcut_dirs::PLUGINS_DIR)
}

/// This build's OS, as manifests name it (`macos`, `windows`, `linux`; every other Unix is `linux`).
pub fn os() -> Os {
    Os::current()
}
