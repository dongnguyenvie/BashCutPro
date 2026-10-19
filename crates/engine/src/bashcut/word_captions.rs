//! BashCutPro: word-by-word caption styles (#49), the no-hook route of `bashcut/word-captions.md`.
//!
//! `bashcut.captions.wordStyle` splits each caption at its words' start times (`captions.split`), turns the segments
//! into graphic text layers (`graphics.upgradeCaption`) and styles the spoken word of each segment with per-character
//! style runs (`graphics.setCharStyle`): highlight colour, pop (a larger size, optional faux bold) and the upcoming
//! words dimmed. Word times come from the transcript (`transcript.inspect`) when its words line up with the caption,
//! else the caption's duration is shared by character count. Everything is one undo step; no render hook.

use filmcraft_project::ClipId;
use filmcraft_time::{FrameRate, Tick};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, bad, f64_p, has_seq, str_p};
use crate::{EngineError, Result, Session};

const CMD: &str = "bashcut.captions.wordStyle";
const UNDO_LABEL: &str = "Word Caption Style";
const MAX_CAPTIONS: usize = 2_000;
const MAX_WORDS: usize = 64;

pub fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: CMD,
        label: "Word-by-Word Caption Style",
        menu: &[],
        shortcut: None,
        params: r##"{"captions":[id]? (default: the selected captions, else every caption of the first track),"style":{"preset":"karaoke|pop|clean"?,"highlight":"#rrggbb"?,"pop":1..1.6?,"bold":bool?,"dim":0..1? (alpha of the upcoming words; 1 = off)}?,"useTranscript":bool=true}"##,
        enabled: has_seq,
        run: word_style,
        journal: true,
    }]
}

/// The look of the spoken word.
#[derive(Clone, Debug, PartialEq)]
pub struct WordStyle {
    pub highlight: String,
    pub pop: f64,
    pub bold: bool,
    pub dim: f64,
}

impl WordStyle {
    pub fn preset(name: &str) -> Option<WordStyle> {
        Some(match name {
            "karaoke" => WordStyle { highlight: "#ffd400".into(), pop: 1.0, bold: false, dim: 0.55 },
            "pop" => WordStyle { highlight: "#ffd400".into(), pop: 1.2, bold: true, dim: 1.0 },
            "clean" => WordStyle { highlight: "#4fc3ff".into(), pop: 1.0, bold: false, dim: 1.0 },
            _ => return None,
        })
    }

