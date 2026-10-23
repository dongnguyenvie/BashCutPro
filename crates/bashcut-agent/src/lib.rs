//! BashCutPro's agent sessions (fork-only; design: `bashcut/in-app-ui.md` §6, UI: `bashcut_ui/agent_panel.rs`).
//!
//! An agent (Claude Code, or an agent plugin) runs as a child process, one per turn: the user's
//! message plus a context line goes in as the agent's event format writes it (a row of
//! [`format::FORMATS`]: Claude Code's stream-json or Codex's JSON), events stream back into a
//! [`Transcript`], and the next turn resumes the agent's session. The agent edits through
//! `bashcutpro-cli mcp --bridge <app>` (a temporary MCP config), so its edits land in History like any
//! other command. Gates come from `<project>.fcproj.bashcut/plan.json`; conversations are kept in
//! `<project>.fcproj.bashcut/conversations/`. Depends on `bashcut-dirs` and `bashcut-manifest` only.
//!
//! Everything an agent prints is hostile input: lines and texts are capped, nothing here panics
//! (AGENTS.md §0).

pub mod codex_json;
pub mod format;
pub mod plan;
pub mod process;
pub mod session;
pub mod store;
pub mod stream_json;
pub mod transcript;

pub use plan::Gate;
pub use process::{AgentSpec, Turn, TurnEnd};
pub use session::{Agent, Context, Status};
pub use store::Conversation;
pub use stream_json::Event;
pub use transcript::{Item, Transcript};

use std::path::Path;

/// Every error here, as a message a person can act on (shared with `bashcut-plugin-host`, RC4).
pub use bashcut_dirs::fs::{Error, Result};

pub(crate) use bashcut_dirs::fs::{err, read_capped};

/// Write `text` to `path` through a temporary file, so a crash never leaves half a file.
pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<()> {
    bashcut_dirs::fs::write_atomic(path, text.as_bytes())
}

/// `text` cut to at most `max` bytes on a character boundary, with `…` when cut.
pub fn clip(text: &str, max: usize) -> String {
    bashcut_dirs::fs::clip(text, max, "…")
}

/// The sidecar folder of a project: `<name>.fcproj` → `<name>.fcproj.bashcut/` (`bashcut/in-app-ui.md` §5).
pub fn sidecar(project: &Path) -> std::path::PathBuf {
    let mut name = project.as_os_str().to_os_string();
    name.push(".bashcut");
    name.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_respects_char_boundaries() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("ăâđ", 3), "ă…");
        assert_eq!(clip("ăâđ", 1), "…");
    }

    #[test]
    fn sidecar_sits_next_to_the_project() {
        assert_eq!(sidecar(Path::new("/x/My Film.fcproj")), Path::new("/x/My Film.fcproj.bashcut"));
    }
}
