//! The speech model a batch transcribes with (#172). `filmcraft-cli` does not read the app's
//! `preferences.json`, so Settings ▸ Media Analysis & Transcription ▸ Speech model would be
//! ignored and every transcribe step would need the engine's default model. The runner picks the
//! model once per run and passes it as `model` to transcribe steps that name none:
//! `--speech-model`, else the Settings choice when it is downloaded, else the engine's default
//! when it is downloaded, else the one downloaded model (the first in the catalogue when several),
//! else the Settings choice (the step then fails with the download command, see `runner`).
//!
//! Each transcribe line is followed by `bashcut.transcript.clean` (#177): Whisper's repeated-text loops on
//! music or ambience are dropped before captions or cuts read the transcript. `"keepDoubtful": true` on the
//! line keeps them.

use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Commands that transcribe with a speech model and take a `model` param.
pub const TRANSCRIBE_COMMANDS: [&str; 2] = ["sequence.transcribe", "transcript.generate"];

/// Biggest `preferences.json` read (the app's file is a few KB).
const MAX_PREFS: u64 = 4 << 20;

/// Biggest `transcript.models` answer read (a few KB in practice).
const MAX_MODELS: u64 = 4 << 20;

/// How long the engine may take to list its speech models.
const MODELS_TIMEOUT: Duration = Duration::from_secs(30);

/// The speech model chosen in the app's Settings (`mediaAnalysis.whisperModel` in
/// `<data dir>/preferences.json`).
pub fn settings_model(data_dir: &Path) -> Option<String> {
    let path = data_dir.join("preferences.json");
    if std::fs::metadata(&path).ok()?.len() > MAX_PREFS {
        return None;
    }
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    v.get("mediaAnalysis")?.get("whisperModel")?.as_str().map(str::trim).filter(|m| !m.is_empty()).map(str::to_string)
}

/// The engine's `transcript.models` answer, `None` when it gives none or takes longer than [`MODELS_TIMEOUT`]
/// (I7: a stuck engine must not hold up the run's start). No stdin, so it cannot wait on a prompt.
pub fn engine_models(cli: &Path, data_dir_args: &[String], env: &[(String, String)]) -> Option<Value> {
    engine_models_within(cli, data_dir_args, env, MODELS_TIMEOUT)
}

fn engine_models_within(cli: &Path, data_dir_args: &[String], env: &[(String, String)], timeout: Duration) -> Option<Value> {
    let mut child = Command::new(cli)
        .args(["exec", "transcript.models"])
        .args(data_dir_args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // read on the side so a full pipe cannot block the wait below
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(MAX_MODELS).read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = reader.join().ok()?;
    if !status.success() {
        return None;
    }
    serde_json::from_slice(&out).ok()
}

/// The model to pass (see the module doc); `None` leaves the engine's default.
pub fn choose(settings: Option<&str>, models: Option<&Value>) -> Option<String> {
    let Some(models) = models else { return settings.map(str::to_string) };
    let installed: Vec<&str> = models
        .get("models")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter().filter(|m| m.get("installed").and_then(Value::as_bool) == Some(true)).filter_map(|m| m.get("id").and_then(Value::as_str)).collect()
        })
        .unwrap_or_default();
    let has = |id: &str| installed.contains(&id);
    if let Some(s) = settings.filter(|s| has(s)) {
        return Some(s.to_string());
    }
    if models.get("default").and_then(Value::as_str).is_some_and(has) {
        return None;
    }
    installed.first().map(|s| s.to_string()).or_else(|| settings.map(str::to_string))
}

/// `params` with `model` set, for a transcribe command that names none.
pub fn with_model(id: &str, params: &Value, model: Option<&str>) -> Value {
    let (Some(model), true) = (model, TRANSCRIBE_COMMANDS.contains(&id)) else { return params.clone() };
    match params {
        Value::Object(map) if map.get("model").is_some_and(|m| !m.is_null()) => params.clone(),
        Value::Object(map) => {
            let mut map = map.clone();
            map.insert("model".into(), Value::String(model.to_string()));
            Value::Object(map)
        }
        Value::Null => serde_json::json!({"model": model}),
        _ => params.clone(),
    }
}

/// The clean line to run after `id` / `params` (see the module doc), with the transcribed `items` when named.
pub fn clean_after(id: &str, params: &Value) -> Option<(String, Value)> {
    if !TRANSCRIBE_COMMANDS.contains(&id) || params.get("keepDoubtful").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let mut clean = serde_json::Map::new();
    if id == "transcript.generate"
        && let Some(items) = params.get("items").filter(|v| v.is_array()).or_else(|| params.get("item").filter(|v| v.is_u64()))
    {
        clean.insert(if items.is_array() { "items" } else { "item" }.into(), items.clone());
    }
    if let Some(n) = params.get("minRepeats").filter(|v| v.is_u64()) {
        clean.insert("minRepeats".into(), n.clone());
    }
    Some(("bashcut.transcript.clean".into(), Value::Object(clean)))
}

