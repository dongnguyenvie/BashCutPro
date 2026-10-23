//! The crash log names BashCutPro (BR3; carried edit in `crash.rs`, `UPSTREAM.md`). Its own test
//! binary: `crash::install` sets a process-wide log dir, which upstream's `crash.rs` test also sets.
//! Fork-only file.

/// CK1: a recorded fatal error is written under a header `=== <crate> BashCutPro <version>`,
/// never FilmCraft's name.
#[test]
fn crash_log_header_names_bashcutpro() {
    let dir = std::env::temp_dir().join(format!("bc-ck1-crash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    filmcraft_ui_egui::crash::install(Some(dir.clone()));
    filmcraft_ui_egui::crash::record("ck1: no adapter");
    let path = filmcraft_ui_egui::crash::log_path().expect("a log dir is set");
    let log = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    let header = log.lines().find(|l| l.starts_with("=== ")).unwrap_or_default();
    assert!(header.contains(&format!(" BashCutPro {} ", env!("CARGO_PKG_VERSION"))), "{log}");
    assert!(!header.contains("FilmCraft"), "{header}");
    assert!(log.contains("ck1: no adapter"), "{log}");
}
