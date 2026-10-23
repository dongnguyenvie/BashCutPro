//! Which engine binary to run (#154, NM1): the Pro name `bashcutpro-cli` first (`apps/bashcutpro-cli`,
//! upstream's engine built under our name, SB2), then its alias `bcpro`, then upstream's `filmcraft-cli`
//! (dev builds). Never plain `bashcutpro`: that is the GUI on Linux (`/usr/bin/bashcutpro`).
//! The one lookup for every consumer (`bashcut-batch`, the UI; RV2), so a new name or variable is one edit here.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The engine's names, preferred first: the CLI `bashcutpro-cli`, its alias `bcpro`, upstream's
/// `filmcraft-cli` (the naming contract follows upstream's `filmcraft` / `filmcraft-cli`, NM1).
pub const NAMES: &[&str] = &["bashcutpro-cli", "bcpro", "filmcraft-cli"];
/// The name tried when nothing names or holds an engine (looked up on `PATH` when it runs).
pub const FALLBACK: &str = "bashcutpro-cli";
/// The environment variables that name it, preferred first (`BASHCUT_FILMCRAFT_CLI` is the older
/// name batch and `bashcut-mcp` used).
pub const ENV: &[&str] = &["BASHCUTPRO_CLI", "BASHCUT_FILMCRAFT_CLI", "FILMCRAFT_CLI"];
fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// `dir/name` when a file of exactly that name is there. On a case-insensitive volume (macOS) a name
/// opens a file spelled differently (the old engine name `bashcutpro` opened the app's own `BashCutPro`
/// executable, #187), so the listing must carry the name as written; a folder that cannot be listed falls back to the plain check. Windows
/// names are case-insensitive by design and keep the plain check.
fn file_named(dir: &Path, name: &str) -> Option<PathBuf> {
    let p = dir.join(name);
    if !p.is_file() {
        return None;
    }
    if cfg!(windows) {
        return Some(p);
    }
    let listed = std::fs::read_dir(dir).map_or(true, |entries| entries.flatten().any(|e| e.file_name() == OsStr::new(name)));
    listed.then_some(p)
}

/// The first of [`NAMES`] in `dir`.
pub fn in_dir(dir: &Path) -> Option<PathBuf> {
    NAMES.iter().find_map(|n| file_named(dir, &exe(n)))
}

/// The first of [`NAMES`] next to this program.
pub fn beside_self() -> Option<PathBuf> {
    in_dir(std::env::current_exe().ok()?.parent()?)
}

/// The first of [`NAMES`] on `path` (a `PATH` value); every name is tried in every folder before
/// the next name.
pub fn on_path(path: Option<&OsStr>) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = path.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    NAMES.iter().find_map(|n| dirs.iter().find_map(|d| file_named(d, &exe(n))))
}

/// The installed engine without the variables: in `exe_dir` (this program's folder), else on `path`.
/// The seam for callers with fakes (the doctor's `Env`, RCB); [`installed`] asks the real process.
pub fn installed_in(exe_dir: Option<&Path>, path: Option<&OsStr>) -> Option<PathBuf> {
    exe_dir.and_then(in_dir).or_else(|| on_path(path))
}

/// [`installed_in`] for this process: next to this program, else on `PATH`.
pub fn installed() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    installed_in(exe_dir.as_deref(), std::env::var_os("PATH").as_deref())
}

/// The whole lookup with injected `var` (environment), `exe_dir` and `path`: [`ENV`], else
/// [`installed_in`]. `None` when nothing names or holds an engine.
pub fn lookup(var: &dyn Fn(&str) -> Option<OsString>, exe_dir: Option<&Path>, path: Option<&OsStr>) -> Option<PathBuf> {
    let env = ENV.iter().find_map(|k| var(k).filter(|v| !v.is_empty())).map(PathBuf::from);
    env.or_else(|| installed_in(exe_dir, path))
}

