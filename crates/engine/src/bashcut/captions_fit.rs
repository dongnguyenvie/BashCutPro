//! BashCutPro: captions grouped from the transcript at a readable speed (#200).
//!
//! `transcript.createCaptions` (upstream) breaks a cue on the character limit and on pauses of 1 s, never at a
//! comma, and ends each cue on its last word, so cues end mid-phrase and fast ones have no silence to hold into.
//! `bashcut.captions.create` makes the caption track from the same words (`transcript::sequence_words`) and:
//!
//! - breaks a cue on a word edge at a sentence end, at a comma once the cue is half full (`fill`), at a pause of
//!   `breakSeconds`, at a speaker change, or when the text no longer fits `maxChars` × `lines` or `maxSeconds`;
//!   never after "a", "the", "and"… when it breaks on the limit, and no lone word inside a sentence;
//! - drops hesitation fillers ("um", "uh") and stuttered repeats ("the the") from the text (`keepFillers` keeps them);
//! - holds each cue into the silence after it until it reads at 95 % of `maxCps` (never over the next cue less
//!   `gapFrames`), starts it up to `leadSeconds` before its first word when that is not enough, and merges a still
//!   fast cue with a neighbour when the pair reads under the limit and fits the lines; `trimHedges` then drops
//!   "just", "you know"… from the cues still over.
//!
//! A port of the agent kit's `skills/captions-text/captions_fit.py group` (same rules, same defaults), in ticks.

use std::ops::Range;

use filmcraft_edit::transcript::SeqWord;
use filmcraft_project::{Caption, CaptionFormat, CaptionTrack, ClipId, TrackId};
use filmcraft_time::{FrameRate, Tick};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, bad, f64_p, has_seq, str_p, u64_p};
use crate::{Result, Session};

const CMD: &str = "bashcut.captions.create";
const MAX_WORDS: usize = 200_000;

const FILLERS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "erm", "er", "ah", "eh", "hmm", "mm", "mhm"];
const HEDGES: &[&str] = &["you know", "i mean", "kind of", "sort of", "just", "actually", "really", "basically", "literally", "honestly", "like"];
const WEAK: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "to", "of", "in", "on", "at", "for", "with", "my", "your", "our", "their", "his", "her", "its", "is", "are", "was",
    "i", "you", "we", "they", "that", "if", "so",
];

pub fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: CMD,
        label: "Create Readable Captions from Transcript…",
        menu: &[],
        shortcut: None,
        params: r#"{"maxChars":n=42 (characters a line),"lines":1|2=2,"maxCps":f=20 (characters a second),"minSeconds":f=0.7,"maxSeconds":f=7,"breakSeconds":f=0.5 (a pause this long starts a cue),"fill":0..1=0.5 (break at a comma once the cue is this full),"gapFrames":n=2,"leadSeconds":f=0.15 (a cue may start this much before its first word),"keepFillers":bool=false,"trimHedges":bool=false,"format":str?,"name":str?="Captions","dryRun":bool=false}"#,
        enabled: has_words,
        run: create,
        journal: true,
    }]
}

fn has_words(s: &Session) -> std::result::Result<(), String> {
    has_seq(s)?;
    if crate::transcript::sequence_words(s).is_empty() { Err("the sequence has no transcript (Transcribe first)".into()) } else { Ok(()) }
}

/// Grouping and timing rules.
#[derive(Clone, Debug, PartialEq)]
pub struct FitRules {
    pub max_chars: usize,
    /// 1 or 2.
    pub lines: usize,
    pub max_cps: f64,
    pub min_duration: Tick,
    pub max_duration: Tick,
    pub break_pause: Tick,
    pub fill: f64,
    pub gap_frames: i64,
    pub lead: Tick,
    /// The last cue holds at most to here (the sequence end); `None`: 1 s past its last word.
    pub end: Option<Tick>,
    pub keep_fillers: bool,
    pub trim_hedges: bool,
}

impl Default for FitRules {
    fn default() -> Self {
        Self {
            max_chars: 42,
            lines: 2,
            max_cps: 20.0,
            min_duration: Tick::from_seconds_f64(0.7),
            max_duration: Tick::from_seconds_f64(7.0),
            break_pause: Tick::from_seconds_f64(0.5),
            fill: 0.5,
            gap_frames: 2,
            lead: Tick::from_seconds_f64(0.15),
            end: None,
            keep_fillers: false,
            trim_hedges: false,
        }
    }
}

