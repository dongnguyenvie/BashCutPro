//! The plugin host as the app holds it (`bashcut/plugin-host.md`): one [`Host`] per app in egui
//! context data (no field in `FilmcraftApp`), polled each frame, and the `bashcut.plugins.*`
//! commands that the Plugins and Runs sections, shortcuts, the control channel and MCP share.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use bashcut_plugin_host::host::RunContext;
use bashcut_plugin_host::settings::{OsKeyStore, SecretStore};
use bashcut_plugin_host::{Host, Result as HostResult};
use filmcraft_engine::shortcuts::CommandInfo;
use serde_json::{Value, json};

use crate::FilmcraftApp;

pub type Shared = Arc<Mutex<Host>>;

fn key() -> egui::Id {
    egui::Id::new("bashcut.plugins.host")
}

/// This app's host, created on first use with the session's data dir. Its secrets come from a
/// [`CachedKeyStore`] warmed on a side thread, so frames never wait on the OS key store.
pub fn host(app: &FilmcraftApp, ctx: &egui::Context) -> Shared {
    super::shared::get_or_init(ctx, key(), || {
        let store = Arc::new(CachedKeyStore::default());
        ctx.data_mut(|d| d.insert_temp(store_key(), store.clone()));
        let host = Host::new(super::data_dir(app), store.clone());
        let keys = host
            .installed()
            .iter()
            .filter_map(|p| p.manifest.as_ref())
            .flat_map(|m| m.settings.iter().filter(|s| s.is_secret()).map(|s| (m.id.clone(), s.id.clone())))
            .collect();
        store.warm(keys);
        host
    })
}

fn store_key() -> egui::Id {
    egui::Id::new("bashcut.plugins.keyStore")
}

/// What a background secret write said: `None` while it runs.
pub type Pending = Arc<Mutex<Option<Result<(), String>>>>;

/// Store (`Some`) or delete (`None`) a plugin's secret on a side thread, so a click never waits on
/// an unlock prompt (RC13). The setting is checked here; the returned slot fills with the outcome.
pub fn set_secret_async(app: &FilmcraftApp, ctx: &egui::Context, plugin: &str, setting: &str, value: Option<String>) -> Result<Pending, String> {
    let known = with(app, ctx, |h| h.plugin(plugin).and_then(|p| p.manifest.as_ref()).and_then(|m| m.setting(setting)).is_some_and(|s| s.is_secret()));
    if !known {
        return Err(format!("{plugin} has no secret `{setting}`"));
    }
    let store = ctx.data(|d| d.get_temp::<Arc<CachedKeyStore>>(store_key())).ok_or("the plugin host is not ready")?;
    let slot: Pending = Arc::new(Mutex::new(None));
    let out = slot.clone();
    let (plugin, setting, value) = (plugin.to_string(), setting.to_string(), value.filter(|v| !v.is_empty()));
    let ctx2 = ctx.clone();
    let started = std::thread::Builder::new().name("bashcut-key-store-write".into()).spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &value {
            Some(v) => store.set(&plugin, &setting, v),
            None => store.delete(&plugin, &setting),
        }));
        let r = match r {
            Ok(r) => r.map_err(|e| e.0),
            Err(_) => Err("the key store write failed".to_string()),
        };
        *out.lock().unwrap_or_else(PoisonError::into_inner) = Some(r);
        ctx2.request_repaint();
    });
    started.map_err(|e| e.to_string())?;
    Ok(slot)
}

/// The OS key store behind a cache (S10): a lookup can wait up to 20 s on an unlock prompt, and
/// the host asks for secrets on the UI thread (the settings form, every run's environment, the
/// agent list). [`CachedKeyStore::warm`] reads the installed plugins' secrets on a side thread; a
/// key it has not read yet is asked once, then served from the cache; set / delete go through.
/// Writes win over reads in flight (RC13): a read only fills a key nothing wrote meanwhile, and a
/// delete leaves `None` cached, so a warm read that started before it cannot bring the value back.
#[derive(Default)]
pub struct CachedKeyStore {
    cache: Mutex<BTreeMap<(String, String), Option<String>>>,
}

impl CachedKeyStore {
    fn cached(&self, plugin: &str, key: &str) -> Option<Option<String>> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).get(&(plugin.to_string(), key.to_string())).cloned()
    }

    /// A write: replaces what is cached.
    fn remember(&self, plugin: &str, key: &str, value: Option<String>) {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).insert((plugin.to_string(), key.to_string()), value);
    }

    /// A read: fills the key only if no write landed while the store was asked.
    fn remember_read(&self, plugin: &str, key: &str, value: Option<String>) -> Option<String> {
        let mut c = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        c.entry((plugin.to_string(), key.to_string())).or_insert(value).clone()
    }

    /// Read `keys` (plugin id, setting id) into the cache on a side thread.
    pub fn warm(self: &Arc<Self>, keys: Vec<(String, String)>) {
        if keys.is_empty() {
            return;
        }
        let me = self.clone();
        let _ = std::thread::Builder::new().name("bashcut-key-store-warm".into()).spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for (plugin, key) in keys {
                    if me.cached(&plugin, &key).is_none()
                        && let Ok(v) = OsKeyStore.get(&plugin, &key)
                    {
                        me.remember_read(&plugin, &key, v);
                    }
                }
            }));
        });
    }
}