/// A transcribe line's `params` without the keys only the clean reads; other lines' as they are.
pub fn without_clean_keys(id: &str, params: &Value) -> Value {
    let mut p = params.clone();
    if let (true, Some(o)) = (TRANSCRIBE_COMMANDS.contains(&id), p.as_object_mut()) {
        o.remove("keepDoubtful");
        o.remove("minRepeats");
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn models(installed: &[&str]) -> Value {
        let list: Vec<Value> =
            ["whisper-tiny", "whisper-base", "whisper-small"].iter().map(|id| json!({"id": id, "installed": installed.contains(id)})).collect();
        json!({"available": true, "default": "whisper-base", "models": list})
    }

    #[test]
    fn the_settings_model_wins_when_downloaded() {
        assert_eq!(choose(Some("whisper-small"), Some(&models(&["whisper-base", "whisper-small"]))).as_deref(), Some("whisper-small"));
    }

    #[test]
    fn else_the_default_else_the_downloaded_one() {
        assert_eq!(choose(Some("whisper-small"), Some(&models(&["whisper-base"]))), None);
        assert_eq!(choose(None, Some(&models(&["whisper-tiny"]))).as_deref(), Some("whisper-tiny"));
        assert_eq!(choose(Some("whisper-small"), Some(&models(&["whisper-tiny"]))).as_deref(), Some("whisper-tiny"));
        // nothing downloaded: the Settings choice, so the error names the model the user picked
        assert_eq!(choose(Some("whisper-small"), Some(&models(&[]))).as_deref(), Some("whisper-small"));
        assert_eq!(choose(None, Some(&models(&[]))), None);
        assert_eq!(choose(Some("whisper-small"), None).as_deref(), Some("whisper-small"));
        assert_eq!(choose(None, Some(&json!({"weird": 1}))), None);
    }

    #[test]
    fn only_transcribe_steps_without_a_model_get_one() {
        let m = Some("whisper-tiny");
        assert_eq!(with_model("sequence.transcribe", &json!({"language": "auto"}), m), json!({"language": "auto", "model": "whisper-tiny"}));
        assert_eq!(with_model("transcript.generate", &Value::Null, m), json!({"model": "whisper-tiny"}));
        assert_eq!(with_model("sequence.transcribe", &json!({"model": "whisper-base"}), m), json!({"model": "whisper-base"}));
        assert_eq!(with_model("captions.export", &json!({}), m), json!({}));
        assert_eq!(with_model("sequence.transcribe", &json!({}), None), json!({}));
    }

    #[test]
    fn transcribe_lines_are_followed_by_the_clean() {
        let (id, p) = clean_after("transcript.generate", &json!({"items": [3, 4], "minRepeats": 4})).unwrap();
        assert_eq!(id, "bashcut.transcript.clean");
        assert_eq!(p, json!({"items": [3, 4], "minRepeats": 4}));
        assert_eq!(clean_after("transcript.generate", &json!({"item": 7})).unwrap().1, json!({"item": 7}));
        // sequence.transcribe has no item list: every recogniser transcript is cleaned
        assert_eq!(clean_after("sequence.transcribe", &json!({"track": "A1"})).unwrap().1, json!({}));
        assert_eq!(clean_after("sequence.transcribe", &json!({"keepDoubtful": true})), None);
        assert_eq!(clean_after("captions.export", &json!({})), None);
        assert_eq!(without_clean_keys("transcript.generate", &json!({"keepDoubtful": false, "minRepeats": 3, "model": "m"})), json!({"model": "m"}));
        let ours = json!({"keepDoubtful": true});
        assert_eq!(without_clean_keys("bashcut.transcript.generate", &ours), ours);
    }

    #[test]
    fn settings_are_read_from_preferences_json() {
        let dir = std::env::temp_dir().join(format!("bashcut-batch-speech-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(settings_model(&dir), None);
        std::fs::write(dir.join("preferences.json"), r#"{"mediaAnalysis": {"whisperModel": "whisper-small"}}"#).unwrap();
        assert_eq!(settings_model(&dir).as_deref(), Some("whisper-small"));
        std::fs::write(dir.join("preferences.json"), "not json").unwrap();
        assert_eq!(settings_model(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn engine_models_times_out_on_a_stuck_engine() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("bashcut-batch-speech-models-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            let p = dir.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        let stuck = script("stuck", "exec sleep 30");
        let start = Instant::now();
        assert_eq!(engine_models_within(&stuck, &[], &[], Duration::from_millis(300)), None);
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
        // no stdin: a read gets end of file at once instead of waiting on a prompt
        let reads = script("reads", r#"read answer; printf '["%s %s %s"]' "$1" "$2" "$answer""#);
        assert_eq!(engine_models_within(&reads, &[], &[], Duration::from_secs(10)), Some(json!(["exec transcript.models "])));
        let fails = script("fails", "echo '[]'; exit 3");
        assert_eq!(engine_models_within(&fails, &[], &[], Duration::from_secs(10)), None);
        assert_eq!(engine_models(&dir.join("missing"), &[], &[]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
