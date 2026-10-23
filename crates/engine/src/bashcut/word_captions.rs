//! BashCutPro: word-by-word caption styles (#49), the no-hook route of `bashcut/word-captions.md`.
//!
//! `bashcut.captions.wordStyle` splits each caption at its words' start times (`captions.split`), turns the segments
//! into graphic text layers (`graphics.upgradeCaption`) and styles the spoken word of each segment with per-character
//! style runs (`graphics.setCharStyle`): highlight colour, pop (a larger size, optional faux bold) and the upcoming
//! words dimmed. Word times come from the transcript (`transcript.inspect`) when its words line up with the caption,
//! else the caption's duration is shared by character count. A highlight step lasts at least [`MIN_STEP_SECONDS`]:
//! fast words share a step with their neighbour instead of flashing for a frame or two (#213). Everything is one undo
//! step; no render hook.

use filmcraft_project::ClipId;
use filmcraft_time::{FrameRate, Tick};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, bad, f64_p, has_seq, str_p};
use crate::{EngineError, Result, Session};

const CMD: &str = "bashcut.captions.wordStyle";
const UNDO_LABEL: &str = "Word Caption Style";
const MAX_CAPTIONS: usize = 2_000;
const MAX_WORDS: usize = 64;
/// Shortest highlight step: a faster word is highlighted together with its neighbour (review flags shorter graphic
/// clips as flash frames, #213).
pub const MIN_STEP_SECONDS: f64 = 0.25;

