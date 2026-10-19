//! Headless UI tests of BashCutPro's Library section (#68) with a fake resources plugin: search
//! online, see the licence, place a result at the playhead, credits only when turned on.
//! Fork-only file. Needs `python3` (the fake plugin is a Python script); skipped without it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_engine::autosave::{AutoSavePrefs, AutosaveConfig, Persistence};
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

const MANIFEST: &str = r#"{
  "schema": "bashcut-plugin/1",
  "id": "bashcut.resources",
  "name": "Online Resources",
  "version": "0.2.0",
  "summary": "Fake resources plugin for the Library UI test.",
  "platforms": [],
  "runtime": {"type": "python", "version": ">=3.9"},
  "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]},
  "connect": {"project": ["--project", "{project}"], "bridge": ["--bridge", "{address}"], "cli": ["--cli", "{cli}"]},
  "output": "bashcut-result/1",
  "app": {"commands": []},
  "actions": [
    {"id": "search", "title": "Search open media…", "args": ["search"], "needs": "none", "edits": false, "params": [
      {"id": "kind", "flag": "--kind", "type": "enum", "choices": ["music", "sfx", "photo", "video", "icon", "font"], "default": "music"},
      {"id": "query", "flag": "--query", "type": "string"},
      {"id": "count", "flag": "--count", "type": "integer", "default": 12}
    ]},
    {"id": "download", "title": "Download a search result", "args": ["download"], "needs": "none", "edits": false, "params": [
      {"id": "kind", "flag": "--kind", "type": "enum", "choices": ["music", "sfx", "photo", "video", "icon", "font"], "default": "music"},
      {"id": "url", "flag": "--url", "type": "string"},
      {"id": "title", "flag": "--title", "type": "string", "default": ""},
      {"id": "license", "flag": "--license", "type": "string", "default": ""},
      {"id": "author", "flag": "--author", "type": "string", "default": ""},
      {"id": "source", "flag": "--source", "type": "string", "default": ""}
    ]}
  ]
}"#;

/// Answers `search` with one music bed and `download` with a one-second WAV next to the script.
const PLUGIN: &str = r#"import json, os, struct, sys, wave
args = sys.argv[1:]
def flag(name):
    return args[args.index(name) + 1] if name in args else ""
if args[:1] == ["search"]:
    item = {"kind": "music", "from": "fake", "title": "Morning Bed", "author": "Ann", "license": "CC0",
            "source": "https://example.org/bed", "url": "https://example.org/bed.wav", "preview": "https://example.org/bed.wav", "seconds": 1}
    print(json.dumps({"ok": True, "message": "1 result", "data": {"kind": flag("--kind"), "query": flag("--query"), "items": [item]}}))
elif args[:1] == ["download"]:
    here = os.path.dirname(os.path.abspath(__file__))
    n = len([f for f in os.listdir(here) if f.endswith(".wav")])
    path = os.path.join(here, "bed-%d.wav" % n)
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
        w.writeframes(struct.pack("<hh", 0, 0) * 48000)
    print(json.dumps({"ok": True, "message": "Downloaded", "data": {"path": path, "license": flag("--license"),
                      "credit": "- %s, by %s, %s" % (flag("--title"), flag("--author"), flag("--license"))}}))
else:
    print(json.dumps({"ok": False, "error": {"code": "bad_args", "message": "unknown"}}))
    sys.exit(2)
"#;

struct Driver {
    harness: Harness<'static, FilmcraftApp>,
    tx: Sender<ControlRequest>,
}

