//! How agents reach BashCutPro (#51; `bashcut/agent-connect.md`).
//!
//! - [`automation`]: `<data dir>/bashcut/automation.json`, written by the open window while its
//!   control channel runs; [`automation::live`] checks the process and the port.
//! - [`lock`]: one engine per project: `<name>.fcproj.bashcut/engine.lock`.
//! - [`roots`]: where an agent may write (the project's folder and `BASHCUTPRO_ROOTS`).
//! - [`guard`]: checks MCP `tools/call` requests for writes outside those roots.
//!
//! Only workspace dependency: `bashcut-dirs`; `apps/bashcut-mcp` puts it together.

pub mod automation;
pub mod guard;
pub mod lock;
pub mod roots;

/// Whether a process with this id runs ([`bashcut_dirs::pid_alive`]).
pub use bashcut_dirs::pid_alive;
