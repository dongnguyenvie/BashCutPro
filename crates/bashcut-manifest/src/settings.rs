//! A plugin's settings: non-secrets in `<data dir>/bashcut-plugins/<id>/settings.json` (the file
//! the plugin reads itself), secrets in the OS key store, passed to the run only as the
//! environment variables the manifest declares. Here (not in the plugin host) so the engine's hooks
//! build the same environment as action runs (RC3, I2); the host re-exports it as `settings`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use bashcut_dirs::child;
use serde_json::Value;

use bashcut_dirs::fs::{Error, Result, err, read_capped, write_atomic};

use crate::{Manifest, Setting};

pub const FILE: &str = "settings.json";
const MAX_BYTES: u64 = 256 * 1024;
const MAX_SECRET: usize = 16 * 1024;
/// How long one key-store call may take before it is killed and fails. The store is called on
/// the UI thread, so a tool waiting on an unlock prompt (`secret-tool` with a locked keyring)
/// must not freeze the app for good; 20 s still leaves time to answer a quick prompt.
const TOOL_TIMEOUT: Duration = Duration::from_secs(20);

pub type Values = BTreeMap<String, Value>;

/// `<plugins dir>/<id>/settings.json`.
pub fn file(plugins_dir: &Path, id: &str) -> PathBuf {
    plugins_dir.join(id).join(FILE)
}

/// The saved values of `m`'s non-secret settings; unknown keys, secrets and bad values are dropped.
pub fn load(plugins_dir: &Path, m: &Manifest) -> Values {
    let Ok(text) = read_capped(&file(plugins_dir, &m.id), MAX_BYTES) else { return Values::new() };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) else { return Values::new() };
    map.into_iter().filter_map(|(k, v)| m.setting(&k).filter(|s| !s.is_secret()).and_then(|s| check(s, &v).ok()).map(|v| (k, v))).collect()
}

/// Merge `values` into the saved settings (a `null` clears one) and write the file.
pub fn save(plugins_dir: &Path, m: &Manifest, values: &Values) -> Result<Values> {
    let mut cur = load(plugins_dir, m);
    for (k, v) in values {
        let Some(s) = m.setting(k) else { return err(format!("{} has no setting `{k}`", m.id)) };
        if s.is_secret() {
            return err(format!("`{k}` is a secret: use the key store (bashcut.plugins.setSecret)"));
        }
        if v.is_null() {
            cur.remove(k);
        } else {
            cur.insert(k.clone(), check(s, v)?);
        }
    }
    let path = file(plugins_dir, &m.id);
    let text = serde_json::to_string_pretty(&cur).map_err(|e| Error(e.to_string()))?;
    write_atomic(&path, text.as_bytes())?;
    Ok(cur)
}

/// `v` as `s`'s type (numbers and bools from strings too, as a form sends them).
pub fn check(s: &Setting, v: &Value) -> Result<Value> {
    let bad = || Error(format!("setting `{}`: expected {}, got {v}", s.id, s.kind));
    let out = match s.kind.as_str() {
        "bool" => Value::Bool(v.as_bool().or_else(|| v.as_str().and_then(|t| t.parse().ok())).ok_or_else(bad)?),
        "integer" => Value::from(v.as_i64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())).ok_or_else(bad)?),
        "number" => {
            let n = v.as_f64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())).filter(|n: &f64| n.is_finite()).ok_or_else(bad)?;
            serde_json::Number::from_f64(n).map(Value::Number).ok_or_else(bad)?
        }
        _ => {
            let t = v.as_str().ok_or_else(bad)?;
            if t.len() > 16 * 1024 {
                return err(format!("setting `{}` is too long", s.id));
            }
            Value::String(t.to_string())
        }
    };
    if let (Some(choices), Some(t)) = (&s.choices, out.as_str())
        && !choices.iter().any(|c| c == t)
    {
        return err(format!("setting `{}` must be one of {}", s.id, choices.join(", ")));
    }
    Ok(out)
}