impl Driver {
    fn with_data_dir(dir: &Path) -> Self {
        let mut session = Session::default();
        session.persistence = Some(Persistence::start(&AutosaveConfig::new(dir), AutoSavePrefs::default()).expect("persistence"));
        session.execute("file.openDemoProject", json!({})).expect("demo project");
        let (tx, rx) = channel();
        let app = FilmcraftApp::new(session).with_control(rx);
        let harness = Harness::builder().with_size(egui::vec2(1600.0, 980.0)).with_max_steps(10_000).build_eframe(move |_cc| app);
        let mut d = Driver { harness, tx };
        d.frames(4);
        d
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            let ctx = self.harness.ctx.clone();
            let mut raw = std::mem::take(self.harness.input_mut());
            eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut raw);
            *self.harness.input_mut() = raw;
            self.harness.step();
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params.clone());
        self.tx.send(req).unwrap();
        for _ in 0..600 {
            self.frames(1);
            if let Ok(v) = reply.try_recv() {
                return v;
            }
        }
        panic!("no reply to {method} {params}");
    }

    fn exec(&mut self, command: &str, params: Value) -> Value {
        let v = self.call("engine.execute", json!({"command": command, "params": params}));
        assert_eq!(v["ok"], json!(true), "{command} {params} failed: {v}");
        self.frames(2);
        v["result"].clone()
    }

    fn has(&mut self, id: &str) -> bool {
        let v = self.call("ui.elements", json!({"prefix": id}));
        v["result"].as_array().is_some_and(|a| a.iter().any(|e| e["id"] == json!(id)))
    }

    /// Frames until `done` holds for the library's results (plugin runs are child processes).
    fn wait_results(&mut self, done: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..200 {
            let r = self.exec("bashcut.ui.library.results", json!({}));
            if done(&r) {
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("the library never got there");
    }

    fn audio_clips(&mut self) -> usize {
        let q = self.exec("sequence.inspect", json!({}));
        q["audio"].as_array().unwrap().iter().map(|t| t["items"].as_array().map_or(0, Vec::len)).sum()
    }
}

fn python() -> bool {
    std::process::Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bashcut-library-ui-{name}-{}", std::process::id()));
    let plugin = dir.join("bashcut-plugins").join("bashcut.resources");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("bashcut-plugin.json"), MANIFEST).unwrap();
    std::fs::write(plugin.join("plugin.py"), PLUGIN).unwrap();
    dir
}

#[test]
fn online_search_shows_the_licence_and_places_at_the_playhead() {
    if !python() {
        eprintln!("skipped: no python3");
        return;
    }
    let dir = data_dir("place");
    let mut d = Driver::with_data_dir(&dir);
    d.exec("bashcut.ui.show.library", json!({}));
    assert!(d.has("bashcut.panel.library"));
    assert!(d.has("bashcut.library.tab.online"));

    d.exec("bashcut.ui.library.search", json!({"tab": "online", "kind": "music", "query": "calm"}));
    let r = d.wait_results(|r| r["items"].as_array().is_some_and(|a| !a.is_empty()));
    assert_eq!(r["items"][0]["license"], json!("CC0"));
    d.frames(3);
    assert!(d.has("bashcut.library.online.0.licence"), "the licence shows before any download");
    assert!(d.has("bashcut.library.online.0.place"));

    // credits are off by default: placed, no credit line in the notes
    let before = d.audio_clips();
    d.exec("bashcut.ui.library.place", json!({"index": 0}));
    d.wait_results(|r| r["downloading"] == json!(0) && r["status"].as_str().is_some_and(|s| s.contains("Placed")));
    assert_eq!(d.audio_clips(), before + 1, "placed on the timeline");
    let notes = d.exec("project.notes", json!({}));
    assert!(!notes["text"].as_str().unwrap_or("").contains("Morning Bed"), "no credits unless turned on");

    // with credits on, the credit line goes into the notes, once
    d.exec("bashcut.ui.library.credits", json!({"on": true}));
    d.exec("bashcut.ui.library.place", json!({"index": 0}));
    d.wait_results(|r| r["downloading"] == json!(0) && r["status"].as_str().is_some_and(|s| s.contains("Placed")));
    assert_eq!(d.audio_clips(), before + 2);
    let notes = d.exec("project.notes", json!({}));
    let text = notes["text"].as_str().unwrap_or("").to_string();
    assert_eq!(text.matches("Morning Bed, by Ann, CC0").count(), 1, "{text}");

    // the palette finds the Library commands
    let found = d.exec("bashcut.palette.search", json!({"query": "search the library"}));
    assert!(found.as_array().unwrap().iter().any(|r| r["command"] == json!("bashcut.ui.library.search")), "{found}");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_the_plugin_the_tab_points_to_plugins() {
    let mut d = Driver::with_data_dir(&std::env::temp_dir().join(format!("bashcut-library-ui-empty-{}", std::process::id())));
    d.exec("bashcut.ui.show.library", json!({}));
    d.exec("bashcut.ui.library.state", json!({"tab": "mine"}));
    d.frames(3);
    assert!(d.has("bashcut.library.install"), "Mine needs the library plugin");
    let v = d.call("engine.execute", json!({"command": "bashcut.ui.library.search", "params": {"tab": "mine", "query": "x"}}));
    assert_ne!(v["ok"], json!(true), "not installed: {v}");
}
