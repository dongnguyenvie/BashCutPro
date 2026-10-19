//! Our UI strings (`bashcut/in-app-ui.md` §4).
//!
//! English lives in the code; translations live in `strings/<lang>.tsv` (`English<TAB>translation`
//! per line, `#` comments), never in upstream's catalogs. Vietnamese is not one of upstream's
//! interface languages, so our own setting (`State::language`, `bashcut.ui.setLanguage`) can pick
//! it; otherwise the interface language is used.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

type Catalog = HashMap<&'static str, &'static str>;

static VIETNAMESE: OnceLock<Catalog> = OnceLock::new();

thread_local! {
    /// Our language override for this frame (set by `layout` from the state).
    static OVERRIDE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Languages we ship a catalog for (codes as in `State::language`).
pub const LANGUAGES: [&str; 2] = ["en", "vi"];

/// Use `code` for our strings (`None` follows the interface language). Unknown codes fall back.
pub fn set_language(code: Option<&str>) {
    let code = code.and_then(|c| LANGUAGES.iter().find(|l| **l == c).copied());
    OVERRIDE.with(|o| o.set(code));
}

fn parse(src: &'static str) -> Catalog {
    src.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).filter_map(|l| l.split_once('\t')).filter(|(_, to)| !to.trim().is_empty()).collect()
}

fn catalog(code: &str) -> Option<&'static Catalog> {
    match code {
        "vi" => Some(VIETNAMESE.get_or_init(|| parse(include_str!("strings/vi.tsv")))),
        _ => None,
    }
}

/// The language our strings are drawn in.
pub fn current() -> &'static str {
    OVERRIDE.with(Cell::get).unwrap_or_else(|| crate::i18n::current().code())
}

/// Translate one of our English strings; unknown strings come back unchanged.
pub fn s(text: &str) -> &str {
    catalog(current()).and_then(|c| c.get(text).copied()).unwrap_or(text)
}

/// Like [`s`], filling `{name}` placeholders.
pub fn f(text: &str, args: &[(&str, &str)]) -> String {
    crate::i18n::fmt(s(text), args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vietnamese_catalog_translates_every_section_title() {
        let vi = catalog("vi").unwrap();
        for sec in super::super::SECTIONS {
            assert!(vi.contains_key(sec.title), "no Vietnamese for {}", sec.title);
        }
        set_language(Some("vi"));
        assert_eq!(s("Plugins"), "Plugin");
        assert_eq!(s("not a known string"), "not a known string");
        set_language(Some("xx"));
        assert_eq!(s("Plugins"), "Plugins");
        set_language(None);
    }
}
