//! The one prefs file store of the side panel (`bashcut/review/ui.md` D5, RU4): every section's small
//! JSON file (`ui.json`, `looks-ui.json`, `library-ui.json`, `palette.json`, `batch.json`) is read
//! with a size cap and written through `bashcut_dirs::fs::write_atomic`, so a crash or a full disk
//! mid-write keeps the previous file whole instead of a truncated one.

use std::path::Path;

use bashcut_dirs::fs;

/// The text of `path`; `None` when it is missing, unreadable or larger than `max` bytes.
pub fn read(path: &Path, max: u64) -> Option<String> {
    fs::read_capped(path, max).ok()
}

/// Write `text` to `path` atomically (creating the folder). Errors are logged under `what`, never fatal.
pub fn write(path: &Path, text: &str, what: &str) {
    if let Err(e) = fs::write_atomic(path, text.as_bytes()) {
        log::warn!("{what} {e}");
    }
}

/// [`write`] unless `text` equals `saved`, then remember it in `saved` either way: a read-only data
/// dir must not be retried every frame.
pub fn save_if_changed(path: &Path, text: String, saved: &mut Option<String>, what: &str) {
    if saved.as_deref() == Some(text.as_str()) {
        return;
    }
    write(path, &text, what);
    *saved = Some(text);
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "test ticket: a stale <name>.<pid>.tmp left by an interrupted write never truncates the real prefs file; read still returns the last whole text"]
    fn interrupted_write_keeps_previous_file() {}

    #[test]
    #[ignore = "test ticket: save_if_changed writes once for the same text and remembers it even when the write fails (read-only dir)"]
    fn save_if_changed_writes_once() {}
}
