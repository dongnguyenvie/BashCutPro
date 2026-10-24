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

#[test]
fn read_capped_refuses_files_over_the_cap() {
    let root = tmp("read-capped");
    let p = root.join("f.txt");
    put(&p, "12345");
    assert_eq!(fs::read_capped(&p, 5), Ok("12345".to_string()), "at the cap");
    let e = fs::read_capped(&p, 4).unwrap_err();
    assert!(e.0.contains(&p.display().to_string()) && e.0.contains("larger than 4 bytes"), "{e}");
    let missing = root.join("missing.txt");
    let e = fs::read_capped(&missing, 5).unwrap_err();
    assert!(e.0.contains(&missing.display().to_string()), "{e}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn write_atomic_replaces_whole_files_only() {
    let root = tmp("write-atomic");
    let p = root.join("a/b/settings.json");
    fs::write_atomic(&p, b"one").unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "one", "parent created");
    fs::write_atomic(&p, b"two").unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "two", "replaced");
    let names: Vec<String> = std::fs::read_dir(root.join("a/b")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["settings.json"], "no .tmp left behind");
    // the rename fails (a folder is in the way): the folder stays and no .tmp is left
    let dir = root.join("a/dir");
    put(&dir.join("keep.txt"), "kept");
    assert!(fs::write_atomic(&dir, b"x").is_err());
    assert_eq!(std::fs::read_to_string(dir.join("keep.txt")).unwrap(), "kept");
    // the parent cannot be created (it is a file): an error naming the path, the old file whole
    let e = fs::write_atomic(&p.join("child"), b"x").unwrap_err();
    assert!(e.0.contains("settings.json"), "{e}");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "two", "old file kept");
    // the temp write fails (a read-only folder): the old file whole, no .tmp
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let ro = root.join("ro");
        put(&ro.join("f.json"), "old");
        let _ = std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555));
        let probe = std::fs::write(ro.join("probe"), "");
        if probe.is_err() {
            // (skipped when running as root, which writes anyway)
            assert!(fs::write_atomic(&ro.join("f.json"), b"new").is_err());
            assert_eq!(std::fs::read_to_string(ro.join("f.json")).unwrap(), "old");
            let names: Vec<String> = std::fs::read_dir(&ro).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            assert_eq!(names, ["f.json"]);
        }
        let _ = std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755));
    }
    let names: Vec<String> = std::fs::read_dir(root.join("a")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn clip_cuts_on_char_boundaries() {
    assert_eq!(fs::clip("short", 10, "…"), "short");
    assert_eq!(fs::clip("exact", 5, "…"), "exact", "at max: not cut, no ellipsis");
    assert_eq!(fs::clip("abcdef", 3, "…"), "abc…");
    assert_eq!(fs::clip("abcdef", 3, ""), "abc");
    // "Tiếng" is T i ế(3 bytes) n g: a cut inside ế backs up to its start
    assert_eq!(fs::clip("Tiếng Việt", 3, "…"), "Ti…");
    assert_eq!(fs::clip("Tiếng Việt", 4, ""), "Ti");
    assert_eq!(fs::clip("Tiếng Việt", 5, "…"), "Tiế…");
    assert_eq!(fs::clip("日本", 2, "…"), "…", "nothing fits: just the ellipsis");
    assert_eq!(fs::clip("", 0, "…"), "");
}

/// A kit folder (it has `skills/`) at `root/rel`.
fn make_kit(root: &Path, rel: &str) -> PathBuf {
    let dir = root.join(rel);
    std::fs::create_dir_all(dir.join("skills")).unwrap();
    dir
}

#[test]
fn kit_in_reads_the_data_folder_then_home_and_skips_a_folder_without_skills() {
    let root = std::env::temp_dir().join(format!("bashcut-ta12-kit-in-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, home) = (root.join("data"), root.join("home"));
    // a folder without skills/ is not the kit
    std::fs::create_dir_all(data.join(kit::DATA_SUBDIR)).unwrap();
    std::fs::create_dir_all(home.join(kit::HOME_SUBDIR)).unwrap();
    assert!(!kit::is_kit(&data.join(kit::DATA_SUBDIR)));
    assert_eq!(kit::kit_in(Some(&data), Some(&home)), None);
    // home only
    let home_kit = make_kit(&home, kit::HOME_SUBDIR);
    assert_eq!(kit::kit_in(Some(&data), Some(&home)), Some(home_kit.clone()));
    assert_eq!(kit::kit_in(None, Some(&home)), Some(home_kit));
    // the data folder wins
    let data_kit = make_kit(&data, kit::DATA_SUBDIR);
    assert_eq!(kit::kit_in(Some(&data), Some(&home)), Some(data_kit.clone()));
    assert_eq!(kit::kit_in(Some(&data), None), Some(data_kit));
    assert_eq!(kit::kit_in(None, None), None);
    let _ = std::fs::remove_dir_all(&root);
}

/// [`kit_dir_follows_the_environment_then_the_default_places`]'s child: `KIT_TEST_EXPECT` is the answer
/// (empty: none) for the environment the parent gave this process.
#[test]
fn kit_dir_child() {
    let Some(expect) = std::env::var_os("KIT_TEST_EXPECT") else { return };
    let expect = Some(PathBuf::from(expect)).filter(|p| !p.as_os_str().is_empty());
    assert_eq!(kit::kit_dir(), expect);
}

/// The lookup order of [`kit::kit_dir`]: `$BASHCUT_KIT`, `$BASHCUTPRO_KIT_DIR`, `<data dir>/bashcut-kit`, `~/.bashcutpro/bashcutpro-agent-kit`;
/// a variable naming a folder without `skills/` (or empty) is skipped. The environment is the process's, and `unsafe` is
/// forbidden, so each case runs in a child test process.
#[test]
fn kit_dir_follows_the_environment_then_the_default_places() {
    let root = std::env::temp_dir().join(format!("bashcut-ta12-kit-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, home) = (root.join("data"), root.join("home"));
    let bare = root.join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    let (a, b) = (make_kit(&root, "a"), make_kit(&root, "b"));
    let run = |env: &[(&str, &Path)], expect: Option<&Path>| {
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "tests::kit_dir_child", "--test-threads=1"])
            .env_remove("BASHCUT_KIT")
            .env_remove("BASHCUTPRO_KIT_DIR")
            .env_remove("USERPROFILE")
            .env("HOME", &home)
            .env("BASHCUTPRO_DATA_DIR", &data)
            .env("KIT_TEST_EXPECT", expect.unwrap_or(Path::new("")));
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && log.contains("1 passed"), "{env:?} -> {expect:?}:\n{log}");
    };
    run(&[], None);
    let home_kit = make_kit(&home, kit::HOME_SUBDIR);
    run(&[], Some(&home_kit));
    let data_kit = make_kit(&data, kit::DATA_SUBDIR);
    run(&[], Some(&data_kit));
    run(&[("BASHCUTPRO_KIT_DIR", &b)], Some(&b));
    run(&[("BASHCUT_KIT", &a), ("BASHCUTPRO_KIT_DIR", &b)], Some(&a));
    // a variable that names no kit, or an empty one, is skipped
    run(&[("BASHCUT_KIT", &bare), ("BASHCUTPRO_KIT_DIR", &b)], Some(&b));
    run(&[("BASHCUT_KIT", &bare), ("BASHCUTPRO_KIT_DIR", &bare)], Some(&data_kit));
    run(&[("BASHCUT_KIT", Path::new(""))], Some(&data_kit));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "test ticket: kit_command_gaps lists missing ids per skill, None for a malformed/huge/no-skills index, skips plugin ids (bashcut.vision) and core ids"]
fn kit_command_gaps_names_the_skills_with_missing_ids() {}
