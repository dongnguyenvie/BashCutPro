use super::*;
use std::collections::HashMap;

fn host<'a>(
    vars: &'a HashMap<&'static str, String>,
    os: &'a str,
    exe_dir: Option<PathBuf>,
) -> (Box<dyn Fn(&str) -> Option<String> + 'a>, &'a str, Option<PathBuf>) {
    (Box::new(move |k: &str| vars.get(k).cloned()), os, exe_dir)
}

fn dirs(vars: &[(&'static str, &str)], os: &str, exe_dir: Option<PathBuf>) -> (Option<PathBuf>, Option<PathBuf>) {
    let vars: HashMap<&'static str, String> = vars.iter().map(|(k, v)| (*k, v.to_string())).collect();
    let (var, os, exe_dir) = host(&vars, os, exe_dir);
    let h = Host { var: &*var, os, exe_dir };
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