impl FitRules {
    fn from_params(p: &Value) -> Result<FitRules> {
        let d = FitRules::default();
        let num = |k: &str, lo: f64, hi: f64| -> Result<Option<f64>> {
            match f64_p(p, k) {
                Some(v) if !v.is_finite() => Err(bad(CMD, format!("`{k}` must be a number"))),
                Some(v) => Ok(Some(v.clamp(lo, hi))),
                None => Ok(None),
            }
        };
        let secs = |k: &str, lo: f64, hi: f64, def: Tick| -> Result<Tick> { Ok(num(k, lo, hi)?.map(Tick::from_seconds_f64).unwrap_or(def)) };
        let flag = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
        Ok(FitRules {
            max_chars: u64_p(p, "maxChars").map(|n| n.clamp(8, 200) as usize).unwrap_or(d.max_chars),
            lines: u64_p(p, "lines").map(|n| n.clamp(1, 2) as usize).unwrap_or(d.lines),
            max_cps: num("maxCps", 5.0, 60.0)?.unwrap_or(d.max_cps),
            min_duration: secs("minSeconds", 0.0, 10.0, d.min_duration)?,
            max_duration: secs("maxSeconds", 1.0, 30.0, d.max_duration)?,
            break_pause: secs("breakSeconds", 0.05, 10.0, d.break_pause)?,
            fill: num("fill", 0.0, 1.0)?.unwrap_or(d.fill),
            gap_frames: u64_p(p, "gapFrames").map(|n| n.min(30) as i64).unwrap_or(d.gap_frames),
            lead: secs("leadSeconds", 0.0, 1.0, d.lead)?,
            end: None,
            keep_fillers: flag("keepFillers"),
            trim_hedges: flag("trimHedges"),
        })
    }
}

/// A word as the grouper sees it (sequence time).
#[derive(Clone, Debug, PartialEq)]
pub struct FitWord {
    pub text: String,
    pub start: Tick,
    pub end: Tick,
    pub speaker: Option<String>,
}

impl From<&SeqWord> for FitWord {
    fn from(w: &SeqWord) -> Self {
        FitWord { text: w.text.trim().to_string(), start: w.start, end: w.end, speaker: w.speaker.clone() }
    }
}

/// A caption cue.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: Tick,
    pub end: Tick,
    /// Lines joined by `\n`.
    pub text: String,
    pub speaker: Option<String>,
    /// End of its last word (before holding).
    pub word_end: Tick,
}

// ------------------------------------------------------------------------------------------- text

/// Characters as a reader (and the review) counts them: lines joined by one space.
pub fn chars(text: &str) -> usize {
    let n: usize = text.lines().map(|l| l.trim().chars().count()).sum();
    n + text.lines().count().saturating_sub(1)
}

pub fn cps(c: &Cue) -> f64 {
    chars(&c.text) as f64 / (c.end - c.start).seconds().max(0.001)
}

fn bare(word: &str) -> String {
    word.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '\'').flat_map(char::to_lowercase).collect()
}

fn ends_with_any(w: &str, set: &[char]) -> bool {
    w.chars().last().is_some_and(|c| set.contains(&c))
}

const SENTENCE: &[char] = &['.', '?', '!'];
const PHRASE: &[char] = &[',', ';', ':'];

fn starts_upper(w: &str) -> bool {
    w.chars().next().is_some_and(char::is_uppercase)
}