impl SecretStore for CachedKeyStore {
    fn get(&self, plugin: &str, key: &str) -> HostResult<Option<String>> {
        if let Some(v) = self.cached(plugin, key) {
            return Ok(v);
        }
        let v = OsKeyStore.get(plugin, key)?;
        Ok(self.remember_read(plugin, key, v))
    }

    fn set(&self, plugin: &str, key: &str, value: &str) -> HostResult<()> {
        OsKeyStore.set(plugin, key, value)?;
        self.remember(plugin, key, Some(value.to_string()));
        Ok(())
    }

    fn delete(&self, plugin: &str, key: &str) -> HostResult<()> {
        let r = OsKeyStore.delete(plugin, key);
        if r.is_ok() {
            // delete wins: a warm read in flight finds the key filled and keeps `None`
            self.remember(plugin, key, None);
        } else {
            // the store said no: ask it again next time
            self.cache.lock().unwrap_or_else(PoisonError::into_inner).remove(&(plugin.to_string(), key.to_string()));
        }
        r
    }
}

/// The host if one was created already (nothing has used plugins yet otherwise).
pub fn existing(ctx: &egui::Context) -> Option<Shared> {
    super::shared::existing(ctx, key())
}

/// Run `f` with the host locked.
pub fn with<R>(app: &FilmcraftApp, ctx: &egui::Context, f: impl FnOnce(&mut Host) -> R) -> R {
    let h = host(app, ctx);
    let mut g = h.lock().unwrap_or_else(PoisonError::into_inner);
    f(&mut g)
}

/// Collect run events (called by `layout` every frame); keeps frames coming while a job runs.
pub fn poll(ctx: &egui::Context) {
    // no host yet means nothing ever ran: don't create one just to poll it
    let Some(h) = existing(ctx) else { return };
    let mut g = h.lock().unwrap_or_else(PoisonError::into_inner);
    let was_running = g.any_running();
    let running = g.poll();
    if running {
        ctx.request_repaint_after(Duration::from_millis(100));
    } else if was_running {
        // the last job just ended: one more frame shows its outcome without waiting for input
        ctx.request_repaint();
    }
}

/// The open app's control address, if it serves one (on by default, [`super::control`]; else
/// `--control <port>` or `FILMCRAFT_CONTROL_PORT`).
pub fn bridge_address() -> Option<String> {
    if let Some(p) = super::control::launched() {
        return p.map(|p| format!("127.0.0.1:{p}"));
    }
    static ADDR: OnceLock<Option<String>> = OnceLock::new();
    ADDR.get_or_init(|| {
        let from_env = std::env::var("FILMCRAFT_CONTROL_PORT").ok().and_then(|p| p.trim().parse::<u16>().ok());
        let args: Vec<String> = std::env::args().collect();
        let from_args = args.iter().enumerate().find_map(|(i, a)| match a.strip_prefix("--control=") {
            Some(p) => p.parse::<u16>().ok(),
            None if a == "--control" => args.get(i + 1).and_then(|p| p.parse::<u16>().ok()),
            None => None,
        });
        from_args.or(from_env).map(|p| format!("127.0.0.1:{p}"))
    })
    .clone()
}

/// The engine next to the running app, `bashcutpro` first, else `filmcraft-cli` (#154); passed to
/// plugins as `--cli`, to batches and to agents.
pub fn cli_path() -> Option<String> {
    bashcut_batch::cli::beside_self().map(|p| p.to_string_lossy().into_owned())
}

pub fn run_context() -> RunContext {
    let known = filmcraft_engine::commands::command_specs().iter().map(|c| c.id.to_string()).collect();
    RunContext { bridge: bridge_address(), cli: cli_path(), known_commands: known }
}

/// `bashcut.plugins.<name>`.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, name: &str, params: &Value) -> Result<Value, String> {
    if name == "sendToAgent" {
        return send_to_agent(app, ctx, params);
    }
    let mut rc = run_context();
    if let Some(b) = params.get("bridge").and_then(Value::as_str).filter(|b| !b.is_empty()) {
        rc.bridge = Some(b.to_string());
    }
    let r = with(app, ctx, |h| h.command(name, params, &rc)).map_err(|e| e.0);
    if name == "run" && r.is_ok() {
        // a run's outcome shows in its plugin's card ("Open in Runs" there), so the panel stays put;
        // a run that shows a form opens Plugins, where the form is (`bashcut/in-app-ui.md` §4)
        let s = |k: &str| params.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let (plugin, action) = (s("plugin"), s("action"));
        let view = with(app, ctx, |h| h.plugin(&plugin).and_then(|p| p.manifest.as_ref()).and_then(|m| m.action(&action)).is_some_and(|a| a.view));
        if view {
            super::State::update(ctx, |st| st.show(super::PLUGINS));
        }
    }
    r
}

