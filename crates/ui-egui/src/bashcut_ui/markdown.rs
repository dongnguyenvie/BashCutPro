//! Minimal Markdown for agent replies (UX8): paragraphs, `#`/`##` headings, `-`/`1.` lists,
//! `**bold**`, `*em*`, inline code and fenced code. One pass per line and per span, no recursion;
//! input past [`MAX_LEN`] is cut at a char boundary. Anything else shows as plain text.

/// Bytes of a reply that are parsed; the rest is dropped with an ellipsis.
pub const MAX_LEN: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading(u8, String),
    Para(String),
    /// marker ("•" or "3."), text
    Item(String, String),
    Code(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub em: bool,
    pub code: bool,
}

fn cap(text: &str) -> (&str, bool) {
    if text.len() <= MAX_LEN {
        return (text, false);
    }
    let mut end = MAX_LEN;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text.get(..end).unwrap_or_default(), true)
}

fn flush(para: &mut String, out: &mut Vec<Block>) {
    if !para.is_empty() {
        out.push(Block::Para(std::mem::take(para)));
    }
}

/// Blocks of `text`. An unterminated fence runs to the end.
pub fn blocks(text: &str) -> Vec<Block> {
    let (text, cut) = cap(text);
    let mut out = Vec::new();
    let mut para = String::new();
    let mut code: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            match code.take() {
                Some(c) => out.push(Block::Code(c)),
                None => {
                    flush(&mut para, &mut out);
                    code = Some(String::new());
                }
            }
            continue;
        }
        if let Some(c) = code.as_mut() {
            if !c.is_empty() {
                c.push('\n');
            }
            c.push_str(line);
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut para, &mut out);
        } else if let Some(h) = trimmed.strip_prefix("## ").or_else(|| trimmed.strip_prefix("### ")) {
            flush(&mut para, &mut out);
            out.push(Block::Heading(2, h.trim().to_string()));
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            flush(&mut para, &mut out);
            out.push(Block::Heading(1, h.trim().to_string()));
        } else if let Some(i) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            flush(&mut para, &mut out);
            out.push(Block::Item("•".into(), i.trim().to_string()));
        } else if let Some((n, rest)) = numbered(trimmed) {
            flush(&mut para, &mut out);
            out.push(Block::Item(format!("{n}."), rest.trim().to_string()));
        } else {
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(trimmed);
        }
    }
    if let Some(c) = code {
        out.push(Block::Code(c));
    }
    flush(&mut para, &mut out);
    if cut {
        out.push(Block::Para("…".into()));
    }
    out
}

fn numbered(line: &str) -> Option<(&str, &str)> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let (n, rest) = line.split_at_checked(digits)?;
    Some((n, rest.strip_prefix(". ")?))
}

/// Inline spans of one block: `**bold**`, `*em*`, `` `code` ``. An unclosed marker is kept as text.
pub fn spans(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut cur = Span::default();
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        let marker = if ch == '`' {
            "`"
        } else if rest.starts_with("**") {
            "**"
        } else if ch == '*' {
            "*"
        } else {
            cur.text.push(ch);
            rest = rest.get(ch.len_utf8()..).unwrap_or_default();
            continue;
        };
        let after = rest.get(marker.len()..).unwrap_or_default();
        if cur.code && marker != "`" {
            cur.text.push_str(marker);
        } else {
            let closes = cur.code || (cur.bold && marker == "**") || (cur.em && marker == "*");
            if closes || after.contains(marker) {
                if !cur.text.is_empty() {
                    out.push(cur.clone());
                }
                cur.text.clear();
                match marker {
                    "`" => cur.code = !cur.code,
                    "**" => cur.bold = !cur.bold,
                    _ => cur.em = !cur.em,
                }
            } else {
                cur.text.push_str(marker);
            }
        }
        rest = after;
    }
    if !cur.text.is_empty() {
        out.push(cur);
    }
    out
}

/// Draw `text` as Markdown in `color` (list markers in `dim`).
pub fn show(ui: &mut egui::Ui, text: &str, color: egui::Color32, dim: egui::Color32) {
    for b in blocks(text) {
        match b {
            Block::Heading(level, h) => {
                ui.add_space(2.0);
                let size = if level == 1 { 15.0 } else { 13.0 };
                ui.add(egui::Label::new(egui::RichText::new(h).size(size).strong().color(color)).selectable(true).wrap());
            }
            Block::Para(p) => {
                ui.horizontal_wrapped(|ui| inline(ui, &p, color));
            }
            Block::Item(marker, item) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format!("{marker} ")).color(dim));
                    inline(ui, &item, color);
                });
            }
            Block::Code(c) => {
                egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(6.0).corner_radius(3.0).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(c).monospace().size(11.0).color(color)).selectable(true).wrap());
                });
            }
        }
    }
}

fn inline(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.spacing_mut().item_spacing.x = 0.0;
    for s in spans(text) {
        let mut r = egui::RichText::new(s.text).color(color);
        if s.bold {
            r = r.strong();
        }
        if s.em {
            r = r.italics();
        }
        if s.code {
            r = r.code();
        }
        ui.add(egui::Label::new(r).selectable(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_bold_and_code_block_lose_their_markers() {
        let b = blocks("# Plan\nSome **bold** and *em* text.\n\n- one\n- `two`\n1. first\n```\nlet x = 1;\n```");
        assert_eq!(b.first(), Some(&Block::Heading(1, "Plan".into())));
        assert_eq!(b.get(2), Some(&Block::Item("•".into(), "one".into())));
        assert_eq!(b.get(4), Some(&Block::Item("1.".into(), "first".into())));
        assert_eq!(b.get(5), Some(&Block::Code("let x = 1;".into())));
        let s = spans("Some **bold** and *em* `c` text.");
        assert!(s.iter().all(|s| !s.text.contains('*') && !s.text.contains('`')), "{s:?}");
        assert!(s.iter().any(|s| s.bold && s.text == "bold"));
        assert!(s.iter().any(|s| s.em && s.text == "em"));
        assert!(s.iter().any(|s| s.code && s.text == "c"));
    }

    #[test]
    fn unterminated_markers_and_fences_are_kept() {
        assert_eq!(blocks("text\n```\ncode"), vec![Block::Para("text".into()), Block::Code("code".into())]);
        for raw in ["a * b ` d", "a **b", "x `y"] {
            assert_eq!(spans(raw).iter().map(|s| s.text.as_str()).collect::<String>(), raw);
        }
        assert_eq!(spans("`**x**`").first().map(|s| s.text.as_str()), Some("**x**"));
    }

    #[test]
    fn one_megabyte_is_capped() {
        let big = "**é*` ".repeat(1024 * 1024 / 7);
        let b = blocks(&big);
        assert_eq!(b.last(), Some(&Block::Para("…".into())));
        let total: usize = b.iter().map(|b| if let Block::Para(p) = b { p.len() } else { 0 }).sum();
        assert!(total <= MAX_LEN + 3, "{total}");
        assert!(!spans(&big).is_empty());
        assert_eq!(blocks(&"```\n".repeat(300_000)).last(), Some(&Block::Para("…".into())));
    }
}
