//! The rail and side panel's state (`bashcut/in-app-ui.md` §4).
//!
//! Kept in egui's context data, so `FilmcraftApp` and `UiState` gain no field, and saved to
//! `<data dir>/bashcut/ui.json` when it changes. The file is hostile input: capped, clamped, and
//! unknown sections are dropped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Monitors remembered for pop-out positions.
const MAX_MONITORS: usize = 8;

/// Side panel width limits (px).
pub const MIN_WIDTH: f32 = 240.0;
pub const MAX_WIDTH: f32 = 720.0;
pub const DEFAULT_WIDTH: f32 = 340.0;

/// Largest `ui.json` read back; anything bigger is ignored.
const MAX_FILE: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    /// The rail is shown (it hides with `bashcut.ui.toggleRail`).
    pub rail: bool,
    /// The section docked in the side panel, if it is open.
    pub open: Option<String>,
    /// Side panel width.
    pub width: f32,
    /// Sections popped out into floating windows.
    pub popped: Vec<String>,
    /// Our language override (`"vi"`); `None` follows the interface language.
    pub language: Option<String>,
    /// Where each popped-out section's window was, per monitor (`"<w>x<h>"` of the monitor): `[x, y, w, h]`.
    pub windows: BTreeMap<String, BTreeMap<String, [f32; 4]>>,
    /// Read from disk already.
    #[serde(skip)]
    loaded: bool,
    /// What was last written to disk.
    #[serde(skip)]
    saved: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self { rail: true, open: None, width: DEFAULT_WIDTH, popped: Vec::new(), language: None, windows: BTreeMap::new(), loaded: false, saved: None }
    }
}

fn key() -> egui::Id {
    egui::Id::new("bashcut.ui.state")
}

/// `<data dir>/bashcut/ui.json`.
pub fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("bashcut").join("ui.json")
}

impl State {
    /// This frame's state.
    pub fn get(ctx: &egui::Context) -> State {
        ctx.data_mut(|d| d.get_temp_mut_or_default::<State>(key()).clone())
    }

