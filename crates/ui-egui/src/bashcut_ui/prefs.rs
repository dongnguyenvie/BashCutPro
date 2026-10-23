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
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bashcut-prefs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn interrupted_write_keeps_previous_file() {
        let dir = temp("interrupted");
        let path = dir.join("ui.json");
        write(&path, r#"{"open":"runs"}"#, "test");
        // an interrupted write leaves half a temp file next to the real one (this pid's and another's)
        let stale = dir.join(format!("ui.json.{}.tmp", std::process::id()));
        std::fs::write(&stale, r#"{"op"#).unwrap();
        std::fs::write(dir.join("ui.json.1.tmp"), "").unwrap();
        assert_eq!(read(&path, 1024).as_deref(), Some(r#"{"open":"runs"}"#), "the real file is whole");
        // the next write goes through its own temp file and replaces the real one whole
        write(&path, r#"{"open":"agent"}"#, "test");
        assert_eq!(read(&path, 1024).as_deref(), Some(r#"{"open":"agent"}"#));
        assert!(!stale.exists(), "this process's temp file was renamed over the real one");
        assert_eq!(std::fs::read_to_string(dir.join("ui.json.1.tmp")).unwrap(), "", "another's is left alone");
        // over the cap reads as missing, never as a cut text
        assert_eq!(read(&path, 4), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_if_changed_writes_once() {
        let dir = temp("once");
        let path = dir.join("palette.json");
        let mut saved = None;
        save_if_changed(&path, "a".into(), &mut saved, "test");
        assert_eq!(read(&path, 16).as_deref(), Some("a"));
        assert_eq!(saved.as_deref(), Some("a"));
        // the same text is not written again (the file removed behind its back stays removed)
        std::fs::remove_file(&path).unwrap();
        save_if_changed(&path, "a".into(), &mut saved, "test");
        assert!(!path.exists());
        save_if_changed(&path, "b".into(), &mut saved, "test");
        assert_eq!(read(&path, 16).as_deref(), Some("b"));

        // a folder that cannot be created (a file is in the way): the failed text is remembered,
        // so it is not tried again every frame
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let bad = blocker.join("batch.json");
        let mut saved = None;
        save_if_changed(&bad, "c".into(), &mut saved, "test");
        assert!(!bad.exists());
        assert_eq!(saved.as_deref(), Some("c"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
