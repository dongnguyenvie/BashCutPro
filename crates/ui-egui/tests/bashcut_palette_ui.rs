//! Headless UI tests of BashCutPro's command palette (#52, `bashcut/in-app-ui.md` §1a), driven
//! through the control channel. Fork-only file.

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

struct Driver {
    harness: Harness<'static, FilmcraftApp>,
    tx: Sender<ControlRequest>,
}

impl Driver {
    fn demo() -> Self {
        let mut session = Session::default();
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

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.call(method, params.clone());
        assert_eq!(v["ok"], json!(true), "{method} {params} failed: {v}");
        v["result"].clone()
    }

    fn exec(&mut self, command: &str, params: Value) -> Value {
        let r = self.ok("engine.execute", json!({"command": command, "params": params}));
        self.frames(2);
        r
    }

    fn has(&mut self, id: &str) -> bool {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        v.as_array().is_some_and(|a| a.iter().any(|e| e["id"] == json!(id)))
    }

    fn v1_clips(&mut self) -> usize {
        let q = self.exec("sequence.inspect", json!({}));
        q["video"][0]["items"].as_array().map_or(0, Vec::len)
    }
}

#[test]
fn open_search_razor_and_run_it() {
    let mut d = Driver::demo();
    assert!(!d.has("bashcut.palette"));
    d.exec("bashcut.palette.open", json!({}));
    assert!(d.has("bashcut.palette"), "overlay shown");
    assert!(d.has("bashcut.palette.input"));

    let found = d.exec("bashcut.palette.search", json!({"query": "razor"}));
    let rows = found.as_array().expect("rows");
    assert!(rows.iter().take(5).any(|r| r["key"] == json!("command:timeline.razor")), "razor near the top: {found}");
    d.frames(2);
    assert!(d.has("bashcut.palette.item.command:timeline.razor"), "row drawn for the typed query");

    // run it through the palette at the middle of the first V1 clip: one more clip, one undo step
    let q = d.exec("sequence.inspect", json!({}));
    let first = &q["video"][0]["items"][0];
    let mid = first["start"].as_i64().unwrap() + first["duration"].as_i64().unwrap() / 2;
    let before = d.v1_clips();
    let undo_before = d.exec("history.list", json!({}))["undo"].as_array().map_or(0, Vec::len);
    d.exec("bashcut.palette.run", json!({"item": "command:timeline.razor", "params": {"time": mid, "track": "V1"}}));
    assert_eq!(d.v1_clips(), before + 1, "razor cut the clip");
    let undo_after = d.exec("history.list", json!({}))["undo"].as_array().map_or(0, Vec::len);
    assert_eq!(undo_after, undo_before + 1, "undoable like any command");
    assert!(!d.has("bashcut.palette"), "running closes the palette");

    // the item it ran comes first for an empty query (recent use)
    let recent = d.exec("bashcut.palette.search", json!({"query": ""}));
    assert_eq!(recent[0]["key"], json!("command:timeline.razor"));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
}

#[test]
fn keyboard_opens_types_and_closes() {
    let mut d = Driver::demo();
    d.ok("ui.key", json!({"key": "Cmd+Shift+P"}));
    d.frames(3);
    assert!(d.has("bashcut.palette"), "the shortcut opens the palette");
    d.ok("ui.type", json!({"text": "razor"}));
    d.frames(3);
    assert!(d.has("bashcut.palette.item.command:timeline.razor"));
    // Tab opens the parameter editor of the selected item, Esc goes back, Esc again closes
    d.ok("ui.key", json!({"key": "Tab"}));
    d.frames(3);
    assert!(d.has("bashcut.palette.params"));
    d.ok("ui.key", json!({"key": "Escape"}));
    d.frames(3);
    assert!(!d.has("bashcut.palette.params"));
    assert!(d.has("bashcut.palette.input"));
    d.ok("ui.key", json!({"key": "Escape"}));
    d.frames(3);
    assert!(!d.has("bashcut.palette"));
}

#[test]
fn recipes_profiles_and_bad_items() {
    let mut d = Driver::demo();
    let travel = d.exec("bashcut.palette.search", json!({"query": "travel"}));
    let rows = travel.as_array().expect("rows");
    assert!(rows.iter().any(|r| r["key"] == json!("recipe:vlog-travel")), "{travel}");
    let tiktok = d.exec("bashcut.palette.search", json!({"query": "tiktok"}));
    assert!(tiktok.as_array().expect("rows").iter().any(|r| r["kind"] == json!("profile")), "output profiles listed: {tiktok}");

    let r = d.exec("bashcut.palette.run", json!({"item": "recipe:vlog-travel"}));
    assert_eq!(r["recipe"], json!("vlog-travel"));
    assert_eq!(d.exec("bashcut.ui.state", json!({}))["open"], json!("agent"), "a recipe opens the Agent section");

    let bad = d.call("engine.execute", json!({"command": "bashcut.palette.run", "params": {"item": "command:no.such"}}));
    assert_ne!(bad["ok"], json!(true));
    let bad = d.call("engine.execute", json!({"command": "bashcut.palette.run", "params": {}}));
    assert_ne!(bad["ok"], json!(true));
    let ids: Vec<String> = filmcraft_ui_egui::menus::external_commands().iter().map(|c| c.id.clone()).collect();
    assert!(ids.iter().any(|c| c == "bashcut.palette.open"));
}
