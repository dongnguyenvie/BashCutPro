//! Doubtful-text filter for Whisper transcripts (#177). On music or ambience with no speech, Whisper can decode
//! the same sentence again and again ("I'm going to have to go to the beach." for 25 s of wind). Upstream's
//! decoder guards only catch a loop of a few tokens inside one window, so a sentence repeated across windows
//! (or longer than the guard's pattern) lands in the transcript, and captions, text cuts and the agent's
//! story build on it. `crates/speech` stays as upstream ships it; this filter runs on the decoded words.
//!
//! | command | what it does |
//! |---|---|
//! | `bashcut.transcript.clean` | drops doubtful words from transcripts already made (one undo step) |
//! | `bashcut.transcript.generate` | `transcript.generate`, then the clean, as one undo step |
//!
//! A word is doubtful when it is part of
//! - a run of the same phrase back to back: 2–24 words repeated `minRepeats` (3) times or more in a row, or one
//!   word 6 times or more;
//! - a sentence of 3 words or more that occurs `minRepeats` times or more anywhere in the item's transcript
//!   (the loop Whisper writes once per 30 s window).
//!
//! When half or more of an item's words are doubtful, the rest is the same hallucination's tail ("I'm sorry."):
//! the whole transcript is dropped and the item reports `noSpeech`. Its transcript stays, empty, so the item
//! reads as transcribed. Transcripts made by hand (`imported`, `manual`) are never touched unless named.

use std::collections::HashMap;
use std::sync::Arc;

use filmcraft_project::{ItemId, Transcript, Word, transcript::normalize_word};
use serde_json::{Map, Value, json};

use super::analysis::secs;
use crate::commands::{CommandSpec, always, bad, bool_p, u64_p};
use crate::{Result, Session};

const CLEAN: &str = "bashcut.transcript.clean";
const GENERATE: &str = "bashcut.transcript.generate";
/// Longest phrase looked for in a back-to-back run, in words.
const MAX_PHRASE: usize = 24;
/// Words checked per transcript; a longer one is checked up to here (a 4 h talk is ~40 000 words).
const MAX_WORDS: usize = 200_000;
/// Doubtful stretches listed per item in a result.
const MAX_LISTED: usize = 20;
/// Default repeats that make a phrase doubtful.
pub const MIN_REPEATS: usize = 3;
/// A single word repeated this many times in a row is doubtful ("the the the …").
const MIN_WORD_REPEATS: usize = 6;
/// A sentence ends at a word ending in one of these, or before a pause this long (seconds).
const SENTENCE_GAP_SECONDS: f64 = 1.5;
/// Shortest sentence the repeated-sentence check counts ("Thank you." said three times is speech).
const MIN_SENTENCE_WORDS: usize = 3;
/// Sentences longer than this many words are split here (no punctuation).
const MAX_SENTENCE: usize = 40;

/// Why a word was dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// A phrase repeated back to back.
    Loop,
    /// The same sentence many times across the transcript.
    Repeated,
    /// Kept text in a transcript that was mostly doubtful.
    NoSpeech,
}

impl Reason {
    fn name(self) -> &'static str {
        match self {
            Reason::Loop => "loop",
            Reason::Repeated => "repeatedSentence",
            Reason::NoSpeech => "noSpeech",
        }
    }
}

/// What [`find_doubtful`] decided for one transcript.
#[derive(Debug, Default)]
pub struct Verdict {
    /// One entry per word: `Some` when it is dropped.
    pub drop: Vec<Option<Reason>>,
    /// The whole transcript is a hallucination.
    pub no_speech: bool,
}

impl Verdict {
    pub fn dropped(&self) -> usize {
        self.drop.iter().filter(|d| d.is_some()).count()
    }
}

