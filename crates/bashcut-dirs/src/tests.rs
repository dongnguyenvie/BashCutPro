use super::*;
use std::collections::HashMap;

fn dirs(vars: &[(&'static str, &str)], os: &str, exe_dir: Option<PathBuf>) -> (Option<PathBuf>, Option<PathBuf>) {
    let vars: HashMap<&'static str, String> = vars.iter().map(|(k, v)| (*k, v.to_string())).collect();
    let var = |k: &str| vars.get(k).cloned();
    let h = Host { var: &var, os, exe_dir };
    (h.data_dir(), h.filmcraft_dir())
}

#[test]
fn per_os_folders_are_ours_next_to_filmcraft() {
    let (d, f) = dirs(&[("HOME", "/Users/a")], "macos", None);
    assert_eq!(d, Some(PathBuf::from("/Users/a/Library/Application Support/BashCutPro")));
    assert_eq!(f, Some(PathBuf::from("/Users/a/Library/Application Support/FilmCraft")));
    let (d, f) = dirs(&[("APPDATA", r"C:\Users\a\AppData\Roaming")], "windows", None);
    assert_eq!(d, Some(PathBuf::from(r"C:\Users\a\AppData\Roaming").join("BashCutPro")));
    assert_eq!(f, Some(PathBuf::from(r"C:\Users\a\AppData\Roaming").join("FilmCraft")));
    let (d, f) = dirs(&[("XDG_DATA_HOME", "/x"), ("HOME", "/h")], "linux", None);
    assert_eq!(d, Some(PathBuf::from("/x/bashcutpro")));
    assert_eq!(f, Some(PathBuf::from("/x/filmcraft")));
    let (d, _) = dirs(&[("XDG_DATA_HOME", ""), ("HOME", "/h")], "linux", None);
    assert_eq!(d, Some(PathBuf::from("/h/.local/share/bashcutpro")));
    assert_eq!(dirs(&[], "macos", None), (None, None));
}

#[test]
fn overrides_win_and_hide_filmcraft() {
    let (d, f) = dirs(&[("HOME", "/h"), ("FILMCRAFT_DATA_DIR", "/fc")], "macos", None);
    assert_eq!((d, f), (Some(PathBuf::from("/fc")), None));
    let (d, f) = dirs(&[("HOME", "/h"), ("FILMCRAFT_DATA_DIR", "/fc"), ("BASHCUTPRO_DATA_DIR", "/bc")], "macos", None);
    assert_eq!((d, f), (Some(PathBuf::from("/bc")), None));
}

#[test]
fn portable_install_never_uses_the_per_user_folder() {
    let tmp = std::env::temp_dir().join(format!("bashcut-dirs-portable-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let (d, _) = dirs(&[("APPDATA", r"C:\r")], "windows", Some(tmp.clone()));
    assert_eq!(d, Some(PathBuf::from(r"C:\r").join("BashCutPro")), "no marker: per-user folder");
    let _ = std::fs::write(tmp.join(PORTABLE_MARKER), "");
    let (d, f) = dirs(&[("APPDATA", r"C:\r")], "windows", Some(tmp.clone()));
    assert_eq!((d, f), (Some(tmp.join("data")), None));
    let _ = std::fs::remove_dir_all(&tmp);
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bashcut-dirs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn put(p: &Path, text: &str) {
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(p));
    let _ = std::fs::write(p, text);
}

#[test]
fn import_copies_settings_and_our_files_once() {
    let root = tmp("import");
    let (fc, ours) = (root.join("FilmCraft"), root.join("BashCutPro"));
    put(&fc.join("preferences.json"), r#"{"recentProjects":["/a.fcproj"]}"#);
    put(&fc.join("export-presets.json"), "[]");
    put(&fc.join("bashcut/profiles/p.json"), "{}");
    put(&fc.join("bashcut/batch-presets/b.json"), "{}");
    put(&fc.join("bashcut/automation.json"), "{}");
    put(&fc.join("bashcut-plugins/x/plugin.json"), "{}");
    put(&fc.join("Auto-Save/a.fcproj"), "x");
    put(&fc.join("Recovery/r"), "x");
    put(&fc.join("models/big.bin"), "x");

    let copied = import_from(&fc, &ours).unwrap_or_default();
    assert_eq!(copied, ["preferences.json", "export-presets.json", "bashcut", "bashcut-plugins"]);
    for f in ["preferences.json", "export-presets.json", "bashcut/profiles/p.json", "bashcut/batch-presets/b.json", "bashcut-plugins/x/plugin.json"] {
        assert!(ours.join(f).is_file(), "{f} copied");
    }
    for f in ["bashcut/automation.json", "Auto-Save", "Recovery", "models"] {
        assert!(!ours.join(f).exists(), "{f} not copied");
    }
    let log = std::fs::read_to_string(ours.join(IMPORT_MARKER)).unwrap_or_default();
    assert!(log.contains("preferences.json") && log.contains("bashcut-plugins"), "{log}");

    // never twice: a later change in FilmCraft's folder does not follow
    put(&fc.join("bashcut/profiles/new.json"), "{}");
    assert_eq!(import_from(&fc, &ours), None);
    assert!(!ours.join("bashcut/profiles/new.json").exists());
    // and nothing to import from: no folder is created
    let other = root.join("Other");
    assert_eq!(import_from(&root.join("missing"), &other), None);
    assert!(!other.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn models_download_target_is_always_ours() {
    let root = tmp("models");
    let (fc, ours) = (root.join("FilmCraft"), root.join("BashCutPro"));
    assert_eq!(models_dir_in(&ours, Some(&fc)), ours.join("models"), "neither: ours");
    assert!(ours.join("models").is_dir(), "created, so downloads land there");
    let _ = std::fs::remove_dir_all(&ours);
    let _ = std::fs::create_dir_all(fc.join("models"));
    assert_eq!(models_dir_in(&ours, Some(&fc)), ours.join("models"), "only FilmCraft's: still ours");
    assert_eq!(models_dir_in(&ours, None), ours.join("models"), "override or portable: ours");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn models_fall_back_to_filmcraft_per_file() {
    let root = tmp("models-per-file");
    let (fc, ours) = (root.join("FilmCraft"), root.join("BashCutPro"));
    put(&fc.join("models/whisper-base/model.bin"), "theirs");
    put(&fc.join("models/whisper-base/tokenizer.json"), "theirs");
    put(&fc.join("models/whisper-base/big.bin.part"), "half");
    put(&fc.join("models/kokoro/voices.bin"), "theirs");
    put(&ours.join("models/whisper-base/model.bin"), "ours");
    let dir = models_dir_in(&ours, Some(&fc));
    assert_eq!(dir, ours.join("models"));
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap_or_default();
    assert_eq!(read("whisper-base/model.bin"), "ours", "ours first");
    assert_eq!(read("whisper-base/tokenizer.json"), "theirs", "missing file: FilmCraft's");
    assert_eq!(read("kokoro/voices.bin"), "theirs", "missing model: FilmCraft's");
    assert!(!dir.join("whisper-base/big.bin.part").exists(), "no half downloads");
    // A download replaces the link (part file + rename), FilmCraft's file is untouched.
    put(&dir.join("kokoro/voices.bin.part"), "new");
    let _ = std::fs::rename(dir.join("kokoro/voices.bin.part"), dir.join("kokoro/voices.bin"));
    assert_eq!(read("kokoro/voices.bin"), "new");
    assert_eq!(std::fs::read_to_string(fc.join("models/kokoro/voices.bin")).unwrap_or_default(), "theirs");
    // Asking again keeps ours.
    let _ = models_dir_in(&ours, Some(&fc));
    assert_eq!(read("kokoro/voices.bin"), "new");
    let _ = std::fs::remove_dir_all(&root);
}
