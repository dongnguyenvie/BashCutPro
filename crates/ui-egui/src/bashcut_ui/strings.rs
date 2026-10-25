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

/// The bashcut-agent crate's messages that carry a value (VI4): the crate builds them in English
/// with `format!`, so the panel matches them back to these templates. Longer forms first; the
/// fixed ones are plain catalog rows. `{error}` is translated again when it is one of ours.
pub const AGENT_MESSAGES: [&str; 19] = [
    "the agent exited with status {code}: {output}",
    "the agent exited with status {code}",
    "the agent was killed: {output}",
    "cannot start the agent thread: {error}",
    "cannot start `{program}`: {error}",
    "agent `{id}` has no command",
    "agent `{id}`: {error}",
    "no agent `{id}`",
    "event format `{name}` is not supported (use {formats})",
    "event format `{name}` has no live session",
    "unknown command bashcut.agent.{name}",
    "bad conversation id `{id}`",
    "conversation {id}: not a {schema} file",
    "conversation {id}: {error}",
    "plan.json: {error}",
    "{path}: no {server} server",
    "{path}: no command",
    // AGM2: the live model switch's notices
    "Model: {model}",
    "Model not changed: {error}",
];

/// A bashcut-agent error or notice in our language: a catalog row, else an [`AGENT_MESSAGES`]
/// template it matches, else unchanged (the agent's own words stay as it wrote them).
pub fn agent(text: &str) -> String {
    agent_at(text, 0)
}

fn agent_at(text: &str, depth: u8) -> String {
    let own = s(text);
    if own != text {
        return own.to_string();
    }
    for template in AGENT_MESSAGES {
        let Some(mut args) = captures(template, text) else { continue };
        if depth < 2 {
            for (name, value) in &mut args {
                if *name == "error" {
                    *value = agent_at(value, depth + 1);
                }
            }
        }
        let args: Vec<(&str, &str)> = args.iter().map(|(n, v)| (*n, v.as_str())).collect();
        return f(template, &args);
    }
    text.to_string()
}

