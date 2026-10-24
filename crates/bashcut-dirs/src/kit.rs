//! The one agent-kit lookup (AG15): the bashcutpro-agent-kit folder that batch agent steps and the in-app
//! Claude Code agent pass to Claude Code as `--plugin-dir`, so the `bcp` skills load only in sessions
//! BashCutPro starts and no global `claude plugin install` is needed.

use std::path::{Path, PathBuf};

/// The folder under the data folder where the kit is kept (`<data dir>/bashcut-kit`).
pub const DATA_SUBDIR: &str = "bashcut-kit";

/// Where the first-run setup clones the kit, under the user's home folder.
pub const HOME_SUBDIR: &str = ".bashcutpro/bashcutpro-agent-kit";

/// `dir` holds the kit (it has `skills/`).
pub fn is_kit(dir: &Path) -> bool {
    dir.join("skills").is_dir()
}

/// The kit in the default places: `<data dir>/bashcut-kit`, then `<home>/.bashcutpro/bashcutpro-agent-kit`.
pub fn kit_in(data_dir: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    data_dir.map(|d| d.join(DATA_SUBDIR)).into_iter().chain(home.map(|h| h.join(HOME_SUBDIR))).find(|k| is_kit(k))
}

/// The kit for this process: `$BASHCUT_KIT`, `$BASHCUTPRO_KIT_DIR` (when they hold a kit), else the default places
/// under [`crate::data_dir`] and the home folder.
pub fn kit_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(k) = ["BASHCUT_KIT", "BASHCUTPRO_KIT_DIR"].into_iter().filter_map(env).find(|k| is_kit(k)) {
        return Some(k);
    }
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    kit_in(crate::data_dir().as_deref(), home.as_deref())
}