/// Mark the doubtful words of `words` (see the module docs). `min_repeats` is clamped to 2..=100.
pub fn find_doubtful(words: &[Word], min_repeats: usize) -> Verdict {
    let min_repeats = min_repeats.clamp(2, 100);
    let n = words.len().min(MAX_WORDS);
    let norm: Vec<String> = words.iter().take(n).map(|w| normalize_word(&w.text)).collect();
    let mut drop: Vec<Option<Reason>> = vec![None; words.len()];

    // back-to-back runs of one phrase
    let mut i = 0;
    while i < n {
        let mut best: Option<usize> = None;
        for len in 1..=MAX_PHRASE.min(n - i) {
            let Some(phrase) = norm.get(i..i + len) else { break };
            // a phrase of one word said twice is the one-word case, with its own threshold
            if phrase.iter().all(String::is_empty) || (len > 1 && phrase.iter().all(|w| Some(w) == phrase.first())) {
                continue;
            }
            let mut reps = 1;
            while norm.get(i + reps * len..i + (reps + 1) * len).is_some_and(|next| next == phrase) {
                reps += 1;
            }
            let needed = if len == 1 { MIN_WORD_REPEATS } else { min_repeats };
            if reps >= needed {
                best = Some(best.map_or(len * reps, |b: usize| b.max(len * reps)));
            }
        }
        match best {
            Some(span) => {
                for d in drop.iter_mut().skip(i).take(span) {
                    *d = Some(Reason::Loop);
                }
                i += span;
            }
            None => i += 1,
        }
    }

    // the same sentence many times anywhere
    let sentences = sentences(words, n);
    let mut counts: HashMap<&[String], usize> = HashMap::new();
    for r in &sentences {
        let Some(key) = norm.get(r.clone()) else { continue };
        if key.iter().filter(|w| !w.is_empty()).count() >= MIN_SENTENCE_WORDS {
            *counts.entry(key).or_default() += 1;
        }
    }
    for r in &sentences {
        if norm.get(r.clone()).and_then(|k| counts.get(k)).is_some_and(|c| *c >= min_repeats) {
            for d in drop.iter_mut().skip(r.start).take(r.len()) {
                d.get_or_insert(Reason::Repeated);
            }
        }
    }

    let mut v = Verdict { drop, no_speech: false };
    if !words.is_empty() && v.dropped() * 2 >= words.len() {
        v.no_speech = true;
        for d in &mut v.drop {
            d.get_or_insert(Reason::NoSpeech);
        }
    }
    v
}

/// Word index ranges of the sentences among the first `n` words.
fn sentences(words: &[Word], n: usize) -> Vec<std::ops::Range<usize>> {
    let gap = filmcraft_time::Tick::from_seconds_f64(SENTENCE_GAP_SECONDS);
    let mut out = Vec::new();
    let mut start = 0;
    for i in 0..n {
        let Some(w) = words.get(i) else { break };
        let ends = w.text.trim_end_matches(['"', '\'', ')', '»', '”', '’']).ends_with(['.', '!', '?', '…', '。', '！', '？']);
        let paused = words.get(i + 1).is_some_and(|next| next.start.0.saturating_sub(w.end.0) >= gap.0);
        if ends || paused || i + 1 == n || i + 1 - start >= MAX_SENTENCE {
            out.push(start..i + 1);
            start = i + 1;
        }
    }
    out
}

/// Doubtful stretches of `words` for a result: consecutive dropped words with the same reason, merged.
fn stretches(words: &[Word], v: &Verdict) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut i = 0;
    while i < words.len() && out.len() < MAX_LISTED {
        let Some(reason) = v.drop.get(i).copied().flatten() else {
            i += 1;
            continue;
        };
        let mut j = i;
        while v.drop.get(j + 1).copied().flatten() == Some(reason) {
            j += 1;
        }
        let (Some(span), Some(first), Some(last)) = (words.get(i..=j), words.get(i), words.get(j)) else { break };
        let mut text = span.iter().take(30).map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        if span.len() > 30 {
            text.push_str(" …");
        }
        out.push(json!({"start": secs(first.start), "end": secs(last.end), "words": j + 1 - i, "reason": reason.name(), "text": text}));
        i = j + 1;
    }
    out
}

/// Transcripts to clean: `items` / `item`, else every transcript a recogniser made.
fn targets(s: &Session, p: &Value) -> Vec<ItemId> {
    let named: Option<Vec<ItemId>> = p
        .get("items")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).map(ItemId).collect())
        .or_else(|| u64_p(p, "item").map(|i| vec![ItemId(i)]));
    match named {
        Some(v) => v.into_iter().filter_map(|i| s.project.resolve_media(i).map(|(root, _, _)| root)).collect(),
        None => s.project.transcripts.iter().filter(|(_, t)| !matches!(t.source.as_str(), "imported" | "manual")).map(|(i, _)| *i).collect(),
    }
}

fn min_repeats_p(p: &Value) -> usize {
    u64_p(p, "minRepeats").map_or(MIN_REPEATS, |n| n.min(100) as usize)
}

