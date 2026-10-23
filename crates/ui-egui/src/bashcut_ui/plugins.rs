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
        let inner = ctx.data(|d| d.get_temp::<Arc<dyn SecretStore>>(inner_key())).unwrap_or_else(|| Arc::new(OsKeyStore));
        let store = Arc::new(CachedKeyStore::new(inner));
        ctx.data_mut(|d| d.insert_temp(store_key(), store.clone()));
        let host = Host::new(super::data_dir(app), store.clone());
        let keys = host
            .installed()
            .iter()
            .filter_map(|p| p.manifest.as_ref())
            .flat_map(|m| m.settings.iter().filter(|s| s.is_secret()).map(|s| (m.id.clone(), s.id.clone())))
            .collect();
        let _ = store.warm(keys);
        host
    })
}

fn store_key() -> egui::Id {
    egui::Id::new("bashcut.plugins.keyStore")
}

fn inner_key() -> egui::Id {
    egui::Id::new("bashcut.plugins.keyStore.inner")
}

/// Put `inner` behind this context's [`CachedKeyStore`] instead of the OS key store (a test's fake
/// store). Only before the host exists: `false` once it was created with the store it has.
pub fn use_key_store(ctx: &egui::Context, inner: Arc<dyn SecretStore>) -> bool {
    if existing(ctx).is_some() {
        return false;
    }
    ctx.data_mut(|d| d.insert_temp(inner_key(), inner));
    true
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

/// A key store (the OS one, [`OsKeyStore`], unless [`use_key_store`] named another) behind a cache (S10): a lookup can wait up to 20 s on an unlock prompt, and
/// the host asks for secrets on the UI thread (the settings form, every run's environment, the
/// agent list). [`CachedKeyStore::warm`] reads the installed plugins' secrets on a side thread; a
/// key it has not read yet is asked once, then served from the cache; set / delete go through.
/// Writes win over reads in flight (RC13): a read only fills a key nothing wrote meanwhile, and a
/// delete leaves `None` cached, so a warm read that started before it cannot bring the value back.
pub struct CachedKeyStore {
    inner: Arc<dyn SecretStore>,
    cache: Mutex<BTreeMap<(String, String), Option<String>>>,
}

impl Default for CachedKeyStore {
    fn default() -> Self {
        Self::new(Arc::new(OsKeyStore))
    }
}

impl CachedKeyStore {
    pub fn new(inner: Arc<dyn SecretStore>) -> Self {
        Self { inner, cache: Mutex::default() }
    }

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

    /// Read `keys` (plugin id, setting id) into the cache on a side thread (its handle, if it started).
    pub fn warm(self: &Arc<Self>, keys: Vec<(String, String)>) -> Option<std::thread::JoinHandle<()>> {
        if keys.is_empty() {
            return None;
        }
        let me = self.clone();
        let started = std::thread::Builder::new().name("bashcut-key-store-warm".into()).spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for (plugin, key) in keys {
                    if me.cached(&plugin, &key).is_none()
                        && let Ok(v) = me.inner.get(&plugin, &key)
                    {
                        me.remember_read(&plugin, &key, v);
                    }
                }
            }));
        });
        started.ok()
    }
}

impl SecretStore for CachedKeyStore {
    fn get(&self, plugin: &str, key: &str) -> HostResult<Option<String>> {
        if let Some(v) = self.cached(plugin, key) {
            return Ok(v);
        }
        let v = self.inner.get(plugin, key)?;
        Ok(self.remember_read(plugin, key, v))
    }

    fn set(&self, plugin: &str, key: &str, value: &str) -> HostResult<()> {
        self.inner.set(plugin, key, value)?;
        self.remember(plugin, key, Some(value.to_string()));
        Ok(())
    }

