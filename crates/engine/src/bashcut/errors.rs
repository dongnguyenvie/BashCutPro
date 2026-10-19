//! Typed errors and units for every `bashcut.*` command (#50).
//!
//! Every error a `bashcut.*` command returns starts with one code, then the sentence the UI shows:
//! `badParams: invalid parameters for `bashcut.audio.loudness`: …`. Codes:
//!
//! | code | meaning | agent's move |
//! |---|---|---|
//! | `notFound` | a clip, track, item, file, preset or command named in the params does not exist | re-read (`bashcut.timeline.summary`), fix the id |
//! | `refused` | the request is valid but cannot be done now (no sequence, locked track, nothing selected, an edit rule) | change the state first, or ask the user |
//! | `busy` | a job or another edit is in progress | wait (`jobs.list`) and retry |
//! | `badParams` | a param is missing, of the wrong type or out of range | fix the params (see `command_list`) |
//! | `offline` | media is missing or offline, a service or a model is not reachable | relink / install, or tell the user |
//!
//! The codes are applied in one place, [`wrap`], to every spec [`super::commands`] returns, so each feature
//! file just returns errors as usual (`commands::bad`, `EngineError::…`) and may name a code itself with
//! [`fail`]. `command_list` errors from `enabled()` (upstream's `Disabled`) are not ours to type: they read
//! "command … is not available right now" and mean `refused`.
//!
//! [`units`] appends a units note to a params string that does not state one.

use std::sync::OnceLock;

use serde_json::Value;

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

/// What kind of failure an error is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    NotFound,
    Refused,
    Busy,
    BadParams,
    Offline,
}

impl Code {
    pub const ALL: [Code; 5] = [Code::NotFound, Code::Refused, Code::Busy, Code::BadParams, Code::Offline];

    pub fn name(self) -> &'static str {
        match self {
            Code::NotFound => "notFound",
            Code::Refused => "refused",
            Code::Busy => "busy",
            Code::BadParams => "badParams",
            Code::Offline => "offline",
        }
    }
}

/// An error with an explicit code: `<code>: <cmd>: <msg>`.
pub fn fail(code: Code, cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::Other(format!("{}: {cmd}: {}", code.name(), msg.into()))
}

/// The code an error text starts with, if any.
pub fn tagged(text: &str) -> Option<Code> {
    let head = text.split(':').next()?;
    Code::ALL.into_iter().find(|c| c.name() == head)
}

fn mentions(text: &str, words: &[&str]) -> bool {
    let t = text.to_ascii_lowercase();
    words.iter().any(|w| t.contains(w))
}

const NOT_FOUND: &[&str] =
    &["no clip", "no track", "no item", "no sequence ", "no such", "not found", "unknown ", "no marker", "no preset", "no file", "does not exist"];
const BUSY: &[&str] = &["busy", "in progress", "still running", "already running"];
const OFFLINE: &[&str] = &["offline", "missing media", "not reachable", "unreachable", "no network", "model is not installed"];

/// The code of any engine error.
pub fn code_of(e: &EngineError) -> Code {
    match e {
        EngineError::BadParams { msg, .. } => {
            if mentions(msg, NOT_FOUND) {
                Code::NotFound
            } else {
                Code::BadParams
            }
        }
        EngineError::UnknownCommand(_) => Code::NotFound,
        EngineError::Media(_) => Code::Offline,
        EngineError::Other(m) => tagged(m).unwrap_or_else(|| {
            if mentions(m, BUSY) {
                Code::Busy
            } else if mentions(m, OFFLINE) {
                Code::Offline
            } else if mentions(m, NOT_FOUND) {
                Code::NotFound
            } else {
                Code::Refused
            }
        }),
        // Disabled, NoSequence, Edit (and anything upstream adds): the request cannot be done now
        _ => Code::Refused,
    }
}

/// `r` with its error typed: unchanged when it already starts with a code.
pub fn typed(r: Result<Value>) -> Result<Value> {
    r.map_err(|e| match &e {
        EngineError::Other(m) if tagged(m).is_some() => e,
        _ => EngineError::Other(format!("{}: {e}", code_of(&e).name())),
    })
}

type Run = fn(&mut Session, &Value) -> Result<Value>;

/// The original `run` of each wrapped command, by position (filled once by [`wrap`]).
static RUNS: OnceLock<Vec<(&'static str, Run)>> = OnceLock::new();

fn slot<const N: usize>(s: &mut Session, p: &Value) -> Result<Value> {
    match RUNS.get().and_then(|r| r.get(N)) {
        Some((_, run)) => typed(run(s, p)),
        None => Err(fail(Code::Refused, "bashcut", "the command table is not ready")),
    }
}

macro_rules! slots {
    ($($n:literal)*) => { [$(slot::<$n> as Run),*] };
}

/// One typed wrapper per position; commands past the last one keep their own `run` (a test fails then).
const SLOTS: [Run; 96] = slots!(
    0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
    32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63
    64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95
);

/// Ids whose `run` [`wrap`] replaced with a typed wrapper.
pub fn typed_ids() -> Vec<&'static str> {
    RUNS.get().map(|r| r.iter().take(SLOTS.len()).map(|(id, _)| *id).collect()).unwrap_or_default()
}

/// Give every spec typed errors and a units note.
pub fn wrap(specs: Vec<CommandSpec>) -> Vec<CommandSpec> {
    let table = RUNS.get_or_init(|| specs.iter().map(|c| (c.id, c.run)).collect());
    // the list is built the same way every time; if it ever is not, keep the specs as they are rather
    // than call the wrong command
    let same = table.len() == specs.len() && table.iter().zip(&specs).all(|((id, _), c)| *id == c.id);
    specs
        .into_iter()
        .enumerate()
        .map(|(i, mut c)| {
            if same && let Some(run) = SLOTS.get(i) {
                c.run = *run;
            }
            c.params = units(c.id, c.params);
            c
        })
        .collect()
}

/// Words that show a params string states its units.
const UNIT_WORDS: &[&str] = &["seconds", "ticks", "frames", "lufs", "dbtp", "pixels", "units"];

/// Units notes for commands whose params name no time or level.
const NOTES: &[(&str, &str)] = &[
    ("bashcut.template.save", "units: none (paths, names and item ids)"),
    ("bashcut.template.instantiate", "units: none (paths and names)"),
    ("bashcut.template.list", "units: none"),
    ("bashcut.profiles.apply", "units: none here; the profile sets pixels, kbit/s and LUFS"),
];

/// `params` with a units note appended when it states none.
pub fn units(id: &'static str, params: &'static str) -> &'static str {
    if mentions(params, UNIT_WORDS) {
        return params;
    }
    static CACHE: OnceLock<std::sync::Mutex<Vec<(&'static str, &'static str)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, p)) = cache.iter().find(|(k, _)| *k == id) {
        return p;
    }
    let note = NOTES.iter().find(|(k, _)| *k == id).map(|(_, n)| *n).unwrap_or("units: none");
    // leaked once per id (a few dozen short strings for the life of the process)
    let full: &'static str = Box::leak(format!("{params} ({note})").into_boxed_str());
    cache.push((id, full));
    full
}