pub fn commands() -> Vec<CommandSpec> {
    vec![spec! {
        id: CMD,
        label: "Word-by-Word Caption Style",
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

/// Which word starts open a highlight step: the first word always, then a word only when the step before it would
/// last at least `min_step`; a last step shorter than that joins the one before. The result is the indices into
/// `starts` (ascending, starting with 0).
pub fn step_starts(start: Tick, duration: Tick, starts: &[Tick], min_step: Tick) -> Vec<usize> {
    let end = start + duration;
    let mut kept = vec![0usize];
    let mut last = start;
    for (k, t) in starts.iter().enumerate().skip(1) {
        if *t - last >= min_step && end - *t > Tick::ZERO {
            kept.push(k);
            last = *t;
        }
    }
    while kept.len() > 1 {
        let tail = kept.last().and_then(|k| starts.get(*k)).copied().unwrap_or(end);
        if end - tail >= min_step {
            break;
        }
        kept.pop();
    }
    kept
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
    let (jobs, (width, height), rate) = {
        let seq = s.active_sequence().ok_or(EngineError::NoSequence)?;
        let mut jobs = Vec::new();
        for tr in &seq.caption_tracks {
            for c in tr.captions.iter().filter(|c| ids.contains(&c.id)) {
                jobs.push(Job { id: c.id, start: c.start, duration: c.duration, text: c.text.clone(), size: f64::from(tr.style.size), color: tr.style.color });
            }
        }
        (jobs, (f64::from(seq.settings.width.max(1)), f64::from(seq.settings.height.max(1))), seq.frame_rate())
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

    let group = super::undo::UndoGroup::start(s);
    let result = apply(s, &style, &jobs, &transcript, (width, height), rate);
    group.finish(s, UNDO_LABEL);
    result
}

fn apply(s: &mut Session, style: &WordStyle, jobs: &[Job], transcript: &[(Tick, Tick)], (width, height): (f64, f64), rate: FrameRate) -> Result<Value> {
    let mut report = Vec::with_capacity(jobs.len());
    for job in jobs {
        let words = word_ranges(&plain_text(&job.text));
        // a one-word cue ("Yes.") is one segment with its word highlighted, like the others (#199)
        if words.is_empty() || words.len() > MAX_WORDS {
            report.push(json!({"caption": job.id.0, "skipped": format!("{} words (1 to {MAX_WORDS} are styled)", words.len())}));
            continue;
        }
        // 1. split at each word's start; a refused split (too short) leaves that word with the previous segment
        // on frame boundaries (nearest), else the graphics round out to whole frames and two words show at once
        let starts: Vec<Tick> = word_starts(job.start, job.duration, &words, transcript).into_iter().map(|t| rate.snap_nearest(t)).collect();
        // a word faster than a highlight step shares the step of its neighbour (#213)
        let min_step = rate.snap_nearest(Tick::from_seconds_f64(MIN_STEP_SECONDS)).max(rate.frame_duration());
        let mut segments: Vec<(u64, usize)> = vec![(job.id.0, 0)];
        let mut current = job.id.0;
        for k in step_starts(job.start, job.duration, &starts, min_step).into_iter().skip(1) {
            let Some(t) = starts.get(k) else { continue };
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
        // 3. a graphic text layer does not wrap like the caption did: break the lines ourselves, inside the frame
        // (a popped word grows, so a little narrower then)
        let max_width = width * if style.pop > 1.0 { 0.84 } else { 0.9 };
        wrap_segments(s, &clips, max_width, height)?;
        // 4. style the spoken word(s) of each segment: words `k` up to the next segment's first word
        let base = job.size * height / 1080.0;
        let dim = hex_to_rgba(&format!("#{:02x}{:02x}{:02x}", job.color[0], job.color[1], job.color[2]), style.dim);
        let mut steps = Vec::with_capacity(segments.len());
        for (i, (clip, (_, k))) in clips.iter().zip(&segments).enumerate() {
            let last = segments.get(i + 1).map_or(words.len(), |x| x.1).saturating_sub(1).max(*k);
            steps.push(json!([k, last]));
            let text = s
                .execute("graphics.list", json!({"clip": clip}))?
                .get("layers")
                .and_then(|l| l.get(0))
                .and_then(|l| l.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let ranges = word_ranges(&text);
            let (Some(&(a, _)), Some(&(_, b))) = (ranges.get(*k), ranges.get(last).or(ranges.last())) else { continue };
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
        report.push(json!({"caption": job.id.0, "words": words.len(), "clips": clips, "steps": steps}));
    }
    Ok(json!({"captions": report}))
}

/// Text, local bounds `[x0, y0, x1, y1]` and position of a graphic clip's first layer.
fn first_layer(s: &mut Session, clip: u64) -> Result<(String, [f64; 4], [f64; 2])> {
    let list = s.execute("graphics.list", json!({"clip": clip}))?;
    let layer = list.get("layers").and_then(|l| l.get(0)).cloned().unwrap_or_default();
    let text = layer.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
    let num = |v: &Value, i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0);
    let b = layer.get("localBounds").cloned().unwrap_or_default();
    let pos = layer.get("position").cloned().unwrap_or_default();
    Ok((text, [num(&b, 0), num(&b, 1), num(&b, 2), num(&b, 3)], [num(&pos, 0), num(&pos, 1)]))
}

/// Width of `text` laid out in the first layer of `clip` (it is left holding `text`).
fn text_width(s: &mut Session, clip: u64, text: &str) -> Result<f64> {
    s.execute("graphics.setText", json!({"clip": clip, "layer": 0, "text": text}))?;
    let (_, b, _) = first_layer(s, clip)?;
    Ok(b[2] - b[0])
}

/// `text` with spaces turned into line breaks so that no line is wider than `max_width` (greedy, measured with the
/// layer's own layout). Every character keeps its index, so the word ranges and style runs still line up.
pub fn wrap_text(text: &str, max_width: f64, mut width_of: impl FnMut(&str) -> Result<f64>) -> Result<String> {
    if text.contains('\n') || width_of(text)? <= max_width {
        return Ok(text.to_string());
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for (i, word) in text.split(' ').enumerate() {
        if i == 0 {
            current.push_str(word);
            continue;
        }
        let candidate = format!("{current} {word}");
        if word.is_empty() || width_of(&candidate)? <= max_width {
            current = candidate;
        } else {
            lines.push(std::mem::replace(&mut current, word.to_string()));
        }
    }
    lines.push(current);
    Ok(lines.join("\n"))
}

/// Wrap the (identical) text of a caption's segments inside `max_width`; a block in the lower half keeps its bottom
/// edge where it was, so the extra lines go up.
fn wrap_segments(s: &mut Session, clips: &[u64], max_width: f64, height: f64) -> Result<()> {
    let Some(&probe) = clips.first() else { return Ok(()) };
    let (text, before, pos) = first_layer(s, probe)?;
    let wrapped = wrap_text(&text, max_width, |t| text_width(s, probe, t))?;
    if wrapped == text {
        // the probe may hold a measured part of the text
        s.execute("graphics.setText", json!({"clip": probe, "layer": 0, "text": text}))?;
        return Ok(());
    }
    for &clip in clips {
        s.execute("graphics.setText", json!({"clip": clip, "layer": 0, "text": wrapped}))?;
    }
    let (_, after, _) = first_layer(s, probe)?;
    let grow = after[3] - before[3];
    if pos[1] > height / 2.0 && grow > 0.0 {
        for &clip in clips {
            s.execute("graphics.set", json!({"clip": clip, "layer": 0, "props": {"position": [pos[0], pos[1] - grow]}}))?;
        }
    }
    Ok(())
}