/// The environment of a run: each set value (and each stored secret) under the setting's first
/// `env` name. Settings without an `env` name are read by the plugin from its settings file.
/// A non-secret setting nobody saved gets its manifest `default` (FX6), unless that default is
/// empty or one of the setting's `env` names is already set in the host's own environment (the
/// child inherits it, and a user's export must not be overridden by a default). Secrets have no default (the manifest
/// refuses one): only a stored value is ever exported.
/// First comes [`DATA_DIR_ENV`] (FX9), so a manifest setting of that name still wins.
pub fn env_for(m: &Manifest, values: &Values, secrets: &dyn SecretStore) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = data_dir_env(bashcut_dirs::data_dir().as_deref()).into_iter().collect();
    env.extend(env_with(m, values, secrets, &|name| std::env::var_os(name).is_some()));
    env
}

/// The variable plugins read their data folder from (`BASHCUTPRO_DATA_DIR`, then `FILMCRAFT_DATA_DIR`,
/// then the per-OS default; plugins #69).
pub const DATA_DIR_ENV: &str = "BASHCUTPRO_DATA_DIR";

/// `BASHCUTPRO_DATA_DIR=<the app's data folder>` for a plugin child (FX9): a plugin cannot see the
/// app's executable folder, so without it a portable install's plugins (`<exe dir>/data`) would write
/// to the per-user folder. [`bashcut_dirs::data_dir`] already prefers a value the user exported, so
/// this never changes an export, only spells out what the app resolved. Nothing without a folder.
pub fn data_dir_env(dir: Option<&Path>) -> Option<(String, String)> {
    dir.filter(|d| !d.as_os_str().is_empty()).map(|d| (DATA_DIR_ENV.to_string(), d.to_string_lossy().into_owned()))
}

/// [`env_for`] with the host environment lookup given (tests).
fn env_with(m: &Manifest, values: &Values, secrets: &dyn SecretStore, host_has: &dyn Fn(&str) -> bool) -> Vec<(String, String)> {
    let text = |v: &Value| match v {
        Value::String(t) => t.clone(),
        other => other.to_string(),
    };
    let mut env = Vec::new();
    for s in &m.settings {
        let Some(name) = s.env.first() else { continue };
        let value = if s.is_secret() {
            secrets.get(&m.id, &s.id).ok().flatten()
        } else if let Some(v) = values.get(&s.id) {
            Some(text(v))
        } else {
            s.default.as_ref().filter(|d| !d.is_null()).map(text).filter(|d| !d.is_empty() && !s.env.iter().any(|n| host_has(n)))
        };
        if let Some(v) = value {
            env.push((name.clone(), v));
        }
    }
    env
}

/// Where secrets live. The OS store in the app, a map in tests.
pub trait SecretStore: Send + Sync {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<String>>;
    fn set(&self, plugin: &str, key: &str, value: &str) -> Result<()>;
    fn delete(&self, plugin: &str, key: &str) -> Result<()>;
}

/// In-memory store (tests, and hosts without an OS store).
#[derive(Default)]
pub struct MemoryStore(Mutex<BTreeMap<(String, String), String>>);

impl SecretStore for MemoryStore {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner).get(&(plugin.to_string(), key.to_string())).cloned())
    }
    fn set(&self, plugin: &str, key: &str, value: &str) -> Result<()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert((plugin.to_string(), key.to_string()), value.to_string());
        Ok(())
    }
    fn delete(&self, plugin: &str, key: &str) -> Result<()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(&(plugin.to_string(), key.to_string()));
        Ok(())
    }
}

/// The OS key store through its command-line tool: `security` (macOS Keychain) or `secret-tool`
/// (Secret Service on Linux). Windows has no such tool in the box, so secrets there come from
/// the environment until a native store lands in `crates/platform`.
#[derive(Default)]
pub struct OsKeyStore;

fn service(plugin: &str) -> String {
    format!("BashCutPro plugin {plugin}")
}

fn tool(program: &str, args: &[&str], stdin: Option<&str>) -> Result<(bool, String)> {
    tool_with(program, args, stdin, TOOL_TIMEOUT)
}

