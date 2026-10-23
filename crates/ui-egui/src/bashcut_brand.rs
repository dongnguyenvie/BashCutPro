//! BashCutPro branding (fork-only file; see `bashcut/rebrand.md`).
//!
//! A modified FilmCraft must not show the ArtCraft marks or link to ArtCraft's community as its
//! own (`docs/brand/LICENSE-brand.txt`). Rather than edit upstream's English strings and every
//! `i18n/*.tsv` catalog, [`text`] replaces the few branded strings at lookup time, in every
//! language: `i18n::Language::tr` asks it first. The replacements are English only. The hook is off
//! in this crate's unit tests, so upstream's catalog-coverage tests keep checking upstream's strings;
//! the tests here check the replacements through `shown` (test-only).

/// The product name shown in the About dialog.
pub const NAME: &str = "BashCutPro";

/// The window's Wayland app id / X11 class (upstream: `ai.storyteller.filmcraft`), set by the hook in
/// `apps/filmcraft/src/main.rs` after upstream's. Same id as the Linux packages' desktop file, metainfo
/// and icons (`bashcut/packaging/generate.py`, SB4), so the desktop matches the window to our entry.
pub const APP_ID: &str = "com.birchapps.bashcutpro";

/// The licence and third-party notices, linked from About.
pub const NOTICE_URL: &str = "https://github.com/dongnguyenvie/BashCutPro/blob/main/NOTICE";

/// The label of the header's community button (upstream: "Discord").
pub const COMMUNITY_BUTTON: &str = "Community";

/// The header's community button label, [`COMMUNITY_BUTTON`] instead of upstream's (`"Discord"`).
/// Called by the hook in `header.rs`, which passes upstream's label.
pub fn community_button(_upstream: &str) -> &'static str {
    COMMUNITY_BUTTON
}

/// The window, Dock, taskbar and app-switcher icon (`bashcut/brand/app-icon/`), set by the hook in
/// `apps/filmcraft/src/main.rs` after upstream's. Same layout as upstream's `app_icon`: macOS gets the
/// tile inside Apple's transparent margin, Windows and Linux the full-bleed tile.
#[cfg(not(target_arch = "wasm32"))]
pub fn app_icon() -> egui::IconData {
    #[cfg(target_os = "macos")]
    let png: &[u8] = include_bytes!("../../../bashcut/brand/app-icon/bashcutpro-macos-512.png");
    #[cfg(not(target_os = "macos"))]
    let png: &[u8] = include_bytes!("../../../bashcut/brand/app-icon/hicolor/256x256/apps/ai.storyteller.filmcraft.png");
    eframe::icon_data::from_png_bytes(png).unwrap_or_default()
}

