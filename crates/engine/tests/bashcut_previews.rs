//! BashCutPro (BR1): the previews hook in `previews.rs` is live outside upstream's unit tests, so a saved project's
//! previews go to `BashCutPro Previews`, and an older project keeps its `FilmCraft Previews` folder.

use filmcraft_engine::previews::dir_for_project;

fn temp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("bashcut-previews-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

#[test]
fn a_saved_project_gets_bashcutpro_previews_next_to_it() {
    let d = temp("new");
    let p = d.join("Film.fcproj");
    assert_eq!(dir_for_project(&p.to_string_lossy()), d.join("BashCutPro Previews").join("Film"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_older_project_keeps_its_filmcraft_previews_folder() {
    let d = temp("old");
    std::fs::create_dir_all(d.join("FilmCraft Previews").join("Film")).expect("old folder");
    let p = d.join("Film.fcproj");
    assert_eq!(dir_for_project(&p.to_string_lossy()), d.join("FilmCraft Previews").join("Film"));
    let _ = std::fs::remove_dir_all(&d);
}
