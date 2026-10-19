//! BashCutPro's agent sessions (fork-only; design: `bashcut/in-app-ui.md` §6, UI: `bashcut_ui/agent_panel.rs`).
//!
//! An agent (Claude Code, or an agent plugin) runs as a child process, one per turn: the user's
//! message plus a context line goes in as a stream-json user message, events stream back into a
//! [`Transcript`], and the next turn resumes the agent's session. The agent edits through
//! `filmcraft-cli mcp --bridge <app>` (a temporary MCP config), so its edits land in History like any
//! other command. Gates come from `<project>.fcproj.bashcut/plan.json`; conversations are kept in
//! `<project>.fcproj.bashcut/conversations/`. Depends on no workspace crate.
//!
//! Everything an agent prints is hostile input: lines and texts are capped, nothing here panics
//! (AGENTS.md §0).

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

use std::fmt;
use std::path::Path;

/// Every error here, as a message a person can act on.
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

/// Read a text file of at most `cap` bytes.
pub(crate) fn read_capped(path: &Path, cap: u64) -> Result<String> {
    let meta = std::fs::metadata(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    if meta.len() > cap {
        return err(format!("{}: larger than {cap} bytes", path.display()));
    }
    std::fs::read_to_string(path).map_err(|e| Error(format!("{}: {e}", path.display())))
}

/// Write `text` to `path` through a temporary file, so a crash never leaves half a file.
pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error(format!("{}: {e}", path.display()))
    })
}

/// `text` cut to at most `max` bytes on a character boundary, with `…` when cut.
pub fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", text.get(..end).unwrap_or_default())
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
