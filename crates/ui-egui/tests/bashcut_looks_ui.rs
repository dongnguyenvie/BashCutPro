//! Headless UI tests of BashCutPro's Looks section (#47, `bashcut/in-app-ui.md` §1a), driven
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

    fn click(&mut self, id: &str) {
        self.ok("ui.click", json!({"id": id}));
        self.frames(3);
    }

    fn first_clip(&mut self) -> u64 {
        let q = self.exec("sequence.inspect", json!({}));
        q["video"].as_array().unwrap().iter().flat_map(|t| t["items"].as_array().unwrap().iter()).next().expect("a video clip")["clip"].as_u64().unwrap()
    }

    fn looks_on(&mut self, clip: u64) -> usize {
        let q = self.exec("sequence.inspect", json!({}));
        let it = q["video"].as_array().unwrap().iter().flat_map(|t| t["items"].as_array().unwrap().iter()).find(|i| i["clip"] == json!(clip)).unwrap().clone();
        it["effects"].as_array().unwrap().iter().filter(|e| e["params"].get("bashcut_look").is_some()).count()
    }
}

#[test]
fn looks_section_lists_and_applies_to_the_selection() {
    let mut d = Driver::demo();
    d.click("bashcut.rail.looks");
    assert!(d.has("bashcut.panel.looks"));
    assert!(d.has("bashcut.looks.teal-orange"), "a card per look");
    assert!(d.has("bashcut.looks.intensity"));
    // thumbnails render a few per frame; let them finish
    d.frames(20);

    let clip = d.first_clip();
    d.exec("timeline.select", json!({"clips": [clip]}));
    d.frames(3);
    d.click("bashcut.looks.teal-orange.apply");
    assert_eq!(d.looks_on(clip), 1, "applied through the panel");
    d.click("bashcut.looks.golden-hour.apply");
    assert_eq!(d.looks_on(clip), 1, "a second look replaces the first");
    d.exec("edit.undo", json!({}));
    assert_eq!(d.looks_on(clip), 1);
    d.click("bashcut.looks.remove");
    assert_eq!(d.looks_on(clip), 0);

    // favourites and the frame preview are drivable too
    let r = d.exec("bashcut.ui.looks.favourite", json!({"look": "fresh-food"}));
    assert_eq!(r["favourite"], json!(true));
    assert_eq!(d.exec("bashcut.ui.looks.state", json!({}))["favourites"], json!(["fresh-food"]));
    let bad = d.call("engine.execute", json!({"command": "bashcut.ui.looks.favourite", "params": {"look": "nope"}}));
    assert_ne!(bad["ok"], json!(true));
    d.exec("bashcut.ui.looks.previewFrame", json!({"on": true}));
    d.frames(20);
    assert_eq!(d.exec("bashcut.ui.looks.state", json!({}))["onFrame"], json!(true));

    // the command palette finds looks
    let found = d.exec("bashcut.palette.search", json!({"query": "teal"}));
    assert!(found.as_array().unwrap().iter().any(|r| r["key"] == json!("look:teal-orange")), "{found}");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
}
