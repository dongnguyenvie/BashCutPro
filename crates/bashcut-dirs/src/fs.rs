//! The one set of small-file helpers of BashCutPro's crates (RC4, `bashcut/review/crates.md` D4/D5):
//! [`read_capped`] (refuse a file over a cap before reading it), [`write_atomic`] (temp file in the same
//! folder + rename, so a crash never leaves half a file), [`clip`] (cut text on a char boundary) and
//! [`Error`], the message-only error `bashcut-plugin-host` and `bashcut-agent` re-export.
//!
//! Every helper puts the path in its message. Reuse these instead of `std::fs::write` for any file a
//! person would lose (settings, preferences, job books): `write_atomic(&path, text.as_bytes())`.

use std::fmt;
use std::path::{Path, PathBuf};

/// An error as a message a person can act on.
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

impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.0
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// `Err(Error(msg))`.
pub fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

fn at(path: &Path, e: impl fmt::Display) -> Error {
    Error(format!("{}: {e}", path.display()))
}

/// The text of `path`, refused (without reading it) when it is larger than `cap` bytes.
pub fn read_capped(path: &Path, cap: u64) -> Result<String> {
    let meta = std::fs::metadata(path).map_err(|e| at(path, e))?;
    if meta.len() > cap {
        return Err(at(path, format_args!("larger than {cap} bytes")));
    }
    std::fs::read_to_string(path).map_err(|e| at(path, e))
}

/// Write `bytes` to `path` through `<name>.<pid>.tmp` in the same folder, then rename it over `path`:
/// a crash or a full disk leaves the previous file whole. Creates the parent folder. Public for every
/// BashCutPro crate (UI preference stores too).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir).map_err(|e| at(dir, e))?;
    }
    let mut name = path.file_name().map(|n| n.to_os_string()).ok_or_else(|| at(path, "not a file path"))?;
    name.push(format!(".{}.tmp", std::process::id()));
    let tmp = dir.map_or_else(|| PathBuf::from(&name), |d| d.join(&name));
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(at(&tmp, e));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        at(path, e)
    })
}

/// `text` cut to at most `max` bytes on a character boundary; `ellipsis` (`"…"` or `""`) is appended
/// when it was cut, beyond `max`.
pub fn clip(text: &str, max: usize, ellipsis: &str) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ellipsis}", text.get(..end).unwrap_or_default())
}