    pub fn put(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(key(), self));
    }

    /// Change the state in place and return what `f` returns.
    pub fn update<R>(ctx: &egui::Context, f: impl FnOnce(&mut State) -> R) -> R {
        let mut st = Self::get(ctx);
        let r = f(&mut st);
        st.put(ctx);
        r
    }

    /// Keep the values in range and the sections known (after loading or a command).
    pub fn sanitize(&mut self, known: &[&str]) {
        self.width = if self.width.is_finite() { self.width.clamp(MIN_WIDTH, MAX_WIDTH) } else { DEFAULT_WIDTH };
        let has = |s: &str| known.iter().any(|k| *k == s);
        if self.open.as_deref().is_some_and(|s| !has(s)) {
            self.open = None;
        }
        self.popped.retain(|s| has(s));
        self.popped.dedup();
        if self.language.as_deref().is_some_and(|l| l.len() > 8) {
            self.language = None;
        }
        // window rects: known sections, finite, on-screen sizes; a few monitors at most
        for rects in self.windows.values_mut() {
            rects.retain(|s, r| has(s) && r.iter().all(|v| v.is_finite() && v.abs() < 100_000.0) && r[2] >= MIN_WIDTH / 2.0 && r[3] >= 40.0);
        }
        self.windows.retain(|m, rects| m.len() <= 32 && !rects.is_empty());
        while self.windows.len() > MAX_MONITORS {
            let Some(first) = self.windows.keys().next().cloned() else { break };
            self.windows.remove(&first);
        }
    }

    /// The saved window rect of `section` on `monitor`.
    pub fn window_rect(&self, monitor: &str, section: &str) -> Option<[f32; 4]> {
        self.windows.get(monitor).and_then(|m| m.get(section)).copied()
    }

    /// Remember where `section`'s window is on `monitor` (rounded, so tiny moves do not rewrite the file).
    pub fn remember_window(&mut self, monitor: &str, section: &str, rect: [f32; 4]) {
        if !rect.iter().all(|v| v.is_finite()) {
            return;
        }
        let r = rect.map(f32::round);
        self.windows.entry(monitor.to_string()).or_default().insert(section.to_string(), r);
        if self.windows.len() > MAX_MONITORS
            && let Some(other) = self.windows.keys().find(|k| k.as_str() != monitor).cloned()
        {
            self.windows.remove(&other);
        }
    }

    /// Open `section` in the side panel, or close it if it is the one open. A popped-out section
    /// docks back.
    pub fn toggle(&mut self, section: &str) {
        if self.open.as_deref() == Some(section) {
            self.open = None;
        } else {
            self.popped.retain(|s| s != section);
            self.open = Some(section.to_string());
        }
    }

    /// Show `section` (docked unless it is popped out).
    pub fn show(&mut self, section: &str) {
        self.rail = true;
        if !self.popped.iter().any(|s| s == section) {
            self.open = Some(section.to_string());
        }
    }

    /// Move `section` into a floating window.
    pub fn pop_out(&mut self, section: &str) {
        if self.open.as_deref() == Some(section) {
            self.open = None;
        }
        if !self.popped.iter().any(|s| s == section) {
            self.popped.push(section.to_string());
        }
    }

    /// Dock a floating `section` back into the side panel.
    pub fn dock_back(&mut self, section: &str) {
        self.popped.retain(|s| s != section);
        self.open = Some(section.to_string());
    }

    /// Read `ui.json` once (first frame). A missing or broken file keeps the defaults.
    pub fn load_once(&mut self, data_dir: Option<&Path>, known: &[&str]) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(dir) = data_dir else { return };
        let path = file(dir);
        let small = std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_FILE);
        if !small {
            return;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        match serde_json::from_str::<State>(&text) {
            Ok(mut st) => {
                st.sanitize(known);
                st.loaded = true;
                st.saved = Some(text);
                *self = st;
            }
            Err(e) => log::warn!("bashcut ui state {}: {e}", path.display()),
        }
    }

    /// Write `ui.json` if the state changed since the last write. Errors are logged, never fatal.
    pub fn save_if_changed(&mut self, data_dir: Option<&Path>) {
        let Some(dir) = data_dir else { return };
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        if self.saved.as_deref() == Some(text.as_str()) {
            return;
        }
        let path = file(dir);
        let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, &text));
        if let Err(e) = written {
            log::warn!("bashcut ui state {}: {e}", path.display());
        }
        // remember it either way: a read-only data dir must not be retried every frame
        self.saved = Some(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: &[&str] = &["agent", "plugins", "runs"];

    #[test]
    fn toggle_pop_out_and_dock_back() {
        let mut st = State::default();
        st.toggle("agent");
        assert_eq!(st.open.as_deref(), Some("agent"));
        st.toggle("agent");
        assert_eq!(st.open, None);
        st.toggle("runs");
        st.pop_out("runs");
        assert_eq!(st.open, None);
        assert_eq!(st.popped, vec!["runs".to_string()]);
        st.show("runs");
        assert_eq!(st.open, None, "a popped-out section stays floating");
        st.dock_back("runs");
        assert_eq!(st.open.as_deref(), Some("runs"));
        assert!(st.popped.is_empty());
    }

    #[test]
    fn hostile_values_are_sanitized() {
        let mut st: State = serde_json::from_str(r#"{"width":1e30,"open":"nope","popped":["x","runs","runs"],"language":"verylonglang"}"#).unwrap();
        st.sanitize(KNOWN);
        assert_eq!(st.width, MAX_WIDTH);
        assert_eq!(st.open, None);
        assert_eq!(st.popped, vec!["runs".to_string()]);
        assert_eq!(st.language, None);
        let mut st = State { width: f32::NAN, ..State::default() };
        st.sanitize(KNOWN);
        assert_eq!(st.width, DEFAULT_WIDTH);
        assert!(serde_json::from_str::<State>("[1,2]").is_err());
    }

    #[test]
    fn pop_out_windows_remember_their_place_per_monitor() {
        let mut st = State::default();
        st.remember_window("2560x1440", "runs", [100.4, 80.0, 400.0, 520.0]);
        st.remember_window("1512x982", "runs", [10.0, 10.0, 300.0, 400.0]);
        assert_eq!(st.window_rect("2560x1440", "runs"), Some([100.0, 80.0, 400.0, 520.0]));
        assert_eq!(st.window_rect("1512x982", "runs"), Some([10.0, 10.0, 300.0, 400.0]));
        assert_eq!(st.window_rect("1512x982", "agent"), None);
        st.remember_window("x", "runs", [f32::NAN, 0.0, 1.0, 1.0]);
        assert_eq!(st.window_rect("x", "runs"), None);
        for i in 0..20 {
            st.remember_window(&format!("{i}x{i}"), "runs", [0.0, 0.0, 300.0, 300.0]);
        }
        assert!(st.windows.len() <= MAX_MONITORS);
        let mut st: State =
            serde_json::from_str(r#"{"windows":{"a":{"runs":[1e30,0,300,300],"nope":[0,0,300,300],"agent":[0,0,300,300]},"b":{"runs":[0,0,1,1]}}}"#).unwrap();
        st.sanitize(KNOWN);
        assert_eq!(st.windows.len(), 1);
        assert_eq!(st.window_rect("a", "agent"), Some([0.0, 0.0, 300.0, 300.0]));
    }

    #[test]
    fn saves_and_loads_through_the_data_dir() {
        let dir = std::env::temp_dir().join(format!("bashcut-ui-state-{}", std::process::id()));
        let mut st = State { open: Some("plugins".into()), width: 400.0, ..State::default() };
        st.save_if_changed(Some(&dir));
        let mut back = State::default();
        back.load_once(Some(&dir), KNOWN);
        assert_eq!(back.open.as_deref(), Some("plugins"));
        assert_eq!(back.width, 400.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
