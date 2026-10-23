//! BashCutPro: our names for folders the engine creates on disk (BR1). Reached from `previews.rs` by two one-line
//! hooks ([`previews_temp_first!`], [`previews_dir_first!`]; `UPSTREAM.md` "Hooks"). The engine's own unit tests
//! keep upstream's names, as with `bashcut_dirs::data_dir_first!`.

use std::path::{Path, PathBuf};

/// The render previews folder next to a saved project (`/path/Film.fcproj` → `/path/BashCutPro Previews/Film/`) and
/// in the temp dir for unsaved projects.
pub const PREVIEWS_DIR: &str = "BashCutPro Previews";
/// Upstream's name, written by FilmCraft and by BashCutPro before BR1: still used when a project already has one.
pub const OLD_PREVIEWS_DIR: &str = "FilmCraft Previews";

/// The preview folder of a project saved at `project_path`: `BashCutPro Previews/<name>` next to it, or the
/// existing `FilmCraft Previews/<name>` of an older project so its rendered previews are kept.
pub fn project_previews_dir(project_path: &str) -> PathBuf {
    let p = Path::new(project_path);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
    let parent = p.parent().unwrap_or(Path::new("."));
    let old = parent.join(OLD_PREVIEWS_DIR).join(&stem);
    if old.is_dir() { old } else { parent.join(PREVIEWS_DIR).join(stem) }
}

/// The root that holds unsaved projects' preview folders.
pub fn previews_temp_root() -> PathBuf {
    crate::temp_dir().join(PREVIEWS_DIR)
}

/// The hook in `previews::default_temp_dir` (after its wasm check): unsaved previews under [`previews_temp_root`].
/// `$untitled` is upstream's per-process folder fn. A macro so the hook stays one line.
macro_rules! previews_temp_first {
    ($untitled:ident) => {
        if cfg!(not(test)) {
            return Some($untitled(&crate::bashcut::brand::previews_temp_root()));
        }
    };
}
pub(crate) use previews_temp_first;

/// The hook in `previews::dir_for_project`: returns [`project_previews_dir`] early. A macro so the hook stays one line.
macro_rules! previews_dir_first {
    ($project_path:expr) => {
        if cfg!(not(test)) {
            return crate::bashcut::brand::project_previews_dir($project_path);
        }
    };
}
pub(crate) use previews_dir_first;