/// [`tool`] with its timeout: stdout is read on a side thread so a full pipe never blocks the
/// wait, and a tool still running at `timeout` is stopped ([`bashcut_dirs::child`]) and reported as an error.
fn tool_with(program: &str, args: &[&str], stdin: Option<&str>, timeout: Duration) -> Result<(bool, String)> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    let opts = child::Opts { stdin: stdin.map(str::to_string), group: false, stderr: false };
    let mut c = child::spawn(cmd, &opts).map_err(|e| Error(format!("key store ({program}): {e}")))?;
    let reader = child::pump("bashcut-key-store", c.stdout.take(), |out| child::read_lines(out, MAX_SECRET, 0, |_, _| {}).0);
    let exit = child::supervise(&mut c, timeout, None);
    if exit.stop.is_some() {
        return Err(Error(format!("key store ({program}): no answer after {} s", timeout.as_secs())));
    }
    let Some(status) = exit.status else { return Err(Error(format!("key store ({program}): lost the tool"))) };
    let stdout = child::collect(&reader, child::STDOUT_WAIT);
    let text = String::from_utf8_lossy(&stdout).trim_end_matches(['\n', '\r']).to_string();
    Ok((status.success(), text))
}

impl SecretStore for OsKeyStore {
    fn get(&self, plugin: &str, key: &str) -> Result<Option<String>> {
        let svc = service(plugin);
        let (ok, text) = if cfg!(target_os = "macos") {
            tool("security", &["find-generic-password", "-s", &svc, "-a", key, "-w"], None)?
        } else if cfg!(windows) {
            return Ok(None);
        } else {
            tool("secret-tool", &["lookup", "service", &svc, "account", key], None)?
        };
        Ok(if ok && !text.is_empty() { Some(text) } else { None })
    }

    fn set(&self, plugin: &str, key: &str, value: &str) -> Result<()> {
        if value.len() > MAX_SECRET || value.contains('\n') {
            return err("secret too long or has a line break");
        }
        let svc = service(plugin);
        let ok = if cfg!(target_os = "macos") {
            // `-w` as the last option reads the password from the terminal / stdin, keeping it off argv
            tool("security", &["add-generic-password", "-U", "-s", &svc, "-a", key, "-w"], Some(&format!("{value}\n{value}\n")))?.0
        } else if cfg!(windows) {
            return err("no OS key store on Windows yet: set the plugin's environment variable instead");
        } else {
            let label = format!("{svc} {key}");
            tool("secret-tool", &["store", "--label", &label, "service", &svc, "account", key], Some(value))?.0
        };
        if ok { Ok(()) } else { err("the key store refused the secret") }
    }