/// The `{name}` values that make `template` read `text`; `None` when it does not match. A value is
/// never empty; a name followed by more text takes the shortest value that fits.
fn captures<'t>(template: &'t str, text: &str) -> Option<Vec<(&'t str, String)>> {
    let (head, mut tail) = template.split_once('{').unwrap_or((template, ""));
    let mut rest = text.strip_prefix(head)?;
    if tail.is_empty() {
        return rest.is_empty().then(Vec::new);
    }
    let mut out = Vec::new();
    loop {
        let (name, after) = tail.split_once('}')?;
        let (literal, next) = after.split_once('{').unwrap_or((after, ""));
        let value = if next.is_empty() {
            // the last value runs to the template's closing text
            let v = rest.strip_suffix(literal)?;
            rest = "";
            v
        } else {
            let first = rest.chars().next()?.len_utf8();
            let at = rest.get(first..)?.find(literal)? + first;
            let v = rest.get(..at)?;
            rest = rest.get(at + literal.len()..)?;
            v
        };
        if value.is_empty() {
            return None;
        }
        out.push((name, value.to_string()));
        if next.is_empty() {
            return Some(out);
        }
        tail = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VI6: every bashcut.* command label, and every menu path part but upstream's own Window menu, has a Vietnamese row.
    #[test]
    fn vietnamese_catalog_translates_every_command_label() {
        let vi = catalog("vi").unwrap();
        let mut missing = Vec::new();
        for c in super::super::commands() {
            if !c.id.starts_with("bashcut.") {
                continue;
            }
            for text in std::iter::once(c.label.as_str()).chain(c.menu.iter().map(|m| m.as_str()).filter(|m| *m != "Window")) {
                if !vi.contains_key(text) && !missing.contains(&text.to_string()) {
                    missing.push(text.to_string());
                }
            }
        }
        assert!(missing.is_empty(), "no Vietnamese row for: {missing:#?}");
    }

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
        let mut files = Vec::new();
        let mut dirs = vec![dir.clone()];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    dirs.push(p);
                } else {
                    files.push(p);
                }
            }
        }
        let mut v: Vec<(String, String)> = files
            .into_iter()
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
        // looked up through a variable, which the scan cannot see: list them here
        for (_, label) in super::super::library_panel::KINDS {
            keys.push(("library_panel.rs".into(), label.into()));
        }
        for (_, label) in super::super::library_panel::MINE_KINDS {
            keys.push(("library_panel.rs".into(), label.into()));
        }
        for (_, title, purpose) in super::super::palette::RECIPES {
            keys.push(("palette.rs".into(), title.to_string()));
            keys.push(("palette.rs".into(), purpose.to_string()));
        }
        for k in super::super::palette::Kind::ALL {
            keys.push(("palette.rs".into(), k.label().into()));
        }
        // single strings kept in a const
        keys.push(("agent_panel.rs".into(), super::super::agent_panel::OUTDATED_LINE.into()));
        keys.push(("control.rs".into(), super::super::control::ALLOW.into()));
        keys.push(("control.rs".into(), super::super::control::RESTART.into()));
        keys.push(("hints.rs".into(), super::super::hints::CONTROL_OFF.into()));
        keys.push(("hints.rs".into(), super::super::hints::HOW_TO_TURN_ON.into()));
        for template in AGENT_MESSAGES {
            keys.push(("strings.rs".into(), template.into()));
        }
        for line in super::super::agent_panel::QUIET_LINES {
            keys.push(("agent_panel.rs".into(), line.into()));
        }
        assert!(keys.len() > 100, "the scan found only {} strings", keys.len());
        let gaps = untranslated(keys);
        assert!(gaps.is_empty(), "strings with no row in strings/vi.tsv:\n{gaps}");
    }

    /// The English keys `src` (a catalog file) has more than once, byte for byte.
    fn duplicates(src: &str) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        let mut v: Vec<&str> = src
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.split_once('\t').map(|(k, _)| k))
            .filter(|k| !seen.insert(*k))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    #[test]
    fn vietnamese_catalog_has_no_duplicate_keys() {
        // a later row silently replaces an earlier one in the catalog
        let dups = duplicates(include_str!("strings/vi.tsv"));
        assert!(dups.is_empty(), "keys listed more than once in strings/vi.tsv: {dups:?}");
        assert_eq!(duplicates("# c\nSave\tLưu\nSave…\tLưu…\nSave\tGhi\n"), ["Save"]);
        // TA4: a key that starts another one is not a duplicate (a prefix grep reads
        // "Command Palette" / "Command Palette…", "Running" / "Running…", "Save" / "Save settings" as twice)
        assert!(duplicates("Command Palette\tBảng lệnh\nCommand Palette…\tBảng lệnh…\nRunning\tĐang chạy\nRunning…\tĐang chạy…\n").is_empty());
    }

    #[test]
    fn agent_messages_translate_through_templates() {
        set_language(Some("vi"));
        // a fixed row
        assert_eq!(agent("stop the running turn first"), "Hãy dừng lượt đang chạy trước");
        // a template with values, the longer form before the shorter one
        assert_eq!(agent("the agent exited with status 1: lỗi: không đọc được tệp"), "Trợ lý thoát với mã 1: lỗi: không đọc được tệp");
        assert_eq!(agent("the agent exited with status 137"), "Trợ lý thoát với mã 137");
        // multi-byte values, also next to the template's own text
        assert_eq!(agent("no agent `trợ-lý-ệ`"), "Không có trợ lý `trợ-lý-ệ`");
        assert_eq!(agent("cannot start `C:\\Phim của tôi\\claude.exe`: không tìm thấy"), "Không chạy được `C:\\Phim của tôi\\claude.exe`: không tìm thấy");
        // a nested {error} that is one of ours is translated too, twice deep at most
        assert_eq!(agent("agent `x`: no agent `y`"), "Trợ lý `x`: Không có trợ lý `y`");
        assert_eq!(agent("cannot start the agent thread: the agent has ended"), "Không khởi động được luồng trợ lý: Trợ lý đã thoát");
        assert_eq!(agent("agent `a`: agent `b`: agent `c`: no agent `d`"), "Trợ lý `a`: Trợ lý `b`: Trợ lý `c`: no agent `d`");
        // VI5: the session, store and mcp-config messages
        assert_eq!(agent("unknown command bashcut.agent.dance"), "Không có lệnh bashcut.agent.dance");
        assert_eq!(agent("bad conversation id `../x`"), "Mã cuộc trò chuyện `../x` không hợp lệ");
        assert_eq!(agent("conversation c1-2: not a bashcut-conversation/1 file"), "Cuộc trò chuyện c1-2: không phải tệp bashcut-conversation/1");
        assert_eq!(agent("conversation c1-2: expected value at line 1"), "Cuộc trò chuyện c1-2: expected value at line 1");
        assert_eq!(agent("C:\\Dự án\\mcp.json: no bashcutpro server"), "C:\\Dự án\\mcp.json: không có máy chủ bashcutpro");
        assert_eq!(agent("/tmp/mcp.json: no command"), "/tmp/mcp.json: chưa có lệnh chạy");
        // the agent's own words, and a template with an empty value, stay as they are
        assert_eq!(agent("Here is your rough cut."), "Here is your rough cut.");
        assert_eq!(agent("no agent ``"), "no agent ``");
        assert_eq!(agent("the agent exited with status "), "the agent exited with status ");
        assert_eq!(agent(""), "");
        set_language(Some("en"));
        assert_eq!(agent("agent `x`: no agent `y`"), "agent `x`: no agent `y`");
        set_language(None);
        assert_eq!(captures("no agent `{id}`", "no agent `a`b`"), Some(vec![("id", "a`b".to_string())]));
        assert_eq!(captures("plan.json: {error}", "plan.json"), None);
    }

    /// The plain literals the bashcut-agent crate shows as an error or notice: `err("…")`,
    /// `Error("…"`, `Notice("…"` and `Notice { message: "…"`, outside its test modules.
    fn agent_crate_messages() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../bashcut-agent/src");
        let mut v = Vec::new();
        for path in std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "rs")) {
            let text = std::fs::read_to_string(&path).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            for call in ["err(", "Error(", "Notice(", "Notice { message: "] {
                for (i, _) in code.match_indices(call) {
                    if code[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    v.extend(literal(&code[i + call.len()..]).map(|m| (file.clone(), m)));
                }
            }
        }
        v
    }

    #[test]
    fn every_agent_crate_message_has_a_vietnamese_row() {
        let messages = agent_crate_messages();
        for known in ["stop the running turn first", "the agent has ended", "Stopped.", "Continued in Terminal.", "skipped an event larger than 1 MB"] {
            assert!(messages.iter().any(|(_, m)| m == known), "the scan missed {known:?}: {messages:?}");
        }
        let gaps = untranslated(messages);
        assert!(gaps.is_empty(), "bashcut-agent messages with no row in strings/vi.tsv:\n{gaps}");
        // and every template is a row
        let gaps = untranslated(AGENT_MESSAGES.iter().map(|t| ("strings.rs".to_string(), t.to_string())).collect());
        assert!(gaps.is_empty(), "templates with no row in strings/vi.tsv:\n{gaps}");
    }

    /// `text` with every `{…}` placeholder written `{}` (an escaped `{{` stays), so a `format!`
    /// literal and its template compare whatever their names.
    fn shape(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' && chars.peek() == Some(&'{') {
                chars.next();
                out.push_str("{{");
            } else if c == '{' {
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                }
                out.push_str("{}");
            } else {
                out.push(c);
            }
        }
        out
    }

    /// The `format!` literals the bashcut-agent crate shows as an error (`err(format!("…"` and
    /// `Error(format!("…"`), outside its test modules.
    fn agent_crate_formats() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../bashcut-agent/src");
        let mut v = Vec::new();
        for path in std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "rs")) {
            let text = std::fs::read_to_string(&path).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            for call in ["err(format!(", "Error(format!("] {
                for (i, _) in code.match_indices(call) {
                    if code[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    v.extend(literal(&code[i + call.len()..]).map(|m| (file.clone(), m)));
                }
            }
        }
        v
    }

    #[test]
    fn every_agent_crate_format_has_a_template() {
        // VI5: a format!() message with no template stays English in Vietnamese
        let formats = agent_crate_formats();
        for known in ["unknown command bashcut.agent.{name}", "bad conversation id `{id}`", "{}: no {SERVER} server", "cannot start `{program}`: {e}"] {
            assert!(formats.iter().any(|(_, m)| m == known), "the scan missed {known:?}: {formats:?}");
        }
        // only a value with nothing of ours in it: a serde error after a file name
        let passthrough = ["{}: {e}"];
        let shapes: Vec<String> = AGENT_MESSAGES.iter().map(|t| shape(t)).collect();
        let mut gaps: Vec<String> = formats
            .into_iter()
            .filter(|(_, m)| !passthrough.contains(&m.as_str()) && !shapes.contains(&shape(m)))
            .map(|(file, m)| format!("{file}: {m:?}"))
            .collect();
        gaps.sort();
        gaps.dedup();
        assert!(gaps.is_empty(), "bashcut-agent format!() messages with no strings::AGENT_MESSAGES template:\n{}", gaps.join("\n"));
        assert_eq!(shape("a {x} {{b}} {}"), "a {} {{b}} {}");
    }

    #[test]
    fn literal_reads_escapes_and_continuations() {
        assert_eq!(literal(r#" "a \"b\" c", x"#).as_deref(), Some(r#"a "b" c"#));
        assert_eq!(literal("\"one \\\n        two\"").as_deref(), Some("one two"));
        assert_eq!(literal("name)"), None);
        assert_eq!(looked_up(r#"s("Hi") + f("{n} files", &[]) + is("no") + x.s("no")"#), ["Hi", "{n} files"]);
    }
}
