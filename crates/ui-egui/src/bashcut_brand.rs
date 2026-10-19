//! BashCutPro branding (fork-only file; see `bashcut/rebrand.md`).
//!
//! A modified FilmCraft must not show the ArtCraft marks or link to ArtCraft's community as its
//! own (`docs/brand/LICENSE-brand.txt`). Rather than edit upstream's English strings and every
//! `i18n/*.tsv` catalog, [`text`] replaces the few branded strings at lookup time, in every
//! language: `i18n::Language::tr` asks it first. The replacements are English only. The hook is off
//! in this crate's unit tests, so upstream's catalog-coverage tests keep checking upstream's strings;
//! the tests here check the replacements through [`shown`].

/// The product name shown in the About dialog.
pub const NAME: &str = "BashCutPro";

/// The licence and third-party notices, linked from About.
pub const NOTICE_URL: &str = "https://github.com/dongnguyenvie/BashCutPro/blob/main/NOTICE";

/// The label of the header's community button (upstream: "Discord").
pub const COMMUNITY_BUTTON: &str = "Community";

/// (upstream English string, what BashCutPro shows instead).
const OVERRIDES: [(&str, &str); 11] = [
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
];

/// What BashCutPro shows instead of the upstream English string `english`, if it is branded.
pub fn text(english: &str) -> Option<&'static str> {
    OVERRIDES.iter().find(|(from, _)| *from == english).map(|(_, to)| *to)
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
        for (_, to) in OVERRIDES {
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

    /// Every overridden string still exists upstream: an upstream rename would silently bring the
    /// ArtCraft text back, so this fails first.
    #[test]
    fn every_override_matches_an_upstream_string() {
        for (from, _) in OVERRIDES {
            assert!(Language::Es.has(from), "no longer upstream: {from}");
        }
        // strings that aren't branded are left alone
        assert_eq!(shown(Language::En, "File"), "File");
        assert_eq!(shown(Language::Es, "File"), Language::Es.tr("File"));
        assert_ne!(shown(Language::Es, "File"), "File");
    }
}