    fn delete(&self, plugin: &str, key: &str) -> Result<()> {
        let svc = service(plugin);
        if cfg!(target_os = "macos") {
            tool("security", &["delete-generic-password", "-s", &svc, "-a", key], None)?;
        } else if !cfg!(windows) {
            tool("secret-tool", &["clear", "service", &svc, "account", key], None)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> Manifest {
        Manifest::parse(
            &json!({"schema": "bashcut-plugin/1", "id": "test.s", "version": "0.1.0", "entry": {"command": ["x"]},
            "settings": [
                {"id": "service", "type": "enum", "choices": ["a", "b"], "default": "a", "env": ["S_SERVICE"]},
                {"id": "n", "type": "integer", "env": ["S_N"]},
                {"id": "apiKey", "type": "secret", "env": ["S_KEY", "OTHER_KEY"]}
            ]})
            .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn saves_checks_and_builds_env() {
        let dir = std::env::temp_dir().join(format!("bashcut-settings-{}", std::process::id()));
        let m = manifest();
        let v = save(&dir, &m, &[("service".to_string(), json!("b")), ("n".to_string(), json!("7"))].into()).unwrap();
        assert_eq!(v["n"], json!(7));
        assert!(save(&dir, &m, &[("service".to_string(), json!("zzz"))].into()).is_err());
        assert!(save(&dir, &m, &[("apiKey".to_string(), json!("k"))].into()).is_err(), "secrets never go to the file");
        assert!(save(&dir, &m, &[("nope".to_string(), json!(1))].into()).is_err());
        std::fs::write(file(&dir, &m.id), r#"{"service":"b","apiKey":"leak","n":"x"}"#).unwrap();
        let loaded = load(&dir, &m);
        assert_eq!(loaded.len(), 1, "secret and bad value dropped: {loaded:?}");
        let store = MemoryStore::default();
        store.set(&m.id, "apiKey", "sk-1").unwrap();
        let env = env_for(&m, &loaded, &store);
        assert_eq!(env, [("S_SERVICE".to_string(), "b".to_string()), ("S_KEY".to_string(), "sk-1".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsaved_settings_export_their_default() {
        // FX6: a default reaches the env when nothing is saved; a saved value still wins
        let m = Manifest::parse(
            &json!({"schema": "bashcut-plugin/1", "id": "test.d", "version": "0.1.0", "entry": {"command": ["x"]},
            "settings": [
                {"id": "service", "type": "enum", "choices": ["a", "b"], "default": "a", "env": ["D_SERVICE", "D_ALT"]},
                {"id": "n", "type": "integer", "default": 3, "env": ["D_N"]},
                {"id": "on", "type": "bool", "default": true, "env": ["D_ON"]},
                {"id": "path", "type": "string", "default": "", "env": ["D_PATH"]},
                {"id": "nodefault", "type": "string", "env": ["D_NONE"]},
                {"id": "fileOnly", "type": "string", "default": "x"},
                {"id": "apiKey", "type": "secret", "env": ["D_KEY"]}
            ]})
            .to_string(),
        )
        .unwrap();
        let none = |_: &str| false;
        let store = MemoryStore::default();
        let pairs = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(env_with(&m, &Values::new(), &store, &none), pairs(&[("D_SERVICE", "a"), ("D_N", "3"), ("D_ON", "true")]));
        let saved: Values = [("service".to_string(), json!("b")), ("on".to_string(), json!(false)), ("path".to_string(), json!("/p"))].into();
        store.set(&m.id, "apiKey", "sk").unwrap();
        assert_eq!(env_with(&m, &saved, &store, &none), pairs(&[("D_SERVICE", "b"), ("D_N", "3"), ("D_ON", "false"), ("D_PATH", "/p"), ("D_KEY", "sk")]));
        // the user's own export is inherited by the child: a default never overrides it, a saved value does
        let host = |n: &str| n == "D_N" || n == "D_ALT";
        assert_eq!(env_with(&m, &saved, &store, &host), pairs(&[("D_SERVICE", "b"), ("D_ON", "false"), ("D_PATH", "/p"), ("D_KEY", "sk")]));
        assert_eq!(env_with(&m, &Values::new(), &store, &host), pairs(&[("D_ON", "true"), ("D_KEY", "sk")]), "a later env name counts too");
        // no stored secret: nothing exported for it (a secret never has a default)
        store.delete(&m.id, "apiKey").unwrap();
        assert!(env_with(&m, &Values::new(), &store, &none).iter().all(|(k, _)| k != "D_KEY"));
    }

    #[cfg(unix)]
    #[test]
    fn key_store_tool_times_out() {
        let start = std::time::Instant::now();
        let e = tool_with("sleep", &["5"], None, Duration::from_millis(100)).unwrap_err();
        assert!(e.0.contains("key store (sleep)") && e.0.contains("no answer"), "{e}");
        assert!(start.elapsed() < child::GRACE, "stopped softly (TERM), not waited out: {:?}", start.elapsed());
        // a tool that answers in time: its stdout without the line break, and its exit
        assert_eq!(tool_with("sh", &["-c", "cat; echo; exit 1"], Some("sk-1"), Duration::from_secs(10)), Ok((false, "sk-1".to_string())));
        assert!(tool_with("/nonexistent/bashcut-key-tool", &[], None, Duration::from_secs(1)).unwrap_err().0.contains("key store"));
    }

    #[test]
    #[ignore = "test ticket: data_dir_env gives BASHCUTPRO_DATA_DIR=<dir>, None for no or an empty folder; env_for puts it first so a manifest setting named BASHCUTPRO_DATA_DIR wins"]
    fn plugin_env_carries_the_data_dir() {}
}