fn capitalise(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Text without hesitation fillers and stuttered repeats; capitalised again if it was.
pub fn clean(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    for w in &words {
        let b = bare(w);
        if FILLERS.contains(&b.as_str()) {
            if let Some(last) = out.last_mut()
                && ends_with_any(w, SENTENCE)
                && !ends_with_any(last, SENTENCE)
            {
                let mut l = last.trim_end_matches(PHRASE).to_string();
                l.extend(w.chars().last());
                *last = l;
            }
            continue;
        }
        if let Some(last) = out.last_mut()
            && !b.is_empty()
            && b == bare(last)
            && !ends_with_any(last, &['.', '?', '!', ',', ';', ':'])
        {
            *last = (*w).to_string(); // "the the" → "the"
            continue;
        }
        out.push((*w).to_string());
    }
    if out.is_empty() {
        return text.to_string();
    }
    if words.first().is_some_and(|w| starts_upper(w))
        && let Some(first) = out.first_mut()
        && first.chars().next().is_some_and(char::is_lowercase)
    {
        *first = capitalise(first);
    }
    out.join(" ")
}

/// Text without hedges and discourse markers ("just", "you know", …); content words stay.
pub fn trim(text: &str) -> String {
    let mut words: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    for h in HEDGES {
        let hw: Vec<&str> = h.split(' ').collect();
        let mut i = 0;
        while i + hw.len() <= words.len() {
            let hit = hw.iter().enumerate().all(|(k, x)| {
                let w = words.get(i + k).map(|w| w.to_lowercase()).unwrap_or_default();
                // the hedge's last word may carry one comma
                w == *x || (k + 1 == hw.len() && w.strip_suffix(',') == Some(x))
            });
            if hit {
                words.drain(i..i + hw.len());
            } else {
                i += 1;
            }
        }
    }
    let out = words.join(" ");
    let out = out.trim_end_matches(PHRASE);
    if out.is_empty() {
        return text.to_string();
    }
    if starts_upper(text) { capitalise(out) } else { out.to_string() }
}

/// One line when it fits, else the best two-line split (even lines, a break after punctuation, never a line
/// ending on "the"); `None` when it does not fit.
pub fn wrap(text: &str, max_chars: usize, lines: usize) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let one = words.join(" ");
    if one.chars().count() <= max_chars || words.is_empty() {
        return Some(one);
    }
    if lines < 2 {
        return None;
    }
    let mut best: Option<(i64, String)> = None;
    for k in 1..words.len() {
        let (a, b) = (words[..k].join(" "), words[k..].join(" "));
        let (la, lb) = (a.chars().count(), b.chars().count());
        if la > max_chars || lb > max_chars {
            continue;
        }
        let mut score = (la as i64 - lb as i64).abs();
        if ends_with_any(&a, &[',', ';', ':', '.', '?', '!']) {
            score -= 12;
        }
        if words.get(k - 1).is_some_and(|w| WEAK.contains(&bare(w).as_str())) {
            score += 12;
        }
        if best.as_ref().is_none_or(|(s, _)| score < *s) {
            best = Some((score, format!("{a}\n{b}")));
        }
    }
    best.map(|b| b.1)
}

// ------------------------------------------------------------------------------------------ group

