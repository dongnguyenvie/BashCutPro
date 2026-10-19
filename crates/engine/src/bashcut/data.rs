//! BashCutPro: where our files live in the data directory (`<data dir>/bashcut/…`).

use std::path::PathBuf;

use serde_json::Value;

use crate::commands::str_p;

/// `<data dir>/bashcut/<sub>`, or `p["dir"]` when the caller names a folder (tests, batch runs).
pub fn folder(p: &Value, sub: &str) -> Option<PathBuf> {
    if let Some(d) = str_p(p, "dir").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    crate::autosave::default_data_dir().map(|d| d.join("bashcut").join(sub))
}

/// A plain file or folder name (no separators, no `..`): safe to join under one of our folders.
pub fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && name != "." && name != ".." && !name.contains(['/', '\\', ':']) && !name.contains("..")
}

/// Read a small UTF-8 JSON file (at most `cap` bytes).
pub fn read_json(path: &std::path::Path, cap: u64) -> Result<Value, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if len > cap {
        return Err(format!("{}: larger than {cap} bytes", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write pretty JSON, creating the parent folder.
pub fn write_json(path: &std::path::Path, v: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}
