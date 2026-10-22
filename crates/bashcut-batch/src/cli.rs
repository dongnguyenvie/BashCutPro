//! Which engine binary to run (#154): the Pro name `bashcutpro` first (`apps/bashcutpro-cli`,
//! upstream's engine built under our name, SB2), then upstream's `filmcraft-cli` (dev builds).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The engine's names, preferred first.
pub const NAMES: &[&str] = &["bashcutpro", "filmcraft-cli"];
/// The environment variables that name it, preferred first (`BASHCUT_FILMCRAFT_CLI` is the older
/// name batch and `bashcut-mcp` used).
pub const ENV: &[&str] = &["BASHCUTPRO_CLI", "BASHCUT_FILMCRAFT_CLI", "FILMCRAFT_CLI"];
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

/// The program a supervised step runs for `cli`: `cli` itself, since `bashcutpro` / `bcpro` are
/// the engine (no wrapper since SB2), so the memory watch and a kill reach the real process.
pub fn engine(cli: &Path) -> PathBuf {
    cli.to_path_buf()
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
}