    fn from_params(p: &Value) -> Result<WordStyle> {
        let st = p.get("style").cloned().unwrap_or_else(|| json!({}));
        if !st.is_object() {
            return Err(bad(CMD, "`style` must be an object"));
        }
        let preset = str_p(&st, "preset").unwrap_or("karaoke");
        let mut w = WordStyle::preset(preset).ok_or_else(|| bad(CMD, format!("unknown preset `{preset}` (karaoke, pop, clean)")))?;
        if let Some(h) = str_p(&st, "highlight") {
            if !is_hex_color(h) {
                return Err(bad(CMD, "`highlight` must be \"#rrggbb\""));
            }
            w.highlight = h.to_string();
        }
        if let Some(v) = f64_p(&st, "pop") {
            if !v.is_finite() {
                return Err(bad(CMD, "`pop` must be a number"));
            }
            w.pop = v.clamp(1.0, 1.6);
        }
        if let Some(b) = st.get("bold").and_then(Value::as_bool) {
            w.bold = b;
        }
        if let Some(v) = f64_p(&st, "dim") {
            if !v.is_finite() {
                return Err(bad(CMD, "`dim` must be a number"));
            }
            w.dim = v.clamp(0.0, 1.0);
        }
        Ok(w)
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s.chars().skip(1).all(|c| c.is_ascii_hexdigit())
}

/// Caption text as `graphics.upgradeCaption` lays it out: `<…>` tags removed.
pub fn plain_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i..).unwrap_or("");
        match tail.find('>') {
            Some(j) => rest = tail.get(j + 1..).unwrap_or(""),
            None => {
                // no closing '>': keep the '<' literally
                out.push_str(tail);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Words as character ranges `[start, end)` of `text` (split on whitespace).
pub fn word_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.chars().enumerate() {
        match (c.is_whitespace(), start) {
            (true, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, text.chars().count()));
    }
    out
}

/// Start time of each word: the transcript's when it has exactly as many words inside the caption, else the
/// caption's duration shared by character count.
pub fn word_starts(start: Tick, duration: Tick, words: &[(usize, usize)], transcript: &[(Tick, Tick)]) -> Vec<Tick> {
    let end = start + duration;
    let inside: Vec<Tick> = transcript.iter().filter(|(a, b)| *b > start && *a < end).map(|(a, _)| (*a).max(start)).collect();
    if inside.len() == words.len() && !words.is_empty() {
        return inside;
    }
    let total: usize = words.iter().map(|(a, b)| b.saturating_sub(*a).max(1)).sum::<usize>().max(1);
    let mut acc = 0usize;
    words
        .iter()
        .map(|(a, b)| {
            let t = Tick((duration.0 as i128 * acc as i128 / total as i128) as i64);
            acc += b.saturating_sub(*a).max(1);
            start + t
        })
        .collect()
}

fn hex_to_rgba(hex: &str, alpha: f64) -> Value {
    let c = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(255) as f64 / 255.0;
    json!([c(1), c(3), c(5), alpha])
}

/// Which captions: `captions`, else the selected ones, else every caption of the first caption track.
fn caption_targets(s: &Session, p: &Value) -> Vec<ClipId> {
    if let Some(a) = p.get("captions").and_then(Value::as_array) {
        return a.iter().filter_map(Value::as_u64).map(ClipId).collect();
    }
    if !s.state.caption_selection.is_empty() {
        return s.state.caption_selection.clone();
    }
    s.active_sequence().and_then(|q| q.caption_tracks.first()).map(|t| t.captions.iter().map(|c| c.id).collect()).unwrap_or_default()
}

struct Job {
    id: ClipId,
    start: Tick,
    duration: Tick,
    text: String,
    /// Track style: base font size (at 1080 lines) and colour.
    size: f64,
    color: [u8; 4],
}

fn word_style(s: &mut Session, p: &Value) -> Result<Value> {
    let style = WordStyle::from_params(p)?;
    let ids = caption_targets(s, p);
    if ids.is_empty() {
        return Err(bad(CMD, "no captions (pass `captions`, select captions, or add a caption track)"));
    }
    if ids.len() > MAX_CAPTIONS {
        return Err(bad(CMD, format!("at most {MAX_CAPTIONS} captions at once")));
    }
    let (jobs, height, rate) = {
        let seq = s.active_sequence().ok_or(EngineError::NoSequence)?;
        let mut jobs = Vec::new();
        for tr in &seq.caption_tracks {
            for c in tr.captions.iter().filter(|c| ids.contains(&c.id)) {
                jobs.push(Job { id: c.id, start: c.start, duration: c.duration, text: c.text.clone(), size: f64::from(tr.style.size), color: tr.style.color });
            }
        }
        (jobs, f64::from(seq.settings.height.max(1)), seq.frame_rate())
    };
    if jobs.is_empty() {
        return Err(bad(CMD, "no such captions"));
    }
    let transcript: Vec<(Tick, Tick)> = if p.get("useTranscript").and_then(Value::as_bool).unwrap_or(true) {
        s.execute("transcript.inspect", json!({}))
            .ok()
            .and_then(|v| v.get("words").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|w| Some((Tick(w.get("start")?.as_i64()?), Tick(w.get("end")?.as_i64()?))))
            .collect()
    } else {
        Vec::new()
    };

    let before = s.history.undo.len();
    let result = apply(s, &style, &jobs, &transcript, height, rate);
    let steps = s.history.undo.len().saturating_sub(before);
    fold_history(s, steps);
    result
}

fn apply(s: &mut Session, style: &WordStyle, jobs: &[Job], transcript: &[(Tick, Tick)], height: f64, rate: FrameRate) -> Result<Value> {
    let mut report = Vec::with_capacity(jobs.len());
    for job in jobs {
        let words = word_ranges(&plain_text(&job.text));
        if words.len() < 2 || words.len() > MAX_WORDS {
            report.push(json!({"caption": job.id.0, "skipped": format!("{} words (2 to {MAX_WORDS} are styled)", words.len())}));
            continue;
        }
        // 1. split at each word's start; a refused split (too short) leaves that word with the previous segment
        // on frame boundaries (nearest), else the graphics round out to whole frames and two words show at once
        let starts: Vec<Tick> = word_starts(job.start, job.duration, &words, transcript).into_iter().map(|t| rate.snap_nearest(t)).collect();
        let mut segments: Vec<(u64, usize)> = vec![(job.id.0, 0)];
        let mut current = job.id.0;
        for (k, t) in starts.iter().enumerate().skip(1) {
            let split = s.execute("captions.split", json!({"caption": current, "time": t.0}));
            if let Some(new) = split.ok().and_then(|v| v.get("captions").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_u64)) {
                segments.push((new, k));
                current = new;
            }
        }
        // 2. one graphic clip per segment, in time order
        let ids: Vec<u64> = segments.iter().map(|x| x.0).collect();
        let upgraded = s.execute("graphics.upgradeCaption", json!({"captions": ids}))?;
        let clips: Vec<u64> = upgraded.get("clips").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        if clips.len() != segments.len() {
            return Err(EngineError::Other(format!("{CMD}: caption {} became {} graphics for {} segments", job.id.0, clips.len(), segments.len())));
        }
        // 3. style the spoken word of each segment
        let base = job.size * height / 1080.0;
        let dim = hex_to_rgba(&format!("#{:02x}{:02x}{:02x}", job.color[0], job.color[1], job.color[2]), style.dim);
        for (clip, (_, k)) in clips.iter().zip(&segments) {
            let text = s
                .execute("graphics.list", json!({"clip": clip}))?
                .get("layers")
                .and_then(|l| l.get(0))
                .and_then(|l| l.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let ranges = word_ranges(&text);
            let Some(&(a, b)) = ranges.get(*k) else { continue };
            let mut word = json!({"color": style.highlight});
            if style.pop > 1.0 {
                word["size"] = json!(base * style.pop);
            }
            if style.bold {
                word["bold"] = json!(true);
            }
            s.execute("graphics.setCharStyle", json!({"clip": clip, "layer": 0, "start": a, "end": b, "style": word}))?;
            let n = text.chars().count();
            if style.dim < 1.0 && b < n {
                s.execute("graphics.setCharStyle", json!({"clip": clip, "layer": 0, "start": b, "end": n, "style": {"color": dim}}))?;
            }
        }
        report.push(json!({"caption": job.id.0, "words": words.len(), "clips": clips}));
    }
    Ok(json!({"captions": report}))
}

/// Fold the last `steps` undo entries into one labelled [`UNDO_LABEL`].
fn fold_history(s: &mut Session, steps: usize) {
    let len = s.history.undo.len();
    let steps = steps.min(len);
    if steps == 0 {
        return;
    }
    let first = len - steps;
    s.history.undo.truncate(first + 1);
    if let Some(step) = s.history.undo.get_mut(first) {
        step.0 = UNDO_LABEL.to_string();
    }
}
