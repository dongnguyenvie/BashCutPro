//! BashCutPro: the previews folder's name (BR1).

use super::brand::{OLD_PREVIEWS_DIR, PREVIEWS_DIR, previews_temp_root, project_previews_dir};

fn temp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("bashcut-brand-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

#[test]
fn a_saved_project_gets_bashcutpro_previews() {
    let d = temp("new");
    let p = d.join("Film.fcproj");
    assert_eq!(project_previews_dir(&p.to_string_lossy()), d.join("BashCutPro Previews").join("Film"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_older_project_keeps_its_filmcraft_previews() {
    let d = temp("old");
    std::fs::create_dir_all(d.join(OLD_PREVIEWS_DIR).join("Film")).expect("old folder");
    let p = d.join("Film.fcproj");
    assert_eq!(project_previews_dir(&p.to_string_lossy()), d.join(OLD_PREVIEWS_DIR).join("Film"));
    // Another project in the same folder has no old previews: it gets our name.
    let q = d.join("Show.fcproj");
    assert_eq!(project_previews_dir(&q.to_string_lossy()), d.join(PREVIEWS_DIR).join("Show"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn unsaved_previews_live_under_our_name_in_temp() {
    let root = previews_temp_root();
    assert_eq!(root.file_name().and_then(|n| n.to_str()), Some(PREVIEWS_DIR));
}
