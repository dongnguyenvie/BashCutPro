//! Batch presets: saved pipelines in `<data dir>/bashcut/batch-presets/<name>.json`. The data dir
//! is FilmCraft's (`FILMCRAFT_DATA_DIR`, else the platform's application-data folder), so presets
//! sit next to the user's export presets. A preset's relative paths resolve against the working
//! folder when it runs; `run --input` / `--projects` replace its inputs and projects folder.

use crate::book::write_atomic;
use crate::pipeline::{self, Pipeline};
use std::path::{Path, PathBuf};

/// FilmCraft's data directory, as the engine finds it (portable installs aside).
pub fn default_data_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = env("FILMCRAFT_DATA_DIR") {
        return Some(d);
    }
    if cfg!(target_os = "macos") {
        return env("HOME").map(|h| h.join("Library/Application Support/FilmCraft"));
    }
    if cfg!(windows) {
        return env("APPDATA").map(|h| h.join("FilmCraft"));
    }
    env("XDG_DATA_HOME").map(|d| d.join("filmcraft")).or_else(|| env("HOME").map(|h| h.join(".local/share/filmcraft")))
}

pub fn presets_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("bashcut").join("batch-presets")
}

fn preset_path(data_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let safe = pipeline::safe_name(name);
    if safe != name || name.contains(' ') {
        return Err(format!("preset name {name:?}: use letters, digits, '-', '_' and '.'"));
    }
    Ok(presets_dir(data_dir).join(format!("{safe}.json")))
}

/// Preset names, sorted.
pub fn list(data_dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(presets_dir(data_dir)) else { return Vec::new() };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

/// Check `text` as a pipeline, then save it under `name` (replacing a preset of that name).
pub fn save(data_dir: &Path, name: &str, text: &str) -> Result<PathBuf, String> {
    let path = preset_path(data_dir, name)?;
    Pipeline::parse(text, Path::new("."))?;
    let dir = presets_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_atomic(&path, text.as_bytes())?;
    Ok(path)
}

/// The preset's text.
pub fn read(data_dir: &Path, name: &str) -> Result<String, String> {
    pipeline::read_capped(&preset_path(data_dir, name)?)
}

/// The preset as a pipeline, relative paths resolved against `base`.
pub fn load(data_dir: &Path, name: &str, base: &Path) -> Result<Pipeline, String> {
    Pipeline::parse(&read(data_dir, name)?, base)
}

pub fn delete(data_dir: &Path, name: &str) -> Result<(), String> {
    let path = preset_path(data_dir, name)?;
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_list_load_delete() {
        let dir = std::env::temp_dir().join(format!("bashcut-batch-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let text = r#"{"schema": "bashcut-batch/1", "name": "captions", "inputs": ["clips"], "projectsFolder": "out"}"#;
        assert!(save(&dir, "../evil", text).is_err());
        assert!(save(&dir, "bad", "{}").is_err());
        let path = save(&dir, "captions-draft", text).unwrap();
        assert!(path.ends_with("bashcut/batch-presets/captions-draft.json"));
        assert_eq!(list(&dir), vec!["captions-draft".to_string()]);
        let p = load(&dir, "captions-draft", Path::new("/work")).unwrap();
        assert_eq!(p.projects_dir(), Path::new("/work").join("out"));
        delete(&dir, "captions-draft").unwrap();
        assert!(list(&dir).is_empty());
        assert!(read(&dir, "captions-draft").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
