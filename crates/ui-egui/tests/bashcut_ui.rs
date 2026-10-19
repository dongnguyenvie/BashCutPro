//! Headless UI tests of BashCutPro's rail and side panel (`bashcut/in-app-ui.md`), driven by
//! automation id through the control channel like upstream's UI tests. Fork-only file.

#![allow(dead_code)]

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

const W: f32 = 1600.0;
const H: f32 = 980.0;

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
        let harness = Harness::builder().with_size(egui::vec2(W, H)).with_max_steps(10_000).build_eframe(move |_cc| app);
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

    fn click(&mut self, id: &str) {
        self.ok("ui.click", json!({"id": id}));
        self.frames(3);
    }

    fn rect(&mut self, id: &str) -> Option<[f32; 4]> {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        let e = v.as_array()?.iter().find(|e| e["id"] == json!(id))?.clone();
        let r = e["rect"].as_array()?;
        Some([0, 1, 2, 3].map(|i| r[i].as_f64().unwrap_or(0.0) as f32))
    }

    fn has(&mut self, id: &str) -> bool {
        self.rect(id).is_some()
    }

    fn state(&mut self) -> Value {
        self.exec("bashcut.ui.state", json!({}))
    }
}

#[test]
fn rail_shows_every_section_and_no_panel_at_start() {
    let mut d = Driver::demo();
    for id in ["agent", "plugins", "runs"] {
        let r = d.rect(&format!("bashcut.rail.{id}")).unwrap_or_else(|| panic!("no rail button {id}"));
        assert!(r[0] > W - 40.0, "rail on the right edge: {r:?}");
    }
    assert!(!d.has("bashcut.panel.agent"));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
}

#[test]
fn click_opens_resizes_pops_out_docks_back_and_closes() {
    let mut d = Driver::demo();
    d.click("bashcut.rail.agent");
    let panel = d.rect("bashcut.panel.agent").expect("agent panel open");
    assert!(panel[2] >= 240.0 && panel[2] <= 720.0, "{panel:?}");
    assert_eq!(d.state()["open"], json!("agent"));

    // resize by command (clamped) and by dragging the grip
    assert_eq!(d.exec("bashcut.ui.setWidth", json!({"width": 5000})), json!(720.0));
    d.frames(2);
    let wide = d.rect("bashcut.panel.agent").unwrap();
    assert!((wide[2] - 720.0).abs() < 2.0, "{wide:?}");
    d.exec("bashcut.ui.setWidth", json!({"width": 300}));
    d.frames(2);
    let narrow = d.rect("bashcut.panel.agent").unwrap();
    assert!((narrow[2] - 300.0).abs() < 2.0, "{narrow:?}");
    let grip = d.rect("bashcut.panel.resize").unwrap();
    let (x, y) = (grip[0] + grip[2] / 2.0, grip[1] + grip[3] / 2.0);
    d.ok("ui.drag", json!({"from": {"x": x, "y": y}, "to": {"x": x - 100.0, "y": y}}));
    d.frames(4);
    let dragged = d.rect("bashcut.panel.agent").unwrap();
    assert!((dragged[2] - 400.0).abs() < 8.0, "drag widens the panel: {dragged:?}");

    // switching section keeps one panel
    d.click("bashcut.rail.runs");
    assert!(d.has("bashcut.panel.runs"));
    assert!(!d.has("bashcut.panel.agent"));

    // pop out into a window, then dock back
    d.click("bashcut.panel.popout");
    assert!(!d.has("bashcut.panel.runs"));
    assert!(d.has("bashcut.window.runs"), "floating window");
    assert_eq!(d.state()["popped"], json!(["runs"]));
    d.click("bashcut.window.runs.dock");
    assert!(d.has("bashcut.panel.runs"));
    assert!(!d.has("bashcut.window.runs"));

    d.click("bashcut.panel.close");
    assert!(!d.has("bashcut.panel.runs"));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
}