/// The default `--cli`: [`ENV`], else next to this program, else on `PATH`, else [`FALLBACK`].
pub fn default_cli() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    lookup(&|k| std::env::var_os(k), exe_dir.as_deref(), std::env::var_os("PATH").as_deref()).unwrap_or_else(|| PathBuf::from(exe(FALLBACK)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bashcut-dirs-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn pro_name_first_then_filmcraft_cli() {
        let (both, old, none) = (tmp("both"), tmp("old"), tmp("none"));
        for n in NAMES {
            std::fs::write(both.join(exe(n)), "").unwrap();
        }
        std::fs::write(old.join(exe("filmcraft-cli")), "").unwrap();
        assert_eq!(in_dir(&both), Some(both.join(exe("bashcutpro-cli"))));
        assert_eq!(in_dir(&old), Some(old.join(exe("filmcraft-cli"))));
        assert_eq!(in_dir(&none), None);
        // the Linux GUI `/usr/bin/bashcutpro` is not the engine (NM1)
        let gui = tmp("gui");
        std::fs::write(gui.join(exe("bashcutpro")), "").unwrap();
        assert_eq!(in_dir(&gui), None);
        let _ = std::fs::remove_dir_all(&gui);
        // the Pro name anywhere on PATH wins over filmcraft-cli earlier on PATH
        let path = std::env::join_paths([&old, &both]).unwrap();
        assert_eq!(on_path(Some(&path)), Some(both.join(exe("bashcutpro-cli"))));
        let path = std::env::join_paths([&none, &old]).unwrap();
        assert_eq!(on_path(Some(&path)), Some(old.join(exe("filmcraft-cli"))));
        assert_eq!(on_path(None), None);
    }

    // not on Windows: there the two names are the same file by design (the plain check, `file_named`)
    #[cfg(not(windows))]
    #[test]
    fn a_name_that_differs_only_in_case_is_not_the_engine() {
        // the macOS app bundle: the app's `BashCutPro` executable next to `bcpro` (#187); on a
        // case-insensitive volume a lookup of `bashcutpro` would open `BashCutPro`
        let bundle = tmp("bundle");
        std::fs::write(bundle.join(exe("BashCutPro")), "").unwrap();
        std::fs::write(bundle.join(exe("bcpro")), "").unwrap();
        assert_eq!(in_dir(&bundle), Some(bundle.join(exe("bcpro"))));
        let path = std::env::join_paths([&bundle]).unwrap();
        assert_eq!(on_path(Some(&path)), Some(bundle.join(exe("bcpro"))));
        let _ = std::fs::remove_dir_all(&bundle);
    }

    #[test]
    fn lookup_with_injected_env_dir_and_path() {
        let (exe_dir, on) = (tmp("exe"), tmp("path"));
        std::fs::write(exe_dir.join(exe("bcpro")), "").unwrap();
        std::fs::write(on.join(exe("bashcutpro-cli")), "").unwrap();
        let path = std::env::join_paths([&on]).unwrap();
        let env = |vars: &'static [(&'static str, &'static str)]| move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v));
        // ENV in order: BASHCUTPRO_CLI over BASHCUT_FILMCRAFT_CLI over FILMCRAFT_CLI
        let all = env(&[("FILMCRAFT_CLI", "/c"), ("BASHCUT_FILMCRAFT_CLI", "/b"), ("BASHCUTPRO_CLI", "/a")]);
        assert_eq!(lookup(&all, Some(&exe_dir), Some(&path)), Some(PathBuf::from("/a")));
        let two = env(&[("FILMCRAFT_CLI", "/c"), ("BASHCUT_FILMCRAFT_CLI", "/b")]);
        assert_eq!(lookup(&two, Some(&exe_dir), Some(&path)), Some(PathBuf::from("/b")));
        let one = env(&[("FILMCRAFT_CLI", "/c")]);
        assert_eq!(lookup(&one, Some(&exe_dir), Some(&path)), Some(PathBuf::from("/c")));
        // an empty value counts as unset
        let empty = env(&[("BASHCUTPRO_CLI", ""), ("BASHCUT_FILMCRAFT_CLI", ""), ("FILMCRAFT_CLI", "/c")]);
        assert_eq!(lookup(&empty, Some(&exe_dir), Some(&path)), Some(PathBuf::from("/c")));
        // no variable: this program's folder, then PATH, then nothing
        let none = env(&[("BASHCUTPRO_CLI", "")]);
        assert_eq!(lookup(&none, Some(&exe_dir), Some(&path)), Some(exe_dir.join(exe("bcpro"))));
        assert_eq!(lookup(&none, None, Some(&path)), Some(on.join(exe("bashcutpro-cli"))));
        assert_eq!(lookup(&none, Some(&tmp("empty")), None), None);
        assert_eq!(installed_in(None, None), None);
        for d in ["exe", "path", "empty"] {
            let _ = std::fs::remove_dir_all(tmp(d));
        }
    }
}
