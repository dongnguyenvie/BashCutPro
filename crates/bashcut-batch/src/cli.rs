//! Which engine binary to run (#154): the Pro name `bashcutpro` first (`apps/bashcut-cli`, a
//! wrapper around the engine), then upstream's `filmcraft-cli`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The engine's names, preferred first.
pub const NAMES: &[&str] = &["bashcutpro", "filmcraft-cli"];
/// The environment variables that name it, preferred first (`BASHCUT_FILMCRAFT_CLI` is the older
/// name batch and `bashcut-mcp` used).
pub const ENV: &[&str] = &["BASHCUTPRO_CLI", "BASHCUT_FILMCRAFT_CLI", "FILMCRAFT_CLI"];
/// The wrapper's names (`bcpro` is its short alias).
const WRAPPERS: &[&str] = &["bashcutpro", "bcpro"];

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// The first of [`NAMES`] in `dir`.
pub fn in_dir(dir: &Path) -> Option<PathBuf> {
    NAMES.iter().map(|n| dir.join(exe(n))).find(|p| p.is_file())
}

/// The first of [`NAMES`] next to this program.
pub fn beside_self() -> Option<PathBuf> {
    in_dir(std::env::current_exe().ok()?.parent()?)
}

/// The first of [`NAMES`] on `path` (a `PATH` value); every name is tried in every folder before
/// the next name.
pub fn on_path(path: Option<&OsStr>) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = path.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    NAMES.iter().find_map(|n| dirs.iter().map(|d| d.join(exe(n))).find(|p| p.is_file()))
}

/// The default `--cli`: [`ENV`], else next to this program, else on `PATH`, else `filmcraft-cli`.
pub fn default_cli() -> PathBuf {
    let env = ENV.iter().find_map(|k| std::env::var_os(k).filter(|v| !v.is_empty())).map(PathBuf::from);
    env.or_else(beside_self).or_else(|| on_path(std::env::var_os("PATH").as_deref())).unwrap_or_else(|| PathBuf::from(exe("filmcraft-cli")))
}

/// The program a supervised step runs for `cli`. On Windows the wrapper cannot replace itself
/// with the engine (it waits for it), so the memory watch would see the wrapper and a kill would
/// leave the engine running: the `filmcraft-cli` beside a `bashcutpro` / `bcpro` is run instead.
/// On Unix the wrapper `exec`s the engine, so `cli` is run as it is.
pub fn engine(cli: &Path) -> PathBuf {
    engine_for(cli, cfg!(windows), std::env::var_os("PATH").as_deref())
}

fn engine_for(cli: &Path, windows: bool, path: Option<&OsStr>) -> PathBuf {
    let wrapper = cli.file_stem().and_then(OsStr::to_str).is_some_and(|s| WRAPPERS.iter().any(|w| s.eq_ignore_ascii_case(w)));
    if !windows || !wrapper {
        return cli.to_path_buf();
    }
    let located = if cli.components().count() > 1 {
        Some(cli.to_path_buf())
    } else {
        path.into_iter().flat_map(std::env::split_paths).map(|d| d.join(cli)).find(|p| p.is_file() || p.with_extension("exe").is_file())
    };
    located.and_then(|p| p.parent().map(|d| d.join("filmcraft-cli.exe"))).filter(|p| p.is_file()).unwrap_or_else(|| cli.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bashcut-batch-cli-{name}-{}", std::process::id()));
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
        assert_eq!(in_dir(&both), Some(both.join(exe("bashcutpro"))));
        assert_eq!(in_dir(&old), Some(old.join(exe("filmcraft-cli"))));
        assert_eq!(in_dir(&none), None);
        // the Pro name anywhere on PATH wins over filmcraft-cli earlier on PATH
        let path = std::env::join_paths([&old, &both]).unwrap();
        assert_eq!(on_path(Some(&path)), Some(both.join(exe("bashcutpro"))));
        let path = std::env::join_paths([&none, &old]).unwrap();
        assert_eq!(on_path(Some(&path)), Some(old.join(exe("filmcraft-cli"))));
        assert_eq!(on_path(None), None);
    }

    #[test]
    fn windows_runs_the_engine_behind_the_wrapper() {
        let d = tmp("engine");
        std::fs::write(d.join("bashcutpro.exe"), "").unwrap();
        std::fs::write(d.join("filmcraft-cli.exe"), "").unwrap();
        let path = std::env::join_paths([&d]).unwrap();
        assert_eq!(engine_for(&d.join("bashcutpro.exe"), true, None), d.join("filmcraft-cli.exe"));
        assert_eq!(engine_for(Path::new("bashcutpro"), true, Some(&path)), d.join("filmcraft-cli.exe"));
        assert_eq!(engine_for(&d.join("bashcutpro.exe"), false, None), d.join("bashcutpro.exe"));
        assert_eq!(engine_for(Path::new("filmcraft-cli"), true, Some(&path)), PathBuf::from("filmcraft-cli"));
        // no engine beside it: the wrapper runs (and finds the engine on PATH itself)
        let lone = tmp("lone");
        assert_eq!(engine_for(&lone.join("bcpro.exe"), true, None), lone.join("bcpro.exe"));
    }
}
