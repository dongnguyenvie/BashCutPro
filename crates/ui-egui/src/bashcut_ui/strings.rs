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

/// The language picked in our setting this frame (`None`: following the interface language).
pub fn chosen() -> Option<&'static str> {
    OVERRIDE.with(Cell::get)
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

    /// Our sources, without their test modules, as (file name, code).
    fn sources() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bashcut_ui");
        let mut v: Vec<(String, String)> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "rs") && p.file_name().is_some_and(|n| n != "strings.rs"))
            .map(|p| {
                let text = std::fs::read_to_string(&p).unwrap();
                let code = text.split("#[cfg(test)]").next().unwrap_or_default().to_string();
                (p.file_name().unwrap().to_string_lossy().into_owned(), code)
            })
            .collect();
        v.sort();
        assert!(v.len() > 10, "no sources under {}", dir.display());
        v
    }

    /// The plain string literal at the start of `src` (after whitespace), unescaped; `None` if
    /// `src` starts with anything else.
    fn literal(src: &str) -> Option<String> {
        let mut chars = src.trim_start().strip_prefix('"')?.chars().peekable();
        let mut out = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(out),
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    // a line continuation also drops the next line's leading whitespace
                    '\n' | '\r' => {
                        while chars.peek().is_some_and(|c| c.is_whitespace()) {
                            chars.next();
                        }
                    }
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
        None
    }

    /// The English strings the `s("…")` / `f("…", …)` calls in `code` look up.
    fn looked_up(code: &str) -> Vec<String> {
        let mut v = Vec::new();
        for call in ["s(", "f("] {
            for (i, _) in code.match_indices(call) {
                if code[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                    continue;
                }
                v.extend(literal(&code[i + call.len()..]));
            }
        }
        v
    }

    /// The `=> "…"` strings in the body of `fn name` in `code`.
    fn match_arms(code: &str, name: &str) -> Vec<String> {
        let Some(start) = code.find(&format!("fn {name}(")) else { return Vec::new() };
        let body = &code[start..];
        let body = &body[..body.find("\n}").unwrap_or(body.len())];
        body.match_indices("=>").filter_map(|(i, _)| literal(&body[i + 2..])).collect()
    }

    /// The `(file, string)` pairs with no Vietnamese row, sorted, one per line.
    fn untranslated(keys: Vec<(String, String)>) -> String {
        let vi = catalog("vi").unwrap();
        let mut v: Vec<String> = keys.into_iter().filter(|(_, k)| !vi.contains_key(k.as_str())).map(|(file, k)| format!("{file}: {k:?}")).collect();
        v.sort();
        v.dedup();
        v.join("\n")
    }

    #[test]
    fn every_status_word_has_a_vietnamese_row() {
        let mut words = Vec::new();
        for (file, code) in sources() {
            for name in ["status_label", "state_label"] {
                words.extend(match_arms(&code, name).into_iter().map(|w| (file.clone(), w)));
            }
        }
        // both panels' lists were found
        assert!(words.iter().any(|(_, w)| w == "Needs fix") && words.iter().any(|(_, w)| w == "Needs the agent"), "{words:?}");
        let gaps = untranslated(words);
        assert!(gaps.is_empty(), "status words with no row in strings/vi.tsv:\n{gaps}");
    }

    #[test]
    fn every_looked_up_string_has_a_vietnamese_row() {
        let mut keys = Vec::new();
        // not lookups: a local `s` closure reading a param
        let not_strings = [("plugins.rs", "plugin"), ("plugins.rs", "action")];
        for (file, code) in sources() {
            keys.extend(looked_up(&code).into_iter().filter(|k| !not_strings.contains(&(file.as_str(), k.as_str()))).map(|k| (file.clone(), k)));
        }
        for sec in super::super::SECTIONS {
            keys.push(("mod.rs".into(), sec.title.into()));
            keys.push(("mod.rs".into(), sec.blurb.into()));
        }
        // Window ▸ BashCutPro: our labels and submenus (upstream translates "Window")
        for c in super::super::commands().into_iter().filter(|c| !c.menu.is_empty()) {
            keys.extend(c.menu.into_iter().filter(|p| p != "Window").map(|p| ("menu".to_string(), p)));
            keys.push(("menu".into(), c.label));
        }
        assert!(keys.len() > 100, "the scan found only {} strings", keys.len());
        let gaps = untranslated(keys);
        assert!(gaps.is_empty(), "strings with no row in strings/vi.tsv:\n{gaps}");
    }

    #[test]
    fn literal_reads_escapes_and_continuations() {
        assert_eq!(literal(r#" "a \"b\" c", x"#).as_deref(), Some(r#"a "b" c"#));
        assert_eq!(literal("\"one \\\n        two\"").as_deref(), Some("one two"));
        assert_eq!(literal("name)"), None);
        assert_eq!(looked_up(r#"s("Hi") + f("{n} files", &[]) + is("no") + x.s("no")"#), ["Hi", "{n} files"]);
    }
}