    fn delete(&self, plugin: &str, key: &str) -> HostResult<()> {
        let r = self.inner.delete(plugin, key);
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

/// The engine next to the running app, `bashcutpro-cli` first, else `bcpro` or `filmcraft-cli` (#154); passed to
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
    use bashcut_plugin_host::Error as HostError;
    use bashcut_plugin_host::settings::MemoryStore;
    use std::sync::Condvar;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PLUGIN: &str = "test.bashcut-cache";

    /// A store that counts what reaches it, can refuse writes, and holds every `get` until
    /// [`Fake::release`] (when built [`Fake::slow`]), answering with what it held when asked.
    #[derive(Default)]
    struct Fake {
        inner: MemoryStore,
        gets: AtomicUsize,
        sets: AtomicUsize,
        deletes: AtomicUsize,
        refuse: std::sync::atomic::AtomicBool,
        held: Mutex<bool>,
        gate: Condvar,
        asked: AtomicUsize,
    }

    impl Fake {
        fn slow() -> Self {
            Self { held: Mutex::new(true), ..Self::default() }
        }

        fn release(&self) {
            *self.held.lock().unwrap_or_else(PoisonError::into_inner) = false;
            self.gate.notify_all();
        }

        /// Wait (at most ~10 s) until `n` gets are waiting at the gate.
        fn wait_asked(&self, n: usize) {
            for _ in 0..1000 {
                if self.asked.load(Ordering::SeqCst) >= n {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("no get reached the store");
        }
    }

    impl SecretStore for Fake {
        fn get(&self, plugin: &str, key: &str) -> HostResult<Option<String>> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            // the answer is read now; it arrives once released, after any write that came meanwhile
            let v = self.inner.get(plugin, key);
            self.asked.fetch_add(1, Ordering::SeqCst);
            let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            let _held = self.gate.wait_timeout_while(held, Duration::from_secs(30), |h| *h);
            v
        }

        fn set(&self, plugin: &str, key: &str, value: &str) -> HostResult<()> {
            self.sets.fetch_add(1, Ordering::SeqCst);
            if self.refuse.load(Ordering::SeqCst) {
                return Err(HostError("refused".into()));
            }
            self.inner.set(plugin, key, value)
        }

        fn delete(&self, plugin: &str, key: &str) -> HostResult<()> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            if self.refuse.load(Ordering::SeqCst) {
                return Err(HostError("refused".into()));
            }
            self.inner.delete(plugin, key)
        }
    }

    fn over(fake: &Arc<Fake>) -> Arc<CachedKeyStore> {
        Arc::new(CachedKeyStore::new(fake.clone()))
    }

    #[test]
    fn cached_key_store_serves_warmed_keys() {
        let fake = Arc::new(Fake::default());
        let store = over(&fake);
        assert_eq!(store.cached(PLUGIN, "api_key"), None, "nothing read yet");
        // what the warm thread does once the store answered
        assert_eq!(store.remember_read(PLUGIN, "api_key", Some("sk-warm".into())).as_deref(), Some("sk-warm"));
        // served from the cache: the store has no such key and is never asked
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-warm"));
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-warm"));
        assert_eq!(fake.gets.load(Ordering::SeqCst), 0);
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
        assert_eq!(fake.gets.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn cached_key_store_asks_the_inner_store_once_and_writes_through() {
        let fake = Arc::new(Fake::default());
        fake.inner.set(PLUGIN, "api_key", "sk-there").unwrap();
        let store = over(&fake);
        // a miss asks the store once, then the cache answers
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-there"));
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-there"));
        assert_eq!(fake.gets.load(Ordering::SeqCst), 1);
        // set reaches the store and the cache
        store.set(PLUGIN, "api_key", "sk-new").unwrap();
        assert_eq!(fake.sets.load(Ordering::SeqCst), 1);
        assert_eq!(fake.inner.get(PLUGIN, "api_key").unwrap().as_deref(), Some("sk-new"));
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-new"));
        // delete reaches the store and leaves `None` cached
        store.delete(PLUGIN, "api_key").unwrap();
        assert_eq!(fake.deletes.load(Ordering::SeqCst), 1);
        assert_eq!(fake.inner.get(PLUGIN, "api_key").unwrap(), None);
        assert_eq!(store.get(PLUGIN, "api_key").ok(), Some(None));
        assert_eq!(fake.gets.load(Ordering::SeqCst), 1, "writes never ask the store");

        // a refused set keeps what was cached; a refused delete forgets it, so the store is asked again
        store.set(PLUGIN, "api_key", "sk-kept").unwrap();
        fake.refuse.store(true, Ordering::SeqCst);
        assert!(store.set(PLUGIN, "api_key", "sk-lost").is_err());
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-kept"));
        assert!(store.delete(PLUGIN, "api_key").is_err());
        assert_eq!(store.cached(PLUGIN, "api_key"), None);
        assert_eq!(store.get(PLUGIN, "api_key").ok().flatten().as_deref(), Some("sk-kept"), "asked again");
        assert_eq!(fake.gets.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cached_key_store_write_beats_warm_read() {
        let store = Arc::new(CachedKeyStore::new(Arc::new(MemoryStore::default())));
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

    /// RC13 with a store that is slow to answer: the warm thread and a UI lookup ask it, a delete
    /// and a set land while they wait, and the old answers that arrive later cannot undo them.
    #[test]
    fn a_slow_warm_read_cannot_undo_a_write() {
        let fake = Arc::new(Fake::slow());
        fake.inner.set(PLUGIN, "gone", "sk-old").unwrap();
        fake.inner.set(PLUGIN, "changed", "sk-old").unwrap();
        let store = over(&fake);
        let warm = store.warm(vec![(PLUGIN.into(), "gone".into()), (PLUGIN.into(), "changed".into())]).expect("warm thread");
        fake.wait_asked(1);
        // the warm thread holds the old "gone"; it is deleted meanwhile
        store.delete(PLUGIN, "gone").unwrap();
        // a lookup of "changed" waits on the store too; it is set meanwhile
        let s = store.clone();
        let lookup = std::thread::spawn(move || s.get(PLUGIN, "changed").ok().flatten());
        fake.wait_asked(2);
        store.set(PLUGIN, "changed", "sk-new").unwrap();
        fake.release();
        assert!(warm.join().is_ok());
        assert_eq!(lookup.join().ok().flatten().as_deref(), Some("sk-new"), "the waiting lookup gets the write");
        assert_eq!(store.cached(PLUGIN, "gone"), Some(None), "the delete stays");
        assert_eq!(store.get(PLUGIN, "gone").ok(), Some(None));
        assert_eq!(store.get(PLUGIN, "changed").ok().flatten().as_deref(), Some("sk-new"), "the set stays");
        assert_eq!(fake.inner.get(PLUGIN, "gone").unwrap(), None);
        // the warm thread skips "changed" (the lookup filled it first) or asked and lost: never more than 3
        assert!(fake.gets.load(Ordering::SeqCst) <= 3);
    }

    #[test]
    fn use_key_store_only_before_the_host_exists() {
        let ctx = egui::Context::default();
        assert!(use_key_store(&ctx, Arc::new(MemoryStore::default())));
        assert!(ctx.data(|d| d.get_temp::<Arc<dyn SecretStore>>(inner_key())).is_some());
        let _ = super::super::shared::get_or_init(&ctx, key(), || Host::new(None, Arc::new(MemoryStore::default())));
        assert!(!use_key_store(&ctx, Arc::new(MemoryStore::default())), "the host has its store already");
    }
}