/// Hand a run's result to the agent as the next message: what ran, what the plugin asks the agent
/// to do (`handoff.then`) and its result.
fn send_to_agent(app: &mut FilmcraftApp, ctx: &egui::Context, params: &Value) -> Result<Value, String> {
    let job = params.get("job").and_then(Value::as_u64).ok_or("`job` (number) is required")?;
    let j = with(app, ctx, |h| h.job(job).cloned()).ok_or(format!("no job {job}"))?;
    if j.state.is_running() {
        return Err(format!("job {job} is still running"));
    }
    let result = j.outcome.as_ref().and_then(|o| o.result.clone());
    let mut text = format!("The plugin run \"{}\" ended: {}.", j.title, j.message);
    if let Some(then) = &j.then {
        text.push_str(&format!(" It hands off to you: {then}."));
    }
    if let Some(r) = &result {
        let r = serde_json::to_string(r).unwrap_or_default();
        text.push_str(&format!("\n\nResult (bashcut-result/1):\n{}", bashcut_agent::clip(&r, 48 * 1024)));
    }
    super::agent::send(app, ctx, &text)?;
    Ok(json!({"sent": true, "job": job, "then": j.then}))
}

pub fn commands() -> Vec<CommandInfo> {
    [
        ("list", "List Plugins"),
        ("registry", "Check Plugin Registry"),
        ("install", "Install Plugin"),
        ("update", "Update Plugin"),
        ("remove", "Remove Plugin"),
        ("settings", "Plugin Settings"),
        ("setSecret", "Store Plugin Secret"),
        ("run", "Run Plugin Action"),
        ("runs", "List Runs"),
        ("job", "Run Details"),
        ("cancel", "Cancel Run"),
        ("clear", "Clear Finished Runs"),
        ("sendToAgent", "Send Run to Agent"),
        ("view", "Plugin Form"),
        ("click", "Press Plugin Form Button"),
    ]
    .into_iter()
    .map(|(id, label)| CommandInfo::new(&format!("bashcut.plugins.{id}"), label, &[], None))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key no OS store holds: a lookup that reached the store would come back `None`.
    const PLUGIN: &str = "test.bashcut-cache-never-stored";

    #[test]
    fn cached_key_store_serves_warmed_keys() {
        let store = CachedKeyStore::default();
        assert_eq!(store.cached(PLUGIN, "api_key"), None, "nothing read yet");
        // what the warm thread does once the OS store answered
        assert_eq!(store.remember_read(PLUGIN, "api_key", Some("sk-warm".into())).as_deref(), Some("sk-warm"));
        // served from the cache: the OS store has no such key, so a second lookup would say None
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-warm"));
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-warm"));
        // a warmed "not set" is cached too
        store.remember_read(PLUGIN, "token", None);
        assert_eq!(store.cached(PLUGIN, "token"), Some(None));
        assert_eq!(store.get(PLUGIN, "token").ok(), Some(None));
        // set (after the store took it) updates the cache, delete leaves `None` cached
        store.remember(PLUGIN, "api_key", Some("sk-new".into()));
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-new"));
        store.remember(PLUGIN, "api_key", None);
        assert_eq!(store.get(PLUGIN, "api_key").ok(), Some(None));
        // keys are per plugin and setting
        assert_eq!(store.cached("test.other", "api_key"), None);
    }

    #[test]
    fn cached_key_store_write_beats_warm_read() {
        let store = std::sync::Arc::new(CachedKeyStore::default());
        // the warm read started before the delete and answers after it: delete wins
        store.remember(PLUGIN, "api_key", None);
        assert_eq!(store.remember_read(PLUGIN, "api_key", Some("sk-old".into())), None);
        assert_eq!(store.cached(PLUGIN, "api_key"), Some(None));
        // the same for a set
        store.remember(PLUGIN, "api_key", Some("sk-new".into()));
        assert_eq!(store.remember_read(PLUGIN, "api_key", Some("sk-old".into())).as_deref(), Some("sk-new"));
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-new"));
        // racing threads: every late read loses to the write that landed first
        store.remember(PLUGIN, "race", None);
        let readers: Vec<_> = (0..8)
            .map(|i| {
                let s = store.clone();
                std::thread::spawn(move || s.remember_read(PLUGIN, "race", Some(format!("old-{i}"))))
            })
            .collect();
        for r in readers {
            assert_eq!(r.join().ok(), Some(None));
        }
        assert_eq!(store.cached(PLUGIN, "race"), Some(None));
    }
}
