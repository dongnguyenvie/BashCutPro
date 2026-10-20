//! The speech model a batch transcribes with (#172). `filmcraft-cli` does not read the app's
//! `preferences.json`, so Settings ▸ Media Analysis & Transcription ▸ Speech model would be
//! ignored and every transcribe step would need the engine's default model. The runner picks the
//! model once per run and passes it as `model` to transcribe steps that name none:
//! `--speech-model`, else the Settings choice when it is downloaded, else the engine's default
//! when it is downloaded, else the one downloaded model (the first in the catalogue when several),
//! else the Settings choice (the step then fails with the download command, see `runner`).

use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// Commands that transcribe with a speech model and take a `model` param.
pub const TRANSCRIBE_COMMANDS: [&str; 2] = ["sequence.transcribe", "transcript.generate"];

/// Biggest `preferences.json` read (the app's file is a few KB).
const MAX_PREFS: u64 = 4 << 20;

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

/// The engine's `transcript.models` answer, `None` when it gives none.
pub fn engine_models(cli: &Path, data_dir_args: &[String], env: &[(String, String)]) -> Option<Value> {
    let out = Command::new(cli).args(["exec", "transcript.models"]).args(data_dir_args).envs(env.iter().map(|(k, v)| (k, v))).output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
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
}
