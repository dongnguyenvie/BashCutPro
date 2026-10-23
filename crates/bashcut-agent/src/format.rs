//! The agent event formats (S1): one row per format, naming how the message goes to stdin and how
//! stdout is decoded. The names are the manifest's ([`bashcut_manifest::AGENT_EVENTS`]); a new format
//! is a name there, a row here and its parser module.

use crate::stream_json::{self, Event};
use crate::{codex_json, err};

/// Reads an agent's stdout, one line at a time (a parser may keep state across lines).
pub trait Decode: Send {
    fn parse_line(&mut self, line: &str) -> Vec<Event>;
}

/// One agent event format.
pub struct Format {
    /// As named in agent specs and plugin manifests.
    pub name: &'static str,
    /// The message as the agent reads it on stdin.
    pub input: fn(&str) -> String,
    /// A fresh decoder for one turn's stdout.
    pub decoder: fn() -> Box<dyn Decode>,
}

/// Every format an agent may speak.
pub const FORMATS: &[Format] = &[
    Format { name: stream_json::FORMAT, input: |m| format!("{}\n", stream_json::user_message(m)), decoder: || Box::new(stream_json::Parser) },
    Format { name: codex_json::FORMAT, input: |m| m.to_string(), decoder: || Box::new(codex_json::Parser::default()) },
];

/// The format named `name`.
pub fn find(name: &str) -> crate::Result<&'static Format> {
    match FORMATS.iter().find(|f| f.name == name) {
        Some(f) => Ok(f),
        None => err(format!("event format `{name}` is not supported (use {})", names().join(" or "))),
    }
}

pub fn names() -> Vec<&'static str> {
    FORMATS.iter().map(|f| f.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_names_have_a_format() {
        for name in bashcut_manifest::AGENT_EVENTS {
            assert_eq!(find(name).map(|f| f.name).ok(), Some(*name), "{name} has no FORMATS row");
        }
        assert_eq!(names().len(), bashcut_manifest::AGENT_EVENTS.len(), "a FORMATS row the manifest cannot name");
        for bad in ["", "other", "Claude-Stream-Json", "codex-json "] {
            let e = find(bad).err().map(|e| e.0).unwrap_or_default();
            assert!(e.contains("not supported") && e.contains(codex_json::FORMAT), "{bad:?}: {e}");
        }
    }
}