/// Clean `items`' transcripts; one undo step when anything is dropped. Returns the per-item report.
fn clean_items(s: &mut Session, items: &[ItemId], min_repeats: usize, dry_run: bool) -> Result<Value> {
    let mut report = Vec::new();
    let mut cleaned: Vec<(ItemId, Transcript)> = Vec::new();
    let mut no_speech = Vec::new();
    for &item in items {
        let Some(t) = s.project.transcripts.get(&item) else { continue };
        let v = find_doubtful(&t.words, min_repeats);
        let dropped = v.dropped();
        let mut o = Map::new();
        o.insert("item".into(), json!(item.0));
        o.insert("words".into(), json!(t.words.len()));
        o.insert("dropped".into(), json!(dropped));
        o.insert("kept".into(), json!(t.words.len() - dropped));
        if v.no_speech {
            o.insert("noSpeech".into(), json!(true));
            no_speech.push(item.0);
        }
        if dropped > 0 {
            o.insert("doubtful".into(), Value::Array(stretches(&t.words, &v)));
            let mut t2 = (**t).clone();
            t2.words = t.words.iter().zip(&v.drop).filter(|(_, d)| d.is_none()).map(|(w, _)| w.clone()).collect();
            if t2.words.is_empty() {
                t2.speakers.clear();
            }
            cleaned.push((item, t2));
        }
        report.push(Value::Object(o));
    }
    let dropped_any = !cleaned.is_empty();
    if dropped_any && !dry_run {
        s.edit("Clean Transcript", move |pr, _| {
            for (i, t) in cleaned {
                pr.transcripts.insert(i, Arc::new(t));
            }
            Ok(())
        })?;
    }
    let mut out = json!({"items": report, "dryRun": dry_run});
    if !no_speech.is_empty() {
        let which = no_speech.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
        out["warning"] = json!(format!(
            "No speech found in item {which}: Whisper wrote only repeated text (a loop on music or ambience), so the transcript was left empty. \
             Pass keepDoubtful: true to keep it, or a larger minRepeats to loosen the filter."
        ));
    }
    Ok(out)
}

fn clean_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(v) = p.get("minRepeats").filter(|v| !v.is_null())
        && v.as_u64().is_none_or(|n| n < 2)
    {
        return Err(bad(CLEAN, "`minRepeats` must be a whole number of 2 or more"));
    }
    let items = targets(s, p);
    clean_items(s, &items, min_repeats_p(p), bool_p(p, "dryRun").unwrap_or(false))
}

fn generate_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(v) = p.get("minRepeats").filter(|v| !v.is_null())
        && v.as_u64().is_none_or(|n| n < 2)
    {
        return Err(bad(GENERATE, "`minRepeats` must be a whole number of 2 or more"));
    }
    let keep = bool_p(p, "keepDoubtful").unwrap_or(false);
    let mut params = p.clone();
    if let Some(o) = params.as_object_mut() {
        for k in ["keepDoubtful", "minRepeats", "dryRun"] {
            o.remove(k);
        }
    }
    let min_repeats = min_repeats_p(p);
    s.edit_group("Transcribe", |s| {
        let mut r = s.execute("transcript.generate", params)?;
        if keep {
            return Ok(r);
        }
        let items: Vec<ItemId> = r.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(|i| i.get("item")?.as_u64()).map(ItemId).collect();
        let c = clean_items(s, &items, min_repeats, false)?;
        // the generate report gains each item's dropped count; noSpeech items say so
        if let Some(list) = r.get_mut("items").and_then(Value::as_array_mut) {
            for it in list.iter_mut() {
                let id = it.get("item").and_then(Value::as_u64);
                if let Some(row) = c["items"].as_array().into_iter().flatten().find(|row| row["item"].as_u64() == id) {
                    it["words"] = row["kept"].clone();
                    it["dropped"] = row["dropped"].clone();
                    for k in ["noSpeech", "doubtful"] {
                        if let Some(v) = row.get(k) {
                            it[k] = v.clone();
                        }
                    }
                }
            }
        }
        if let Some(w) = c.get("warning") {
            r["warning"] = w.clone();
        }
        Ok(r)
    })
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: GENERATE,
            label: "Transcribe (Drop Doubtful Text)",
            menu: &[],
            shortcut: None,
            params: r#"{…transcript.generate params, "keepDoubtful":bool=false,"minRepeats":n=3}  (transcribes, then drops Whisper's repeated-text loops; an item with only loops reports noSpeech and keeps an empty transcript)"#,
            enabled: crate::transcript::can_transcribe,
            run: generate_cmd,
            journal: true,
        },
        CommandSpec {
            id: CLEAN,
            label: "Drop Doubtful Transcript Text",
            menu: &[],
            shortcut: None,
            params: r#"{"items":[id]? (default every transcript a recogniser made),"minRepeats":n=3,"dryRun":bool=false}  (doubtful stretches: start / end in media seconds)"#,
            enabled: always,
            run: clean_cmd,
            journal: true,
        },
    ]
}