#[test]
fn panel_takes_width_from_the_dock() {
    let mut d = Driver::demo();
    let before: Vec<String> = d.ok("ui.elements", json!({"prefix": "timeline."})).as_array().unwrap().iter().map(|e| e["id"].to_string()).collect();
    assert!(!before.is_empty());
    d.click("bashcut.rail.plugins");
    let panel = d.rect("bashcut.panel.plugins").unwrap();
    let els = d.ok("ui.elements", json!({"prefix": "timeline."}));
    for e in els.as_array().unwrap() {
        let x = e["rect"][0].as_f64().unwrap() as f32;
        assert!(x < panel[0], "timeline element {} drawn under the panel", e["id"]);
    }
}

#[test]
fn focus_agent_command_and_menu() {
    let mut d = Driver::demo();
    d.exec("bashcut.ui.focusAgent", json!({}));
    assert!(d.has("bashcut.panel.agent"));
    d.exec("bashcut.ui.section", json!({"section": "agent", "open": false}));
    assert!(!d.has("bashcut.panel.agent"));
    d.exec("bashcut.ui.show.runs", json!({}));
    assert!(d.has("bashcut.panel.runs"));
    let bad = d.call("engine.execute", json!({"command": "bashcut.ui.section", "params": {"section": "nope"}}));
    assert_eq!(bad["ok"], json!(false));
    // the shortcut is registered and the Window menu lists the sections
    let items = filmcraft_ui_egui::menus::default_menu_items();
    assert!(items.iter().any(|i| i.id == "bashcut.ui.show.agent" && i.path == ["Window", "BashCutPro"]));
    assert!(filmcraft_ui_egui::menus::external_commands().iter().any(|c| c.id == "bashcut.ui.focusAgent"));
}

#[test]
fn rail_hides_in_export_mode_and_on_toggle() {
    let mut d = Driver::demo();
    d.exec("bashcut.ui.toggleRail", json!({}));
    assert!(!d.has("bashcut.rail.agent"));
    d.exec("bashcut.ui.toggleRail", json!({}));
    assert!(d.has("bashcut.rail.agent"));
    d.exec("mode.export", json!({}));
    assert!(!d.has("bashcut.rail.agent"), "no rail in Export mode");
}

#[test]
fn vietnamese_strings() {
    let mut d = Driver::demo();
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    d.click("bashcut.rail.plugins");
    let v = d.ok("ui.elements", json!({"prefix": "bashcut.panel.title"}));
    assert_eq!(v[0]["label"], json!("Plugin"));
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
}

#[test]
fn plugins_and_runs_sections_draw_and_commands_answer() {
    let mut d = Driver::demo();
    d.click("bashcut.rail.plugins");
    assert!(d.has("bashcut.panel.plugins"));
    assert!(d.has("bashcut.plugins.registry.refresh"), "registry button");
    // a headless session has no data folder: nothing to install, listed as such
    let list = d.exec("bashcut.plugins.list", json!({}));
    assert_eq!(list["installed"], json!([]));
    assert_eq!(list["canInstall"], json!(false));
    let v = d.call("engine.execute", json!({"command": "bashcut.plugins.run", "params": {"plugin": "nope", "action": "x"}}));
    assert_ne!(v["ok"], json!(true), "unknown plugin: {v}");
    let v = d.call("engine.execute", json!({"command": "bashcut.plugins.cancel", "params": {}}));
    assert_ne!(v["ok"], json!(true), "job is required: {v}");
    assert_eq!(d.exec("bashcut.plugins.runs", json!({})), json!([]));
    d.click("bashcut.rail.runs");
    assert!(d.has("bashcut.panel.runs"));
    assert!(!d.has("bashcut.runs.clear"), "nothing ran");
    let ids: Vec<String> = filmcraft_ui_egui::menus::external_commands().iter().map(|c| c.id.clone()).collect();
    for id in ["bashcut.plugins.list", "bashcut.plugins.run", "bashcut.plugins.cancel", "bashcut.plugins.sendToAgent"] {
        assert!(ids.iter().any(|c| c == id), "{id} listed");
    }
}

