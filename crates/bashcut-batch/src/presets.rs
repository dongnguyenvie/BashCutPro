//! Batch presets: saved pipelines in `<data dir>/bashcut/batch-presets/<name>.json`. The data dir
//! is BashCutPro's (`bashcut_dirs`: `BASHCUTPRO_DATA_DIR` / `FILMCRAFT_DATA_DIR`, else its own per-user folder), so presets
//! sit next to the user's export presets. A preset's relative paths resolve against the working
//! folder when it runs; `run --input` / `--projects` replace its inputs and projects folder.

use crate::pipeline::{self, Pipeline};
use bashcut_dirs::fs::write_atomic;
use std::path::{Path, PathBuf};

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

/// Install presets from a pack (`bashcutpro-plugins/packs/batch/`): `src` is one pipeline file or
/// a folder of them (`*.json`, not recursive); each is checked and saved under its file stem,
/// replacing a preset of that name. Returns the names installed.
pub fn install(data_dir: &Path, src: &Path) -> Result<Vec<String>, String> {
    let files: Vec<PathBuf> = if src.is_dir() {
        let rd = std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))?;
        let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json")).collect();
        v.sort();
        v
    } else {
        vec![src.to_path_buf()]
    };
    if files.is_empty() {
        return Err(format!("{}: no .json presets", src.display()));
    }
    // check every file before saving any, so a bad pack installs nothing
    let mut checked = Vec::with_capacity(files.len());
    for f in &files {
        let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        preset_path(data_dir, &name)?;
        let text = pipeline::read_capped(f)?;
        Pipeline::parse(&text, Path::new(".")).map_err(|e| format!("{}: {e}", f.display()))?;
        checked.push((name, text));
    }
    let mut names = Vec::with_capacity(checked.len());
    for (name, text) in checked {
        save(data_dir, &name, &text)?;
        names.push(name);
    }
    Ok(names)
}

/// The preset's one-line description (empty when it has none or does not parse).
pub fn description(data_dir: &Path, name: &str) -> String {
    read(data_dir, name)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("description").and_then(|d| d.as_str()).map(str::to_string))
        .unwrap_or_default()
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
        // a pack folder: all checked first, then installed
        let pack = dir.join("pack");
        std::fs::create_dir_all(&pack).unwrap();
        let described = r#"{"schema": "bashcut-batch/1", "description": "Clean audio", "inputs": ["."], "projectsFolder": "out"}"#;
        std::fs::write(pack.join("clean-audio.json"), described).unwrap();
        std::fs::write(pack.join("bad.json"), "{}").unwrap();
        assert!(install(&dir, &pack).is_err());
        assert!(list(&dir).is_empty(), "a bad pack installs nothing");
        std::fs::remove_file(pack.join("bad.json")).unwrap();
        assert_eq!(install(&dir, &pack).unwrap(), vec!["clean-audio".to_string()]);
        assert_eq!(description(&dir, "clean-audio"), "Clean audio");
        delete(&dir, "clean-audio").unwrap();
        assert!(list(&dir).is_empty());
        assert!(read(&dir, "captions-draft").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