/// Cues from words, broken at meaning first and at the limits second; no one-word cue inside a sentence.
pub fn group(words: &[FitWord], r: &FitRules, rate: FrameRate) -> Vec<Cue> {
    let capacity = (r.max_chars * r.lines) as f64;
    let text_of = |g: &Range<usize>| words.get(g.clone()).unwrap_or_default().iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    let span = |g: &Range<usize>| match (words.get(g.start), g.end.checked_sub(1).and_then(|i| words.get(i))) {
        (Some(a), Some(b)) => b.end - a.start,
        _ => Tick::ZERO,
    };
    let fits = |g: &Range<usize>| wrap(&text_of(g), r.max_chars, r.lines).is_some() && span(g) <= r.max_duration;
    let mut groups: Vec<Range<usize>> = Vec::new();
    let mut cur = 0..0;
    for (i, w) in words.iter().enumerate() {
        if !cur.is_empty()
            && let Some(prev) = i.checked_sub(1).and_then(|j| words.get(j))
        {
            let sentence = ends_with_any(&prev.text, SENTENCE) && cur.len() >= 2;
            let phrase = ends_with_any(&prev.text, PHRASE) && text_of(&cur).chars().count() as f64 >= r.fill * capacity;
            let full = !fits(&(cur.start..i + 1));
            if full || sentence || phrase || w.start - prev.end >= r.break_pause || w.speaker != prev.speaker {
                if full && !(sentence || phrase) && cur.len() > 2 && WEAK.contains(&bare(&prev.text).as_str()) {
                    // never end a cue on "the": it opens the next one
                    groups.push(cur.start..i - 1);
                    if fits(&(i - 1..i + 1)) {
                        cur = i - 1..i + 1;
                    } else {
                        groups.push(i - 1..i);
                        cur = i..i + 1;
                    }
                    continue;
                }
                groups.push(cur.clone());
                cur = i..i;
            }
        } else {
            cur = i..i;
        }
        cur.end = i + 1;
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    // a lone word ending a sentence goes back to its cue, or takes a word from it
    let mut k = 1;
    while k < groups.len() {
        let (Some(p), Some(g)) = (groups.get(k - 1).cloned(), groups.get(k).cloned()) else { break };
        let (Some(last), Some(first)) = (words.get(p.end.saturating_sub(1)), words.get(g.start)) else { break };
        let joined = first.start - last.end < r.break_pause && !ends_with_any(&last.text, SENTENCE) && first.speaker == last.speaker;
        if g.len() == 1 && joined {
            if fits(&(p.start..g.end)) {
                groups[k - 1] = p.start..g.end;
                groups.remove(k);
                continue;
            }
            if p.len() > 2 {
                groups[k - 1] = p.start..p.end - 1;
                groups[k] = p.end - 1..g.end;
            }
        }
        k += 1;
    }
    let fd = rate.frame_duration();
    groups
        .iter()
        .filter_map(|g| {
            let (a, b) = (words.get(g.start)?, words.get(g.end.checked_sub(1)?)?);
            let start = rate.snap(a.start);
            let end = ceil(rate, b.end).max(start + fd);
            Some(Cue { start, end, text: text_of(g), speaker: a.speaker.clone(), word_end: end })
        })
        .collect()
}

fn ceil(rate: FrameRate, t: Tick) -> Tick {
    let s = rate.snap(t);
    if s < t { s + rate.frame_duration() } else { s }
}

// -------------------------------------------------------------------------------------------- fit

fn need(c: &Cue, r: &FitRules) -> Tick {
    Tick::from_seconds_f64(chars(&c.text) as f64 / (0.95 * r.max_cps)).max(r.min_duration)
}

/// Hold cue `i` into the silence after it until it reads in time; start it earlier (up to `lead`) when that is not
/// enough, taking back the tail of the cue before when that one still reads in time.
fn hold(work: &mut [Cue], i: usize, r: &FitRules, rate: FrameRate) {
    let fd = rate.frame_duration();
    let gap = Tick(fd.0.saturating_mul(r.gap_frames));
    let next_start = work.get(i + 1).map(|n| n.start);
    let prev = i.checked_sub(1).and_then(|p| work.get(p)).map(|p| (p.start, p.end, p.word_end, need(p, r)));
    let Some(c) = work.get_mut(i) else { return };
    let want = need(c, r);
    if c.end - c.start >= want {
        return;
    }
    let last = r.end.unwrap_or(c.word_end + Tick::from_seconds_f64(1.0));
    let limit = next_start.map(|n| n - gap).unwrap_or(last).min(c.start + want).min(c.start + r.max_duration);
    c.end = c.end.max(rate.snap(limit));
    if c.end - c.start >= want {
        return;
    }
    let mut floor = Tick::ZERO;
    if let Some((p_start, p_end, p_word_end, p_need)) = prev {
        floor = (p_start + p_need).max(p_word_end - r.lead).max(p_start + r.min_duration).min(p_end) + gap;
    }
    let start = c.start.min(ceil(rate, floor.max(c.start - r.lead).max(c.end - want)));
    c.start = start;
    if let Some(p) = i.checked_sub(1).and_then(|p| work.get_mut(p))
        && start < p.end + gap
    {
        p.end = rate.snap(start - gap).max(p.start + fd);
    }
}

/// Clean and wrap the text, hold fast cues into the silence after them, then merge what is still fast.
pub fn fit(cues: &[Cue], r: &FitRules, rate: FrameRate) -> Vec<Cue> {
    let mut work: Vec<Cue> = cues.to_vec();
    for c in &mut work {
        let t = if r.keep_fillers { c.text.clone() } else { clean(&c.text) };
        if let Some(t) = wrap(&t, r.max_chars, r.lines) {
            c.text = t;
        }
    }
    for i in 0..work.len() {
        hold(&mut work, i, r, rate);
    }
    let mut i = 0;
    while i < work.len() {
        let mut best: Option<(f64, usize, String)> = None;
        if work.get(i).is_some_and(|c| cps(c) > r.max_cps) {
            for j in [i.checked_sub(1), Some(i + 1)].into_iter().flatten() {
                let (lo, hi) = (i.min(j), i.max(j));
                let (Some(a), Some(b)) = (work.get(lo), work.get(hi)) else { continue };
                if a.speaker != b.speaker {
                    continue;
                }
                let Some(text) = wrap(&format!("{} {}", a.text, b.text), r.max_chars, r.lines) else { continue };
                if b.end - a.start > r.max_duration {
                    continue;
                }
                let speed = chars(&text) as f64 / (b.end - a.start).seconds().max(0.001);
                if speed <= r.max_cps && best.as_ref().is_none_or(|x| speed < x.0) {
                    best = Some((speed, lo, text));
                }
            }
        }
        let Some((_, k, text)) = best else {
            i += 1;
            continue;
        };
        if k + 1 < work.len() {
            let b = work.remove(k + 1);
            if let Some(a) = work.get_mut(k) {
                a.text = text;
                a.end = b.end;
                a.word_end = b.word_end;
            }
            hold(&mut work, k, r, rate);
        }
        i = k;
    }
    if r.trim_hedges {
        for k in 0..work.len() {
            let Some(c) = work.get_mut(k) else { continue };
            if cps(c) > r.max_cps
                && let Some(t) = wrap(&trim(&c.text), r.max_chars, r.lines)
                && t != c.text
            {
                c.text = t;
                hold(&mut work, k, r, rate);
            }
        }
    }
    // never overlapping, at least a frame each
    let fd = rate.frame_duration();
    for k in 1..work.len() {
        let prev_end = work.get(k - 1).map(|p| p.end).unwrap_or_default();
        if let Some(c) = work.get_mut(k)
            && c.start < prev_end
        {
            c.start = prev_end;
            c.end = c.end.max(c.start + fd);
        }
    }
    work
}

/// Share of cues at or under `limit`, share over 25, median speed.
pub fn stats(cues: &[Cue], limit: f64) -> Value {
    let mut speeds: Vec<f64> = cues.iter().filter(|c| !c.text.trim().is_empty()).map(cps).collect();
    let n = speeds.len().max(1) as f64;
    let pct = |k: usize| (1000.0 * k as f64 / n).round() / 10.0;
    let under = speeds.iter().filter(|s| **s <= limit).count();
    let over = speeds.iter().filter(|s| **s > 25.0).count();
    speeds.sort_by(f64::total_cmp);
    let median = speeds.get(speeds.len() / 2).map(|m| (m * 10.0).round() / 10.0);
    json!({"cues": speeds.len(), "atOrUnder": pct(under), "over25": pct(over), "medianCps": median})
}

// ---------------------------------------------------------------------------------------- command

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let mut rules = FitRules::from_params(p)?;
    let seq_words = crate::transcript::sequence_words(s);
    if seq_words.len() > MAX_WORDS {
        return Err(bad(CMD, format!("at most {MAX_WORDS} words")));
    }
    let mut words: Vec<FitWord> = seq_words.iter().map(FitWord::from).filter(|w| !w.text.is_empty() && w.end >= w.start).collect();
    words.sort_by_key(|w| w.start);
    if words.is_empty() {
        return Err(bad(CMD, "the sequence has no transcript (Transcribe first)"));
    }
    let rate = s.sequence_rate();
    rules.end = s.active_sequence().map(|q| q.duration()).filter(|d| *d > Tick::ZERO);
    let grouped = group(&words, &rules, rate);
    let cues = fit(&grouped, &rules, rate);
    let left: Vec<Value> = cues
        .iter()
        .filter(|c| cps(c) > rules.max_cps)
        .map(|c| json!({"at": super::data::round_to(c.start.seconds(), 2), "cps": super::data::round_to(cps(c), 1), "text": c.text}))
        .collect();
    let mut out = json!({
        "captions": cues.len(),
        "before": stats(&grouped, rules.max_cps),
        "after": stats(&cues, rules.max_cps),
        "left": left,
    });
    if p.get("dryRun").and_then(Value::as_bool).unwrap_or(false) {
        out["cues"] = cues.iter().map(|c| json!({"start": c.start.0, "end": c.end.0, "text": c.text, "speaker": c.speaker})).collect();
        return Ok(out);
    }
    let format = str_p(p, "format").and_then(CaptionFormat::from_name).unwrap_or_default();
    let name = str_p(p, "name").unwrap_or("Captions").to_string();
    let tid = s.edit_sequence("Create Captions", |q, ctx, st| {
        let tid = TrackId(ctx.alloc());
        let mut t = CaptionTrack::new(tid, name, format);
        t.captions = cues
            .iter()
            .map(|c| Caption {
                id: ClipId(ctx.alloc()),
                start: c.start,
                duration: c.end - c.start,
                text: c.text.clone(),
                speaker: c.speaker.clone(),
                cue_id: None,
                settings: String::new(),
            })
            .collect();
        q.caption_tracks.insert(0, t);
        st.caption_selection.clear();
        Ok(tid)
    })?;
    out["track"] = json!(tid.0);
    Ok(out)
}