#[test]
fn agent_section_draws_and_gates_show_as_cards() {
    let mut d = Driver::demo();
    d.click("bashcut.rail.agent");
    for id in ["bashcut.agent.input", "bashcut.agent.send", "bashcut.agent.stop", "bashcut.agent.picker", "bashcut.agent.transcript"] {
        assert!(d.has(id), "{id}");
    }
    assert!(!d.has("bashcut.gate.approve"), "no plan, no gate");
    let st = d.exec("bashcut.agent.state", json!({}));
    assert_eq!(st["agent"]["id"], json!("claude-code"));
    assert_eq!(st["status"]["state"], json!("idle"));
    assert!(d.exec("bashcut.agent.agents", json!({})).as_array().is_some_and(|a| !a.is_empty()));
    // no control channel in a headless test: Claude Code cannot be started, and nothing is shown for it
    let v = d.call("engine.execute", json!({"command": "bashcut.agent.send", "params": {"text": "cut the intro"}}));
    assert_ne!(v["ok"], json!(true), "{v}");
    assert_eq!(d.exec("bashcut.agent.transcript", json!({}))["items"], json!([]));

    // a saved project with a pending gate in its sidecar
    let dir = std::env::temp_dir().join(format!("bashcut-ui-agent-{}", std::process::id()));
    let project = dir.join("film.fcproj");
    let side = dir.join("film.fcproj.bashcut");
    std::fs::create_dir_all(&side).unwrap();
    std::fs::write(side.join("plan.json"), r#"{"gates":[{"stage":"rough cut","summary":"9 clips","evidence":["cut list"],"state":"pending"}]}"#).unwrap();
    d.harness.state_mut().session.path = Some(project.to_string_lossy().into_owned());
    d.frames(3);
    assert!(d.has("bashcut.gate.approve"), "gate card");
    assert!(d.has("bashcut.gate.change"));
    assert_eq!(d.exec("bashcut.agent.gate", json!({}))["stage"], json!("rough cut"));
    // the answer is kept only when it can be sent
    let v = d.call("engine.execute", json!({"command": "bashcut.agent.approve", "params": {}}));
    assert_ne!(v["ok"], json!(true), "{v}");
    assert_eq!(d.exec("bashcut.agent.gate", json!({}))["stage"], json!("rough cut"), "still pending");

    let ids: Vec<String> = filmcraft_ui_egui::menus::external_commands().iter().map(|c| c.id.clone()).collect();
    for id in ["bashcut.agent.send", "bashcut.agent.stop", "bashcut.agent.approve", "bashcut.agent.change", "bashcut.agent.conversations"] {
        assert!(ids.iter().any(|c| c == id), "{id} listed");
    }
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stand-in for `filmcraft-cli` in Python: `run <script>` with `--save-as` / `--project --save`,
/// `export <out>`. Script id `fake.needs` fails (exit 1, a failed JSON line) while `params.file`
/// is missing; every other id succeeds.
#[cfg(unix)]
const FAKE_CLI: &str = r#"#!/usr/bin/env python3
import json, os, sys
VALUED = {"--project", "--save-as", "--preset", "--format", "--settings", "--data-dir"}
pos, opts, args = [], {}, sys.argv[1:]
i = 0
while i < len(args):
    a = args[i]
    if a in VALUED:
        opts[a] = args[i + 1] if i + 1 < len(args) else ""
        i += 2
        continue
    if a.startswith("--"):
        opts[a] = True
    else:
        pos.append(a)
    i += 1
project = open(opts["--project"]).read() if "--project" in opts else ""
if pos[:1] == ["run"]:
    for n, line in enumerate(l for l in open(pos[1]).read().splitlines() if l.strip()):
        v = json.loads(line)
        if v.get("id") == "fake.needs" and not os.path.exists(v.get("params", {}).get("file", "")):
            print(json.dumps({"line": n + 1, "id": v["id"], "ok": False, "error": "fake failure"}))
            sys.exit(1)
        print(json.dumps({"line": n + 1, "id": v.get("id"), "ok": True, "result": None}))
        project += str(v.get("id")) + "\n"
    target = opts.get("--save-as") or (opts.get("--project") if "--save" in opts else None)
    if target:
        open(target, "w").write(project)
    sys.exit(0)
if pos[:1] == ["export"]:
    open(pos[1], "w").write("export\n" + project)
    print(json.dumps({"path": pos[1]}))
    sys.exit(0)
sys.exit(2)
"#;

#[cfg(unix)]
#[test]
fn batch_section_runs_a_batch_with_a_fake_cli() {
    use std::os::unix::fs::PermissionsExt;
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        eprintln!("python3 not found: skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bashcut-ui-batch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("clips")).unwrap();
    for c in ["a", "b"] {
        std::fs::write(dir.join("clips").join(format!("{c}.mp4")), b"media").unwrap();
    }
    // `a` may go on, `b` fails until its file appears
    std::fs::write(dir.join("a.ok"), b"").unwrap();
    let cli = dir.join("fake-cli");
    std::fs::write(&cli, FAKE_CLI).unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cli = cli.to_string_lossy().into_owned();

    let mut d = Driver::demo();
    d.click("bashcut.rail.batch");
    for id in ["bashcut.panel.batch", "bashcut.batch.drop", "bashcut.batch.run", "bashcut.batch.step.search"] {
        assert!(d.has(id), "{id}");
    }
    let v = d.call("engine.execute", json!({"command": "bashcut.batch.run", "params": {"cli": cli}}));
    assert_ne!(v["ok"], json!(true), "no inputs: {v}");
    let v = d.call("engine.execute", json!({"command": "bashcut.batch.addStep", "params": {"command": "no.such.command"}}));
    assert_ne!(v["ok"], json!(true), "only engine commands: {v}");

    let clips = dir.join("clips");
    let added = d.exec("bashcut.batch.addInput", json!({"paths": [clips.join("a.mp4"), clips.join("b.mp4")]}));
    assert_eq!(added["added"], json!(2));
    let needs = dir.join("{name}.ok").to_string_lossy().into_owned();
    d.exec("bashcut.batch.addStep", json!({"step": {"command": "fake.needs", "params": {"file": needs}}, "label": "Check"}));
    d.exec("bashcut.batch.addOutput", json!({"output": {"name": "{name}.mp4"}}));
    d.exec("bashcut.batch.set", json!({"name": "ui", "projectsFolder": dir.join("out").to_string_lossy()}));
    assert!(d.has("bashcut.batch.step.0"), "step row");
    assert!(d.has("bashcut.batch.output.0.remove"), "output row");

    let finished = |d: &mut Driver| {
        for _ in 0..600 {
            let st = d.exec("bashcut.batch.state", json!({}));
            if st["running"] == json!(false) {
                return st;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("the batch did not finish");
    };
    d.exec("bashcut.batch.run", json!({"cli": cli}));
    let st = finished(&mut d);
    assert_eq!(st["counts"]["done"], json!(1), "{st}");
    assert_eq!(st["counts"]["failed"], json!(1), "{st}");
    let b = st["items"].as_array().unwrap().iter().find(|i| i["name"] == json!("b")).unwrap().clone();
    assert_eq!(b["status"], json!("failed"), "{b}");
    d.frames(3);
    assert!(d.has("bashcut.batch.item.b.error"), "the failure is shown");
    assert!(d.has("bashcut.batch.item.b.retry"), "retry on the failed item");
    assert!(d.has("bashcut.batch.item.a.reveal"));

    // a finished item reveals its output
    d.exec("bashcut.batch.reveal", json!({"item": "a"}));
    assert!(!d.harness.state().ui.extras.opened.is_empty(), "revealed");

    std::fs::write(dir.join("b.ok"), b"").unwrap();
    let r = d.exec("bashcut.batch.retry", json!({"items": ["b"]}));
    assert_eq!(r["reopened"], json!(1), "{r}");
    let st = finished(&mut d);
    assert_eq!(st["counts"]["done"], json!(2), "{st}");
    let b = st["items"].as_array().unwrap().iter().find(|i| i["name"] == json!("b")).unwrap().clone();
    assert!(b["outputs"].as_array().is_some_and(|o| o.iter().filter_map(Value::as_str).any(|o| std::path::Path::new(o).exists())), "{b}");
    let v = d.call("engine.execute", json!({"command": "bashcut.batch.open", "params": {"item": "nope"}}));
    assert_ne!(v["ok"], json!(true), "unknown item: {v}");

    let ids: Vec<String> = filmcraft_ui_egui::menus::external_commands().iter().map(|c| c.id.clone()).collect();
    for id in ["bashcut.batch.run", "bashcut.batch.cancel", "bashcut.batch.retry", "bashcut.batch.addInput", "bashcut.batch.prefill"] {
        assert!(ids.iter().any(|c| c == id), "{id} listed");
    }
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&dir);
}