/// About ▸ About, drawn instead of upstream's tab (the hook in `panels/dialogs.rs` returns when this
/// returns `true`, which it always does): our wordmark, name and links, the NOTICE, and the same
/// automation ids as upstream (`about.discord`, `about.website`, `about.appPage`, `about.github`,
/// `about.reportIssue`) plus `about.notice`. Branded labels go through `tl!`, so [`text`] replaces them.
pub fn about_tab(app: &mut crate::FilmcraftApp, ui: &mut egui::Ui) -> bool {
    use crate::icons::{self, Icon};
    use crate::links;
    let t = app.tokens;
    ui.set_width(380.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
    crate::brand::paint_wordmark(ui, egui::pos2(r.min.x, r.center().y), 20.0, app.ui.dark);
    ui.add_space(6.0);
    ui.heading(NAME);
    ui.label(tlf!("Version {version}", version = env!("CARGO_PKG_VERSION")));
    ui.label(tl!("A clean-room, pure-Rust non-linear video editor. Part of the ArtCraft family."));
    ui.add_space(10.0);
    let mut link = |ui: &mut egui::Ui, id: &str, icon: Icon, label: &str, url: &str, primary: bool| {
        let size = egui::vec2(ui.available_width(), if primary { 36.0 } else { 28.0 });
        let (r, resp) = ui.allocate_exact_size(size, egui::Sense::click());
        let resp = resp.on_hover_text(url);
        app.auto.add(&format!("about.{id}"), r, label);
        let bg = if primary {
            if resp.hovered() { t.accent_hover } else { t.accent }
        } else if resp.hovered() {
            t.hover
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(r, 6.0, bg);
        let fg = if primary { egui::Color32::WHITE } else { t.text };
        let ir = egui::Rect::from_center_size(egui::pos2(r.min.x + 20.0, r.center().y), egui::vec2(18.0, 18.0));
        icons::paint(ui.painter(), ir, icon, fg);
        ui.painter().text(
            egui::pos2(r.min.x + 40.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            crate::theme::Tokens::ui(if primary { 14.0 } else { 13.0 }),
            fg,
        );
        if resp.clicked() {
            links::open(app, ui.ctx(), url);
        }
    };
    link(ui, "discord", Icon::Chat, tl!("Join the ArtCraft Discord"), links::DISCORD, true);
    ui.add_space(6.0);
    link(ui, "website", Icon::Globe, links::WEBSITE.trim_start_matches("https://"), links::WEBSITE, false);
    link(ui, "appPage", Icon::Globe, tl!("FilmCraft on getartcraft.com"), links::APP_PAGE, false);
    link(ui, "github", Icon::Code, tl!("Source code on GitHub"), links::GITHUB, false);
    link(ui, "reportIssue", Icon::Code, tl!("Report an issue"), links::ISSUES, false);
    link(ui, "notice", Icon::Code, "Licence and notices (NOTICE)", NOTICE_URL, false);
    ui.add_space(10.0);
    ui.label(egui::RichText::new(tl!("MIT OR Apache-2.0. Fonts: Inter, Noto Serif and JetBrains Mono (SIL OFL 1.1).")).small().color(t.text_dim));
    true
}

/// (upstream English string, what BashCutPro shows instead). Each replacement has a Vietnamese row in
/// `bashcut_ui/strings/vi.tsv`.
const OVERRIDES: &[(&str, &str)] = &[
    // Help menu and Home screen links (`links::ALL`; their URLs are in links.rs)
    ("Join the ArtCraft Discord…", "BashCutPro Discussions…"),
    ("ArtCraft Website", "BashCut Website"),
    ("FilmCraft on getartcraft.com", "BashCutPro Releases"),
    ("FilmCraft on GitHub", "BashCutPro on GitHub"),
    ("Join us on Discord", "Join the discussions"),
    // header community button and About
    ("Join the ArtCraft Discord (discord.gg/artcraft)", "Join the BashCutPro discussions on GitHub"),
    ("Join the ArtCraft Discord", "Join the BashCutPro discussions"),
    (
        "A clean-room, pure-Rust non-linear video editor. Part of the ArtCraft family.",
        "BashCutPro, based on FilmCraft by the ArtCraft team: a clean-room, pure-Rust non-linear video editor.",
    ),
    // app menu (macOS)
    ("About FilmCraft", "About BashCutPro"),
    ("Hide FilmCraft", "Hide BashCutPro"),
    ("Quit FilmCraft", "Quit BashCutPro"),
    // file dialogs and File ▸ Export (BR2; the .fcproj / .fcgt extensions stay)
    ("FilmCraft Project", "BashCutPro Project"),
    ("Selection as FilmCraft Project…", "Selection as BashCutPro Project…"),
    ("FilmCraft Graphics Template", "BashCutPro Graphics Template"),
    ("Saved as a FilmCraft graphics template (.fcgt) in your templates folder.", "Saved as a BashCutPro graphics template (.fcgt) in your templates folder."),
    // messages and notes that name the running app (BR2)
    ("FilmCraft hit an error", "BashCutPro hit an error"),
    ("FilmCraft was closed while “{name}” had unsaved changes.", "BashCutPro was closed while “{name}” had unsaved changes."),
    ("FilmCraft quit unexpectedly while “{name}” had unsaved changes.", "BashCutPro quit unexpectedly while “{name}” had unsaved changes."),
    (
        "Changes are copied in the background a moment after each edit. If FilmCraft quits unexpectedly, they are offered the next time it starts.",
        "Changes are copied in the background a moment after each edit. If BashCutPro quits unexpectedly, they are offered the next time it starts.",
    ),
    ("FilmCraft keeps the rest of the RAM free for other applications.", "BashCutPro keeps the rest of the RAM free for other applications."),
    (
        "The media cache holds render previews of unsaved projects and other files FilmCraft can recreate. Deleting them is always safe; FilmCraft rebuilds them as needed.",
        "The media cache holds render previews of unsaved projects and other files BashCutPro can recreate. Deleting them is always safe; BashCutPro rebuilds them as needed.",
    ),
    ("Not supported by FilmCraft's encoders yet", "Not supported by BashCutPro's encoders yet"),
    ("Not supported in FilmCraft yet", "Not supported in BashCutPro yet"),
    ("FilmCraft always composites in linear light", "BashCutPro always composites in linear light"),
    ("Shows the version, community links and credits in a FilmCraft window.", "Shows the version, community links and credits in a BashCutPro window."),
];

/// What BashCutPro shows instead of the upstream English string `english`, if it is branded: in Vietnamese when our
/// strings are (`bashcut_ui::strings`), else in English.
pub fn text(english: &str) -> Option<&'static str> {
    OVERRIDES.iter().find(|(from, _)| *from == english).map(|(_, to)| crate::bashcut_ui::strings::s(to))
}

/// What a build shows for `english` in `lang`: the branded text, else the translation (what
/// `Language::tr` returns outside tests).
#[cfg(test)]
fn shown(lang: crate::i18n::Language, english: &str) -> &str {
    text(english).unwrap_or_else(|| lang.tr(english))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn no_artcraft_mark_or_link_is_shown() {
        // only the plain-text "based on FilmCraft by the ArtCraft team" may name ArtCraft
        for &(_, to) in OVERRIDES {
            assert!(!to.contains("ArtCraft") || to.contains("based on FilmCraft by the ArtCraft team"), "{to}");
        }
        for lang in Language::ALL {
            for (id, label, url) in crate::links::ALL {
                let shown = shown(lang, label);
                assert!(!shown.contains("ArtCraft") && !shown.contains("getartcraft"), "{lang:?} {id}: {shown}");
                assert!(!url.contains("artcraft"), "{id}: {url}");
            }
            assert_eq!(shown(lang, "About FilmCraft"), "About BashCutPro");
        }
    }

    #[test]
    fn file_dialogs_and_messages_name_bashcutpro_in_both_languages() {
        assert_eq!(text("FilmCraft Project"), Some("BashCutPro Project"));
        assert_eq!(text("FilmCraft hit an error"), Some("BashCutPro hit an error"));
        crate::bashcut_ui::strings::set_language(Some("vi"));
        let vi = text("FilmCraft Project");
        crate::bashcut_ui::strings::set_language(None);
        assert_eq!(vi, Some("Dự án BashCutPro"));
    }

    #[test]
    fn every_replacement_has_a_vietnamese_row() {
        crate::bashcut_ui::strings::set_language(Some("vi"));
        let missing: Vec<&str> = OVERRIDES.iter().map(|(_, to)| *to).filter(|to| crate::bashcut_ui::strings::s(to) == *to).collect();
        crate::bashcut_ui::strings::set_language(None);
        assert!(missing.is_empty(), "no row in bashcut_ui/strings/vi.tsv: {missing:?}");
        for &(_, to) in OVERRIDES {
            assert!(!to.contains("FilmCraft") || to.contains("based on FilmCraft"), "{to}");
        }
    }

    #[test]
    fn header_button_is_ours() {
        assert_eq!(community_button("Discord"), COMMUNITY_BUTTON);
    }

    /// Every overridden string still exists upstream: an upstream rename would silently bring the
    /// ArtCraft text back, so this fails first.
    #[test]
    fn every_override_matches_an_upstream_string() {
        for &(from, _) in OVERRIDES {
            assert!(Language::Es.has(from), "no longer upstream: {from}");
        }
        // strings that aren't branded are left alone
        assert_eq!(shown(Language::En, "File"), "File");
        assert_eq!(shown(Language::Es, "File"), Language::Es.tr("File"));
        assert_ne!(shown(Language::Es, "File"), "File");
    }
}
