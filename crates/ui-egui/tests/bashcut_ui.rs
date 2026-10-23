//! Headless UI tests of BashCutPro's rail and side panel (`bashcut/in-app-ui.md`), driven by
//! automation id through the control channel like upstream's UI tests. Fork-only file.

#![allow(dead_code)]

use std::path::Path;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use bashcut_plugin_host::settings::{MemoryStore, SecretStore};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use filmcraft_engine::Session;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

const W: f32 = 1600.0;
const H: f32 = 980.0;
/// Upper bound of [`Driver::wait`]: about a minute even on a loaded machine running tests in parallel.
const WAIT_TRIES: usize = 3000;

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

    /// A session saving into `data` (so plugins install there) with the demo project open,
    /// `setup` applied to the app, and the registry pointed at an empty `data/registry.json`
    /// (`bashcut.plugins.registry {location}`) before anything draws: Plugins' first-open check
    /// reads that file and nothing goes to the network.
    fn with_data(data: &Path, setup: impl FnOnce(&mut FilmcraftApp)) -> Self {
        let mut d = Self::with_env_registry(data, setup);
        Self::point_registry(&mut d, data);
        d
    }

    /// Point the registry at an empty `data/registry.json` (see [`Driver::with_data`]).
    fn point_registry(d: &mut Self, data: &Path) {
        std::fs::create_dir_all(data).unwrap();
        let path = data.join("registry.json");
        std::fs::write(&path, json!({"schema": "bashcut-registry/1", "name": "test", "plugins": []}).to_string()).unwrap();
        d.exec("bashcut.plugins.registry", json!({"location": path.to_string_lossy()}));
        let at = d.exec("bashcut.plugins.list", json!({}))["location"].clone();
        assert!(at.as_str().is_some_and(|l| l.starts_with(&*path.to_string_lossy())), "the registry is the local file: {at}");
    }

    /// Like [`Driver::with_data`] but the registry is whatever `BASHCUT_PLUGIN_REGISTRY` names;
    /// only for a test that sets it to a local file (a child test process, never the network).
    fn with_env_registry(data: &Path, setup: impl FnOnce(&mut FilmcraftApp)) -> Self {
        Self::build(data, setup, |_| {})
    }

    /// [`Driver::with_data`] with `secrets` behind the app's key store cache instead of the OS
    /// key store (`plugins::use_key_store`, before the first frame).
    fn with_key_store(data: &Path, secrets: std::sync::Arc<dyn SecretStore>) -> Self {
        let mut d = Self::build(data, |_| {}, |ctx| assert!(filmcraft_ui_egui::bashcut_ui::plugins::use_key_store(ctx, secrets), "no host yet"));
        Self::point_registry(&mut d, data);
        d
    }

    fn build(data: &Path, setup: impl FnOnce(&mut FilmcraftApp), before: impl FnOnce(&egui::Context)) -> Self {
        let mut session = Session::default();
        session.start_autosave(filmcraft_engine::autosave::AutosaveConfig::new(data)).expect("data dir");
        session.execute("file.openDemoProject", json!({})).expect("demo project");
        let (tx, rx) = channel();
        let mut app = FilmcraftApp::new(session).with_control(rx);
        setup(&mut app);
        let harness = Harness::builder().with_size(egui::vec2(W, H)).with_max_steps(10_000).build_eframe(move |_cc| app);
        before(&harness.ctx);
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

    /// The automation entry `id` (`Value::Null` when it is not on screen).
    fn element(&mut self, id: &str) -> Value {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        v.as_array().and_then(|a| a.iter().find(|e| e["id"] == json!(id)).cloned()).unwrap_or(Value::Null)
    }

    /// Read `read` until `done` holds for it, a frame (plus 10 ms for background threads and child
    /// processes) between tries, at most [`WAIT_TRIES`] times; the last value comes back either way,
    /// so the caller's assert shows what was there. Every wait on async state goes through here.
    fn wait(&mut self, mut read: impl FnMut(&mut Self) -> Value, done: impl Fn(&Value) -> bool) -> Value {
        let mut v = read(self);
        for _ in 0..WAIT_TRIES {
            if done(&v) {
                break;
            }
            self.frames(1);
            std::thread::sleep(Duration::from_millis(10));
            v = read(self);
        }
        v
    }

    /// The entry `id` once `done` holds for it (see [`Driver::wait`]).
    fn wait_element(&mut self, id: &str, done: impl Fn(&Value) -> bool) -> Value {
        self.wait(|d| d.element(id), done)
    }

    /// Plugin job `job` once it is no longer running.
    fn wait_job(&mut self, job: u64) -> Value {
        self.wait(|d| d.exec("bashcut.plugins.job", json!({"job": job})), |j| j["state"] != json!("running"))
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
    let w = d.rect("bashcut.window.runs").unwrap();
    assert!(w[1] >= 40.0 && w[0] + w[2] <= W - 36.0, "first pop-out sits where the panel was, clear of header and rail: {w:?}");
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

/// Window ▸ BashCutPro ▸ Language: three checked items that switch the panels at once (UX11).
#[test]
fn language_menu_switches_and_checks_the_current_language() {
    let checked = || {
        let items = filmcraft_ui_egui::menus::default_menu_items();
        let mine: Vec<_> = items.into_iter().filter(|i| i.id.starts_with("bashcut.ui.setLanguage.")).collect();
        assert_eq!(mine.len(), 3, "Follow App, English, Tiếng Việt");
        let on: Vec<String> = mine.iter().filter(|i| i.checked == Some(true)).map(|i| i.id.clone()).collect();
        assert_eq!(on.len(), 1, "one check: {on:?}");
        (on[0].clone(), mine)
    };
    let mut d = Driver::demo();
    d.click("bashcut.rail.plugins");
    let (on, mine) = checked();
    assert_eq!(on, "bashcut.ui.setLanguage.follow");
    assert!(mine.iter().all(|i| i.path == ["Window", "BashCutPro", "Language"]), "{:?}", mine[0].path);

    d.exec("bashcut.ui.setLanguage.vi", json!({}));
    let v = d.ok("ui.elements", json!({"prefix": "bashcut.panel.title"}));
    assert_eq!(v[0]["label"], json!("Plugin"), "the open panel switched");
    assert_eq!(d.state()["language"], json!("vi"), "saved in the rail state");
    let (on, mine) = checked();
    assert_eq!(on, "bashcut.ui.setLanguage.vi");
    assert!(mine.iter().all(|i| i.path == ["Window", "BashCutPro", "Ngôn ngữ"]), "the submenu reads in Vietnamese");
    assert!(mine.iter().any(|i| i.label == "Theo ứng dụng"));

    d.exec("bashcut.ui.setLanguage.en", json!({}));
    assert_eq!(checked().0, "bashcut.ui.setLanguage.en");
    let v = d.ok("ui.elements", json!({"prefix": "bashcut.panel.title"}));
    assert_eq!(v[0]["label"], json!("Plugins"));
    d.exec("bashcut.ui.setLanguage.follow", json!({}));
    assert_eq!(checked().0, "bashcut.ui.setLanguage.follow");
    assert_eq!(d.state()["language"], json!(null));
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
    // the empty state points somewhere, and the button really switches sections
    d.click("bashcut.runs.openPlugins");
    assert!(d.has("bashcut.panel.plugins"), "Open Plugins switched to Plugins");
    assert!(!d.has("bashcut.panel.runs"));
    d.click("bashcut.panel.close");
    assert!(!d.has("bashcut.panel.plugins"), "close still closes");
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
    // the empty transcript offers three kit recipes as example prompts; a click fills, never sends
    for id in ["vlog-plan", "vlog-hook-script", "vlog-publish"] {
        assert!(d.has(&format!("bashcut.agent.example.{id}")), "{id} example");
    }
    d.click("bashcut.agent.example.vlog-plan");
    d.frames(2);
    assert_eq!(d.exec("bashcut.agent.transcript", json!({}))["items"], json!([]), "not sent");
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
    d.wait_element("bashcut.gate.approve", |e| !e.is_null());
    assert!(d.has("bashcut.gate.approve"), "gate card");
    assert!(d.has("bashcut.gate.change"));
    assert_eq!(d.exec("bashcut.agent.gate", json!({}))["stage"], json!("rough cut"));
    // the answer is kept only when it can be sent
    let v = d.call("engine.execute", json!({"command": "bashcut.agent.approve", "params": {}}));
    assert_ne!(v["ok"], json!(true), "{v}");
    assert_eq!(d.exec("bashcut.agent.gate", json!({}))["stage"], json!("rough cut"), "still pending");

    // a long conversation title truncates: New stays inside the panel
    let conv = side.join("conversations");
    std::fs::create_dir_all(&conv).unwrap();
    let title = "Cut the intro to ten seconds and add captions in Vietnamese for every speaker";
    std::fs::write(conv.join("c1-0.json"), json!({"schema": "bashcut-conversation/1", "id": "c1-0", "title": title, "agent": "claude-code"}).to_string())
        .unwrap();
    d.exec("bashcut.agent.open", json!({"id": "c1-0"}));
    d.frames(3);
    let panel = d.rect("bashcut.panel.agent").unwrap();
    for id in ["bashcut.agent.conversations", "bashcut.agent.new"] {
        let r = d.rect(id).unwrap_or_else(|| panic!("no {id}"));
        assert!(r[0] + r[2] <= panel[0] + panel[2] + 0.5, "{id} {r:?} spills out of the panel {panel:?}");
    }

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

    let finished = |d: &mut Driver| d.wait(|d| d.exec("bashcut.batch.state", json!({})), |st| st["running"] == json!(false));
    d.exec("bashcut.batch.run", json!({"cli": cli}));
    let st = finished(&mut d);
    assert_eq!(st["counts"]["done"], json!(1), "{st}");
    assert_eq!(st["counts"]["failed"], json!(1), "{st}");
    let b = st["items"].as_array().unwrap().iter().find(|i| i["name"] == json!("b")).unwrap().clone();
    assert_eq!(b["status"], json!("failed"), "{b}");
    d.wait_element("bashcut.batch.item.b.error", |e| !e.is_null());
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

/// A plugin with a `view` action: `review` shows a form (input, toggle, select, list with row
/// buttons, button, progress, text) and stays up until `stop` is created; `accept` echoes its args.
const VIEW_PLUGIN: &str = r#"
import json, os, sys, time
mode = sys.argv[1]
if mode == "review":
    rows = [
        {"type": "text", "text": "2 captions to review"},
        {"type": "input", "id": "lang", "label": "Language", "value": "vi"},
        {"type": "toggle", "id": "keep", "label": "Keep timing", "value": True},
        {"type": "select", "id": "tone", "label": "Tone", "value": "plain", "choices": ["plain", "formal"]},
        {"type": "list", "id": "lines", "items": [
            {"id": "c1", "text": "Xin chào", "detail": "Hello", "buttons": [{"id": "ok", "label": "Accept", "action": "accept"}]},
            {"id": "c2", "text": "Tạm biệt", "detail": "Bye", "buttons": [{"id": "ok", "label": "Accept", "action": "accept"}]}]},
        {"type": "button", "id": "all", "label": "Accept all", "action": "accept", "params": {"all": True}},
        {"type": "image", "path": "missing.png", "caption": "frame"},
        {"type": "progress", "value": 0.5, "label": "translating"},
    ]
    print(json.dumps({"schema": "bashcut-view/1", "title": "Review", "rows": rows}), flush=True)
    for _ in range(600):
        if os.path.exists("stop"):
            break
        time.sleep(0.05)
    print(json.dumps({"ok": True, "message": "reviewed"}))
else:
    print(json.dumps({"ok": True, "message": "accepted", "data": {"args": sys.argv[2:]}}))
"#;

#[test]
fn a_plugin_view_draws_its_rows_and_buttons_run_actions() {
    if !has_python() {
        eprintln!("python not found: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-view-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.views");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.py"), VIEW_PLUGIN).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.views", "name": "Views", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]}, "output": "bashcut-result/1",
        "actions": [
            {"id": "review", "title": "Review", "args": ["review"], "needs": "none", "view": true},
            {"id": "accept", "title": "Accept", "args": ["accept"], "needs": "none", "params": [
                {"id": "lang", "flag": "--lang", "type": "string", "optional": true},
                {"id": "item", "flag": "--item", "type": "string", "optional": true},
                {"id": "all", "flag": "--all", "type": "bool", "optional": true}]}
        ]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();

    let mut d = Driver::with_data(&data, |_| {});

    assert_eq!(d.exec("bashcut.plugins.view", json!({"plugin": "test.views"})), Value::Null);
    d.exec("bashcut.plugins.run", json!({"plugin": "test.views", "action": "review"}));
    let shown = d.wait(|d| d.exec("bashcut.plugins.view", json!({"plugin": "test.views"})), |v| !v.is_null());
    assert_eq!(shown["view"]["rows"].as_array().map(Vec::len), Some(8), "{shown}");
    let review = shown["job"].as_u64().unwrap();
    assert_eq!(d.state()["open"], json!("plugins"), "a view action opens its plugin's card");
    d.wait_element("bashcut.view.test.views.row7", |e| !e.is_null());
    for id in [
        "bashcut.view.test.views.title",
        "bashcut.view.test.views.row0",
        "bashcut.view.test.views.lang",
        "bashcut.view.test.views.keep",
        "bashcut.view.test.views.tone",
        "bashcut.view.test.views.lines.c1",
        "bashcut.view.test.views.lines.c2.ok",
        "bashcut.view.test.views.all",
        "bashcut.view.test.views.row6",
        "bashcut.view.test.views.row7",
    ] {
        assert!(d.has(id), "{id}");
    }

    // a row button: its item and the form's values the action takes
    d.click("bashcut.view.test.views.lines.c2.ok");
    let runs = d.exec("bashcut.plugins.runs", json!({}));
    let accept = runs.as_array().unwrap().iter().filter_map(|r| r["id"].as_u64()).max().unwrap();
    assert_ne!(accept, review, "the click started a run");
    let job = d.wait_job(accept);
    assert_eq!(job["state"], json!("succeeded"), "{job}");
    assert_eq!(job["outcome"]["result"]["data"]["args"], json!(["--lang", "vi", "--item", "c2"]));
    // the control channel presses buttons with its own values
    let r = d.exec("bashcut.plugins.click", json!({"plugin": "test.views", "button": "all", "values": {"lang": "en"}}));
    assert!(r["job"].as_u64().is_some(), "{r}");
    let v = d.call("engine.execute", json!({"command": "bashcut.plugins.click", "params": {"plugin": "test.views", "button": "nope"}}));
    assert_ne!(v["ok"], json!(true), "unknown button: {v}");

    std::fs::write(plugin.join("stop"), b"").unwrap();
    let ids: Vec<String> = filmcraft_ui_egui::menus::external_commands().iter().map(|c| c.id.clone()).collect();
    for id in ["bashcut.plugins.view", "bashcut.plugins.click"] {
        assert!(ids.iter().any(|c| c == id), "{id} listed");
    }
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn plugin_action_forms_follow_the_manifest() {
    if !has_python() {
        eprintln!("python not found: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-forms-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.forms");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.py"), "import json, sys\nprint(json.dumps({\"ok\": True, \"data\": {\"args\": sys.argv[1:]}}))\n").unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.forms", "name": "Forms", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]}, "output": "bashcut-result/1",
        "actions": [
            {"id": "go", "title": "Go", "args": ["go"], "needs": "none", "params": [
                {"id": "target", "flag": "--target", "type": "enum", "default": "vi", "choices": ["vi", "en"]},
                {"id": "maxCps", "flag": "--max-cps", "type": "integer", "minimum": 8, "maximum": 30, "default": 17},
                {"id": "answer", "flag": "--answer", "type": "path"},
                {"id": "out_dir", "flag": "--out-dir", "type": "string", "optional": true, "title": "Save to"}]}
        ]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();

    let mut d = Driver::with_data(&data, |app| app.hooks.pick_files = Some(Box::new(|_| vec!["/tmp/answer.json".to_string()])));

    d.click("bashcut.rail.plugins");
    d.click("bashcut.plugins.test.forms");
    d.click("bashcut.plugins.test.forms.action.go");
    let param = "bashcut.plugins.test.forms.action.go.param";
    assert_eq!(d.element(&format!("{param}.target"))["label"], json!("Target"));
    assert_eq!(d.element(&format!("{param}.maxCps"))["label"], json!("Max cps"));
    assert_eq!(d.element(&format!("{param}.out_dir"))["label"], json!("Save to"), "a title wins over the id");
    // `answer` is required and empty: Run waits for it
    assert_eq!(d.element("bashcut.plugins.test.forms.go.run")["enabled"], json!(false));
    // an integer field takes no letters
    d.click(&format!("{param}.maxCps"));
    d.ok("ui.type", json!({"text": "abc"}));
    d.ok("ui.key", json!({"key": "Enter"}));
    d.frames(3);
    // Browse… fills the path (the picker answers in a later frame)
    d.click(&format!("{param}.answer.browse"));
    let run = d.wait_element("bashcut.plugins.test.forms.go.run", |e| e["enabled"] == json!(true));
    assert_eq!(run["enabled"], json!(true), "the path was filled");
    d.click("bashcut.plugins.test.forms.go.run");
    let runs = d.exec("bashcut.plugins.runs", json!({}));
    let id = runs.as_array().unwrap().iter().filter_map(|r| r["id"].as_u64()).max().expect("a run started");
    let job = d.wait_job(id);
    assert_eq!(job["state"], json!("succeeded"), "{job}");
    let args = job["outcome"]["result"]["data"]["args"].clone();
    assert_eq!(args, json!(["go", "--target", "vi", "--max-cps", "17", "--answer", "/tmp/answer.json"]), "{job}");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// Started without `--control` (as every test is): the Plugins section says so before any click,
/// a project action's Run waits for the channel with the reason, and a plugin whose interpreter
/// is missing shows the sentence, not `os error 2`. `BASHCUT_UI_SHOTS=<dir>` saves both languages.
#[test]
fn plugins_say_the_control_channel_is_off_and_errors_read_as_sentences() {
    if std::env::var_os("FILMCRAFT_CONTROL_PORT").is_some() {
        eprintln!("FILMCRAFT_CONTROL_PORT is set: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-hints-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.hints");
    std::fs::create_dir_all(&plugin).unwrap();
    // an interpreter no machine has: starting it fails like a missing python3
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.hints", "name": "Hints", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3-bashcut-missing", "plugin.py"]}, "output": "bashcut-result/1",
        "connect": {"bridge": ["--bridge", "{address}"]},
        "actions": [
            {"id": "translate", "title": "Translate captions…", "args": ["translate"], "needs": "project", "params": [
                {"id": "target", "flag": "--target", "type": "enum", "default": "vi", "choices": ["vi", "en"]}]},
            {"id": "describe", "title": "Describe shots", "args": ["describe"], "needs": "project"},
            {"id": "check", "title": "Check the key", "args": ["check"], "needs": "none"}
        ]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();

    let mut d = Driver::with_data(&data, |_| {});

    d.click("bashcut.rail.plugins");
    let setup = d.element("bashcut.plugins.setup");
    assert!(setup["label"].as_str().is_some_and(|l| l.starts_with("Control channel is off")), "{setup}");
    d.click("bashcut.plugins.test.hints");
    assert_eq!(d.element("bashcut.plugins.test.hints.action.describe")["enabled"], json!(false), "▶ waits for the channel");
    assert!(d.has("bashcut.plugins.test.hints.action.describe.blocked"), "the reason sits beside it");
    assert_eq!(d.element("bashcut.plugins.test.hints.describe.batch")["enabled"], json!(true), "a batch needs no channel");
    d.click("bashcut.plugins.test.hints.action.translate");
    assert_eq!(d.element("bashcut.plugins.test.hints.translate.run")["enabled"], json!(false), "Run waits for the channel");
    assert_eq!(d.element("bashcut.plugins.test.hints.translate.batch")["enabled"], json!(true));
    if let Some(dir) = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        for lang in ["en", "vi"] {
            d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
            d.frames(3);
            d.harness.render().unwrap().save(dir.join(format!("plugins-control-off-{lang}.png"))).unwrap();
        }
        d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    }
    // a needs-none action runs, and its spawn error reads as a sentence in its card
    d.click("bashcut.plugins.test.hints.action.check");
    assert!(!d.has("bashcut.plugins.status"), "not at the top of the panel");
    // the spawn fails on the host's thread: the card says so a frame or more later
    let status = d.wait_element("bashcut.plugins.test.hints.last", |e| e["label"].as_str().is_some_and(|l| l.starts_with("Failed")));
    let text = status["label"].as_str().unwrap_or_default();
    assert!(text.starts_with("Failed: Python 3 was not found") && text.ends_with("See Troubleshooting."), "{status}");
    assert!(!text.contains("os error"), "{text}");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// `fail` reports a coded error (exit 1), `make` names a file it wrote (`data.path`).
const RUNS_PLUGIN: &str = r#"
import json, sys
if sys.argv[1] == "fail":
    print(json.dumps({"ok": False, "error": {"code": "no_key", "message": "set the API key in settings"}}))
    sys.exit(1)
print(json.dumps({"ok": True, "message": "made", "data": {"path": "/out/made.wav"}}))
"#;

#[test]
fn a_failed_run_shows_its_error_and_retries_and_a_result_path_shows_reveal() {
    if !has_python() {
        eprintln!("python not found: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-runs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.runs");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.py"), RUNS_PLUGIN).unwrap();
    let word = json!([{"id": "word", "flag": "--word", "type": "string", "optional": true}]);
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.runs", "name": "Runs", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]}, "output": "bashcut-result/1",
        "actions": [
            {"id": "fail", "title": "Fail", "args": ["fail"], "needs": "none", "params": word},
            {"id": "make", "title": "Make", "args": ["make"], "needs": "none", "params": []}
        ]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();

    let mut d = Driver::with_data(&data, |_| {});

    let failed = d.exec("bashcut.plugins.run", json!({"plugin": "test.runs", "action": "fail", "params": {"word": "hi"}}))["job"].as_u64().unwrap();
    assert_eq!(d.wait_job(failed)["state"], json!("failed"));
    // a run no longer switches to Runs (UX5): open it
    d.click("bashcut.rail.runs");
    assert_eq!(d.state()["open"], json!("runs"));
    let error = d.wait_element(&format!("bashcut.runs.{failed}.error"), |e| !e.is_null());
    assert!(error.to_string().contains("no_key: set the API key in settings"), "{error}");
    assert!(d.has(&format!("bashcut.runs.{failed}.copyLog")));
    assert!(!d.has(&format!("bashcut.runs.{failed}.reveal.0")));
    d.click(&format!("bashcut.runs.{failed}.retry"));
    let runs = d.exec("bashcut.plugins.runs", json!({}));
    let again = runs.as_array().unwrap().iter().filter_map(|r| r["id"].as_u64()).max().unwrap();
    assert_ne!(again, failed, "Retry started a new run");
    let j = d.wait_job(again);
    assert_eq!(j["kind"], json!({"type": "run", "plugin": "test.runs", "action": "fail"}));
    assert_eq!(j["params"], json!({"word": "hi"}), "the same params");

    let made = d.exec("bashcut.plugins.run", json!({"plugin": "test.runs", "action": "make"}))["job"].as_u64().unwrap();
    assert_eq!(d.wait_job(made)["state"], json!("succeeded"));
    let reveal = d.wait_element(&format!("bashcut.runs.{made}.reveal.0"), |e| !e.is_null());
    assert!(!reveal.is_null(), "data.path shows Reveal");
    assert!(!d.has(&format!("bashcut.runs.{made}.retry")) && !d.has(&format!("bashcut.runs.{made}.error")));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// UX5: the first open of Plugins checks the registry with no click and says how many plugins it
/// offers; Install reads "Installing…" (disabled) while that install runs, so a double click
/// starts one install; a failed run says why in its card, without opening Runs.
///
/// The first-open check reads `BASHCUT_PLUGIN_REGISTRY`, so the test runs itself again with it
/// pointing at a local registry whose plugin lives on a git server that never answers.
#[test]
fn plugin_cards_show_the_last_run_and_install_waits() {
    const LOCAL: &str = "BASHCUT_UI_TEST_LOCAL_REGISTRY";
    let Some(registry) = std::env::var_os(LOCAL).map(std::path::PathBuf::from) else {
        let data = std::env::temp_dir().join(format!("bashcut-ui-card-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        let registry = data.join("registry.json");
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(exe)
            .args(["--exact", "plugin_cards_show_the_last_run_and_install_waits", "--nocapture", "--test-threads=1"])
            .env(LOCAL, &registry)
            .env("BASHCUT_PLUGIN_REGISTRY", &registry)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&data);
        let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && log.contains("1 passed"), "{log}");
        return;
    };
    let data = registry.parent().unwrap().to_path_buf();
    // accepts git's connection and never answers: the install stays running until it is dropped
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = silent.local_addr().unwrap().port();
    let entry = |id: &str, name: &str| {
        json!({"id": id, "name": name, "version": "0.1.0", "summary": "test entry", "platforms": ["macos", "windows", "linux"],
               "source": {"type": "git", "repo": format!("git://127.0.0.1:{port}/plugins"), "ref": "main", "path": "p"}})
    };
    let reg = json!({"schema": "bashcut-registry/1", "name": "test", "plugins": [entry("test.slow", "Slow"), entry("test.other", "Other")]});
    std::fs::write(&registry, reg.to_string()).unwrap();
    let python = has_python();
    if python {
        let plugin = data.join("bashcut-plugins").join("test.card");
        std::fs::create_dir_all(&plugin).unwrap();
        let script =
            "import json, sys\nprint(json.dumps({\"ok\": False, \"error\": {\"code\": \"bad_params\", \"message\": \"no captions in the sequence\"}}))\n";
        std::fs::write(plugin.join("plugin.py"), script).unwrap();
        let manifest = json!({
            "schema": "bashcut-plugin/1", "id": "test.card", "name": "Card", "version": "0.1.0", "summary": "test plugin",
            "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]}, "output": "bashcut-result/1",
            "actions": [{"id": "fail", "title": "Fail", "args": ["fail"], "needs": "none"}]
        });
        std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    }

    let mut d = Driver::with_env_registry(&data, |_| {});
    let jobs = |d: &mut Driver, kind: &str| -> Vec<Value> {
        let runs = d.exec("bashcut.plugins.runs", json!({}));
        runs.as_array().cloned().unwrap_or_default().into_iter().filter(|r| r["kind"]["type"] == json!(kind)).collect()
    };

    // the first open checks the registry by itself, once
    d.exec("bashcut.ui.setLanguage", json!({"language": "en"}));
    d.click("bashcut.rail.plugins");
    let line = d.wait_element("bashcut.plugins.registry.status", |e| e["label"] == json!("2 plugins available"));
    assert_eq!(line["label"], json!("2 plugins available"), "{line}");
    assert!(d.has("bashcut.plugins.test.slow.install"), "the list shows without a click");
    d.click("bashcut.rail.runs");
    d.click("bashcut.rail.plugins");
    assert_eq!(jobs(&mut d, "registry").len(), 1, "no second check");

    // a double click on Install starts one install
    d.ok("ui.click", json!({"id": "bashcut.plugins.test.slow.install"}));
    d.frames(1);
    let _ = d.call("ui.click", json!({"id": "bashcut.plugins.test.slow.install"}));
    let install = d.wait_element("bashcut.plugins.test.slow.install", |e| e["label"] == json!("Installing…"));
    assert_eq!((install["label"].clone(), install["enabled"].clone()), (json!("Installing…"), json!(false)), "{install}");
    assert_eq!(d.element("bashcut.plugins.test.other.install")["enabled"], json!(true), "only that plugin waits");
    assert_eq!(jobs(&mut d, "install").len(), 1, "one install");
    if let Some(dir) = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        for lang in ["en", "vi"] {
            d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
            d.frames(3);
            d.harness.render().unwrap().save(dir.join(format!("plugins-installing-{lang}.png"))).unwrap();
        }
        d.exec("bashcut.ui.setLanguage", json!({"language": "en"}));
    }

    // a failed run says why in its card, and Plugins stays open
    if python {
        d.click("bashcut.plugins.test.card");
        d.click("bashcut.plugins.test.card.action.fail");
        let last = d.wait_element("bashcut.plugins.test.card.last", |e| e["label"].as_str().is_some_and(|l| l.starts_with("Failed")));
        assert_eq!(last["label"], json!("Failed: bad_params: no captions in the sequence"), "{last}");
        assert_eq!(d.state()["open"], json!("plugins"), "no switch to Runs");
        assert!(!d.has("bashcut.plugins.status"), "no panel-top status");
        if let Some(dir) = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from) {
            for lang in ["en", "vi"] {
                d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
                d.frames(3);
                d.harness.render().unwrap().save(dir.join(format!("plugins-card-failed-{lang}.png"))).unwrap();
            }
            d.exec("bashcut.ui.setLanguage", json!({"language": "en"}));
        }
        // closed, the card still says it; Open in Runs switches there; ✕ hides it
        d.click("bashcut.plugins.test.card");
        // past the closing animation
        d.frames(30);
        assert!(d.has("bashcut.plugins.test.card.last"), "shown under a closed card");
        d.click("bashcut.plugins.test.card.last.runs");
        assert_eq!(d.state()["open"], json!("runs"));
        d.click("bashcut.rail.plugins");
        d.click("bashcut.plugins.test.card.last.dismiss");
        assert!(!d.has("bashcut.plugins.test.card.last"), "dismissed");
    } else {
        eprintln!("python not found: the failed-run part is skipped");
    }
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    drop(silent);
}

/// The test plugins' Python runs (`py -3` on Windows, where `python3` may be the Store's stub).
fn has_python() -> bool {
    let (exe, args): (&str, &[&str]) = if cfg!(windows) { ("py", &["-3", "--version"]) } else { ("python3", &["--version"]) };
    std::process::Command::new(exe).args(args).output().is_ok_and(|o| o.status.success())
}

/// A stand-in for `bashcut.resources`: one search result, whose download is `tone.wav` beside it.
const RESOURCES_PLUGIN: &str = r#"
import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
if sys.argv[1] == "search":
    items = [{"title": "Calm tone", "kind": "music", "url": "https://example.invalid/tone.wav", "license": "CC0", "author": "test", "source": "test"}]
    print(json.dumps({"ok": True, "data": {"items": items}}))
else:
    print(json.dumps({"ok": True, "data": {"path": os.path.join(here, "tone.wav")}}))
"#;

/// One second of 16 kHz mono silence.
fn silent_wav() -> Vec<u8> {
    let data = 16_000u32 * 2;
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&16_000u32.to_le_bytes());
    w.extend_from_slice(&32_000u32.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    w.resize(w.len() + data as usize, 0);
    w
}

#[test]
fn library_added_status_clears_after_a_few_seconds() {
    if !has_python() {
        eprintln!("python not found: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-library-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("bashcut.resources");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.py"), RESOURCES_PLUGIN).unwrap();
    std::fs::write(plugin.join("tone.wav"), silent_wav()).unwrap();
    let string = |id: &str, flag: &str| json!({"id": id, "flag": flag, "type": "string", "optional": true});
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "bashcut.resources", "name": "Online Resources", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]}, "output": "bashcut-result/1",
        "actions": [
            {"id": "search", "title": "Search", "args": ["search"], "needs": "none", "params": [string("kind", "--kind"), string("query", "--query"),
                {"id": "count", "flag": "--count", "type": "integer", "optional": true}]},
            {"id": "download", "title": "Download", "args": ["download"], "needs": "none", "params": [string("kind", "--kind"), string("url", "--url"),
                string("title", "--title"), string("license", "--license"), string("author", "--author"), string("source", "--source")]}
        ]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();

    let mut d = Driver::with_data(&data, |_| {});

    d.exec("bashcut.ui.library.search", json!({"tab": "online", "kind": "music", "query": "calm"}));
    let results = d.wait(|d| d.exec("bashcut.ui.library.results", json!({})), |r| r["items"].as_array().is_some_and(|a| !a.is_empty()));
    assert_eq!(results["items"][0]["title"], json!("Calm tone"), "{results}");
    d.exec("bashcut.ui.library.add", json!({"index": 0}));
    let status = d.wait(|d| d.exec("bashcut.ui.library.results", json!({}))["status"].clone(), |s| s.as_str().is_some_and(|s| s.starts_with("Added")));
    assert_eq!(status, json!("Added Calm tone to the project"));
    // the harness steps a quarter second a frame: still shown after 1 s, gone after 7.5 s
    d.frames(4);
    assert_eq!(d.exec("bashcut.ui.library.results", json!({}))["status"], status);
    assert!(d.has("bashcut.library.status"));
    d.frames(30);
    assert_eq!(d.exec("bashcut.ui.library.results", json!({}))["status"], Value::Null, "the done message clears (#168)");
    assert!(!d.has("bashcut.library.status"));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn popped_out_windows_come_back_where_they_were() {
    let mut d = Driver::demo();
    d.click("bashcut.rail.runs");
    d.click("bashcut.panel.popout");
    let r = d.rect("bashcut.window.runs").expect("floating window");
    let st = d.state();
    let windows = st["windows"].as_object().cloned().unwrap_or_default();
    let (key, place) = windows.iter().find(|(k, _)| k.starts_with("runs@")).expect("the window's place is remembered");
    assert!(key.contains('x'), "keyed by monitor: {key}");
    assert!(place[0].as_f64().is_some_and(|x| (x as f32 - r[0]).abs() <= 1.0), "{place} vs {r:?}");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
}

/// UX10: a plugin step opens as its action's form (labels in the UI language, no JSON) and Apply
/// rebuilds it through `setStep`; an engine step shows its signature over the JSON, and Apply
/// stays disabled while that JSON does not parse.
#[test]
fn batch_plugin_steps_edit_as_a_form_and_bad_json_cannot_apply() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-stepform-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.cap");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.cap", "name": "Captions", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"]}, "output": "bashcut-result/1",
        "connect": {"project": ["--project", "{project}"]},
        "actions": [{"id": "translate", "title": "Translate captions…", "args": ["translate"], "needs": "project", "params": [
            {"id": "target", "title": "Target language", "flag": "--target", "type": "enum", "default": "vi", "choices": ["vi", "en"],
             "i18n": {"vi": {"title": "Ngôn ngữ đích"}}},
            {"id": "note", "flag": "--note", "type": "string", "optional": true},
            {"id": "burn", "flag": "--burn", "type": "bool", "optional": true}]}]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    let mut d = Driver::with_data(&data, |_| {});
    d.click("bashcut.rail.batch");
    d.exec("bashcut.batch.addStep", json!({"plugin": "test.cap", "action": "translate", "params": {"target": "en", "burn": true}}));
    d.exec("bashcut.batch.addStep", json!({"command": "markers.add", "params": {"name": "Hook"}}));
    let label = d.exec("bashcut.batch.state", json!({}))["draft"]["steps"][0]["label"].clone();

    // the plugin step: a form filled from its command line, no JSON
    d.click("bashcut.batch.step.0");
    let param = "bashcut.batch.step.0.param";
    assert!(!d.has("bashcut.batch.step.0.json"), "no JSON for a plugin step");
    assert_eq!(d.element(&format!("{param}.target"))["label"], json!("Target language"));
    assert!(d.has(&format!("{param}.burn")) && d.has(&format!("{param}.note")));
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    d.frames(3);
    assert_eq!(d.element(&format!("{param}.target"))["label"], json!("Ngôn ngữ đích"), "the label in the UI language");
    if let Some(dir) = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        for lang in ["en", "vi"] {
            d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
            d.frames(3);
            d.harness.render().unwrap().save(dir.join(format!("batch-step-form-{lang}.png"))).unwrap();
        }
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    d.click(&format!("{param}.note"));
    d.ok("ui.type", json!({"text": "hi"}));
    d.frames(2);
    d.click("bashcut.batch.step.0.apply");
    let steps = d.exec("bashcut.batch.state", json!({}))["draft"]["steps"].clone();
    let argv: Vec<String> = serde_json::from_value(steps[0]["plugin"]["argv"].clone()).unwrap();
    let tail = argv.iter().skip_while(|a| *a != "--target").cloned().collect::<Vec<_>>();
    assert_eq!(tail, ["--target", "en", "--note", "hi", "--burn"], "{argv:?}");
    assert_eq!(steps[0]["label"], label, "the label is kept");
    assert!(!d.has(&format!("{param}.target")), "Apply closes the form");

    // the engine step: its signature over the JSON; JSON that doesn't parse can't be applied
    d.click("bashcut.batch.step.1");
    assert!(d.element("bashcut.batch.step.1.signature")["label"].as_str().is_some_and(|l| l.contains("name")));
    assert_eq!(d.element("bashcut.batch.step.1.apply")["enabled"], json!(true));
    d.click("bashcut.batch.step.1.json");
    d.ok("ui.type", json!({"text": "}"}));
    d.frames(3);
    assert_eq!(d.element("bashcut.batch.step.1.apply")["enabled"], json!(false), "invalid JSON");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// BT1: a plugin step with an empty required param says so under its row and Run batch waits
/// (the command refuses too); filled in, it runs. A `false` is kept; a missing plugin greys its step.
#[test]
fn batch_steps_with_empty_required_params_block_the_run() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-bt1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.ask");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.ask", "name": "Ask", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"]}, "output": "bashcut-result/1",
        "connect": {"project": ["--project", "{project}"]},
        "actions": [{"id": "go", "title": "Go", "args": ["go"], "needs": "project", "params": [
            {"id": "answer", "title": "Answer", "flag": "--answer", "type": "string"},
            {"id": "burn", "flag": "--burn", "type": "bool", "default": true}]}]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    let input = data.join("a.mp4");
    std::fs::write(&input, b"x").unwrap();
    let mut d = Driver::with_data(&data, |_| {});
    d.click("bashcut.rail.batch");
    d.exec("bashcut.batch.addInput", json!({"paths": [input]}));
    d.exec("bashcut.batch.addStep", json!({"plugin": "test.ask", "action": "go", "params": {"burn": false}}));
    let step = d.exec("bashcut.batch.state", json!({}))["draft"]["steps"][0].clone();
    assert_eq!(step["pluginAction"], json!({"plugin": "test.ask", "action": "go", "params": {"burn": false}, "missing": ["answer"]}));
    d.frames(2);
    assert_eq!(d.element("bashcut.batch.step.0.error")["label"], json!("Fill in: Answer"));
    assert_eq!(d.element("bashcut.batch.run")["enabled"], json!(false), "Run batch waits");
    assert_eq!(d.element("bashcut.batch.run.blocked")["label"], json!("Step 1: fill in Answer"));
    let v = d.call("engine.execute", json!({"command": "bashcut.batch.run", "params": {"cli": "nope"}}));
    assert!(v.to_string().contains("fill in answer"), "the command refuses too: {v}");
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    d.frames(3);
    assert_eq!(d.element("bashcut.batch.run.blocked")["label"], json!("Bước 1: cần điền Answer"));
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));

    // the form reads back the `false` (the command line can't show it) and fills the blank
    d.click("bashcut.batch.step.0");
    d.click("bashcut.batch.step.0.param.answer");
    d.ok("ui.type", json!({"text": "42"}));
    d.frames(2);
    d.click("bashcut.batch.step.0.apply");
    let step = d.exec("bashcut.batch.state", json!({}))["draft"]["steps"][0].clone();
    assert_eq!(step["pluginAction"]["params"], json!({"answer": "42", "burn": false}), "{step}");
    assert!(step["pluginAction"].get("missing").is_none());
    d.frames(2);
    assert!(!d.has("bashcut.batch.step.0.error"));
    assert_eq!(d.element("bashcut.batch.run")["enabled"], json!(true));

    // a step whose plugin is not installed: greyed, with Install
    d.exec("bashcut.batch.addStep", json!({"step": {"plugin": {"argv": ["x"]}, "pluginAction": {"plugin": "gone.plugin", "action": "go"}}}));
    d.frames(2);
    assert_eq!(d.element("bashcut.batch.step.1")["enabled"], json!(false));
    assert!(d.has("bashcut.batch.step.1.notInstalled") && d.has("bashcut.batch.step.1.install"));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// UX12: a saved preset loads from the "Start from" row at the top of Batch; recent rows read
/// "name · date · n done / m failed"; "Clear all" empties only the inputs.
#[test]
fn batch_starts_from_a_preset_and_clears_inputs() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-startfrom-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(data.join("bashcut")).unwrap();
    let clips: Vec<String> = ["a", "b"]
        .iter()
        .map(|c| {
            let p = data.join(format!("{c}.mp4"));
            std::fs::write(&p, b"media").unwrap();
            p.to_string_lossy().into_owned()
        })
        .collect();
    // 2026-10-22 UTC
    let recent = json!({"recent": [{"job": data.join("no-job").to_string_lossy(), "name": "promo", "at": 1_792_627_200_000u64, "done": 3, "failed": 1}]});
    std::fs::write(data.join("bashcut").join("batch.json"), recent.to_string()).unwrap();
    let mut d = Driver::with_data(&data, |_| {});
    d.click("bashcut.rail.batch");
    d.exec("bashcut.batch.addStep", json!({"command": "markers.add", "params": {"name": "Hook"}}));
    d.exec("bashcut.batch.set", json!({"name": "hooks"}));
    d.exec("bashcut.batch.addInput", json!({"paths": [clips[0]]}));
    d.exec("bashcut.batch.savePreset", json!({"name": "hooks"}));
    d.exec("bashcut.batch.clear", json!({}));
    d.exec("bashcut.batch.addInput", json!({"paths": clips}));
    d.frames(3);

    // the row sits above the inputs
    let (row, input) = (d.rect("bashcut.batch.startFrom").expect("start-from row"), d.rect("bashcut.batch.input.0").expect("input row"));
    assert!(row[1] < input[1], "{row:?} / {input:?}");
    d.click("bashcut.batch.startFrom");
    let r = d.element("bashcut.batch.startFrom.recent.0");
    assert_eq!(r["label"], json!("promo · 2026-10-22 · 3 done / 1 failed"));
    d.click("bashcut.batch.startFrom.preset.hooks");
    let draft = d.exec("bashcut.batch.state", json!({}))["draft"].clone();
    assert_eq!(draft["name"], json!("hooks"), "the preset is loaded");
    assert_eq!(draft["steps"].as_array().map(Vec::len), Some(1));

    // the command: a preset fills the draft
    d.exec("bashcut.batch.clear", json!({}));
    d.exec("bashcut.batch.startFrom", json!({"preset": "hooks"}));
    d.exec("bashcut.batch.addInput", json!({"paths": clips}));
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    d.frames(3);
    assert_eq!(d.element("bashcut.batch.clearInputs")["label"], json!("Xoá hết"));
    d.click("bashcut.batch.startFrom");
    assert_eq!(d.element("bashcut.batch.startFrom.recent.0")["label"], json!("promo · 2026-10-22 · 3 xong / 1 lỗi"));
    d.harness.key_press(egui::Key::Escape);
    d.frames(2);
    if let Some(dir) = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        for lang in ["en", "vi"] {
            d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
            d.frames(3);
            d.harness.render().unwrap().save(dir.join(format!("batch-start-from-{lang}.png"))).unwrap();
        }
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    d.frames(2);
    // a chosen projects folder resets to the default with its ✕
    assert!(d.has("bashcut.batch.projects.choose"), "Choose… stays on screen next to a long folder");
    d.click("bashcut.batch.projects.reset");
    assert_eq!(d.exec("bashcut.batch.state", json!({}))["draft"]["projectsFolder"], json!(""));
    assert!(!d.has("bashcut.batch.projects.reset"), "no ✕ for the default");
    d.click("bashcut.batch.clearInputs");
    let draft = d.exec("bashcut.batch.state", json!({}))["draft"].clone();
    assert_eq!(draft["inputs"], json!([]), "inputs cleared");
    assert_eq!(draft["steps"].as_array().map(Vec::len), Some(1), "steps kept");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_failed_run_puts_a_dot_on_runs_until_runs_opens() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-rail-dot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.fails");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.fails", "name": "Fails", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["sh", "-c", "sleep 4; exit 4"], "windows": ["powershell", "-NoProfile", "-Command", "Start-Sleep -Milliseconds 4000; exit 4"]}, "output": "bashcut-result/1",
        "actions": [{"id": "go", "title": "Go", "args": [], "needs": "none"}]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();

    let mut d = Driver::with_data(&data, |_| {});
    // the rail's label for Runs once it reads `want`, else the last one
    let label_of = |d: &mut Driver, want: &str| {
        let v = d.wait_element("bashcut.rail.runs", |e| e["label"] == json!(want));
        v["label"].as_str().unwrap_or_default().to_string()
    };
    assert_eq!(label_of(&mut d, "Runs"), "Runs");

    // the registry check (of the local file) ends; see it in Runs first
    d.click("bashcut.rail.plugins");
    d.wait(|d| d.exec("bashcut.plugins.runs", json!({})), |r| !r.as_array().is_some_and(|r| r.iter().any(|j| j["state"] == json!("running"))));
    d.click("bashcut.rail.runs");
    d.click("bashcut.rail.plugins");
    assert_eq!(label_of(&mut d, "Runs"), "Runs");

    let job = d.exec("bashcut.plugins.run", json!({"plugin": "test.fails", "action": "go"}))["job"].as_u64().expect("a job");
    d.exec("bashcut.ui.show.plugins", json!({}));
    assert_eq!(label_of(&mut d, "Runs: running"), "Runs: running");
    assert_eq!(d.wait_job(job)["state"], json!("failed"));
    assert_eq!(label_of(&mut d, "Runs: failed"), "Runs: failed", "the failure shows on the rail while Plugins is open");
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    assert_eq!(label_of(&mut d, "Tác vụ: lỗi"), "Tác vụ: lỗi");
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));

    d.click("bashcut.rail.runs");
    d.click("bashcut.rail.plugins");
    assert_eq!(label_of(&mut d, "Runs"), "Runs", "seen once Runs was open");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// RN1: a run that ends while Runs is not on screen toasts once ("✓ … finished" / "✕ … failed", en
/// and vi), Show opens Runs on that run, and with Runs open nothing toasts. Runs' status line has a
/// ✕ and comes back only when its message changes.
#[test]
fn a_finished_run_toasts_once_and_status_lines_dismiss() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-toast-{}", std::process::id()));
    if !has_python() {
        eprintln!("python not found: skipped");
        return;
    }
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.toast");
    std::fs::create_dir_all(&plugin).unwrap();
    let script = "import json, sys
if sys.argv[1:] == ['bad']:
    sys.exit(4)
print(json.dumps({'ok': True, 'data': {'path': '/out/toast.wav'}}))
";
    std::fs::write(plugin.join("plugin.py"), script).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.toast", "name": "Toast", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]},
        "output": "bashcut-result/1",
        "actions": [
            {"id": "good", "title": "Good", "args": ["good"], "needs": "none"},
            {"id": "bad", "title": "Bad", "args": ["bad"], "needs": "none"}
        ]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();

    let reveals = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = reveals.clone();
    let mut d = Driver::with_data(&data, move |app| {
        app.hooks.open_path = Some(Box::new(move |_, _| Err(format!("cannot reveal ({})", counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1))));
    });
    let run = |d: &mut Driver, action: &str| {
        let job = d.exec("bashcut.plugins.run", json!({"plugin": "test.toast", "action": action}))["job"].as_u64().expect("a job");
        d.wait_job(job);
        d.frames(3);
        job
    };
    // the toasts on screen as (id, label), once there are `want` of them (else the last ones)
    fn listed(v: &Value) -> Vec<(String, String)> {
        let toast = |e: &Value| {
            e["id"].as_str().filter(|id| id.matches('.').count() == 2).map(|id| (id.to_string(), e["label"].as_str().unwrap_or_default().to_string()))
        };
        v.as_array().map(|a| a.iter().filter_map(toast).collect()).unwrap_or_default()
    }
    let toasts = |d: &mut Driver, want: usize| {
        let v = d.wait(|d| d.ok("ui.elements", json!({"prefix": "bashcut.toast."})), |v| listed(v).len() == want);
        listed(&v)
    };

    let good = run(&mut d, "good");
    let j = d.exec("bashcut.plugins.job", json!({"job": good}));
    assert_eq!(j["state"], json!("succeeded"), "{j}");
    let shown = toasts(&mut d, 1);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0].0, format!("bashcut.toast.{good}"));
    assert!(shown[0].1.starts_with('✓') && shown[0].1.ends_with("finished"), "{shown:?}");
    d.frames(5);
    assert_eq!(toasts(&mut d, 1).len(), 1, "once per run");
    d.click(&format!("bashcut.toast.{good}.dismiss"));
    assert!(toasts(&mut d, 0).is_empty());
    d.frames(5);
    assert!(toasts(&mut d, 0).is_empty(), "a dismissed toast stays gone");

    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    let bad = run(&mut d, "bad");
    let shown = toasts(&mut d, 1);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(shown[0].1.starts_with('✕') && shown[0].1.ends_with("bị lỗi"), "{shown:?}");
    assert!(d.has(&format!("bashcut.toast.{bad}.show")));
    d.click(&format!("bashcut.toast.{bad}.show"));
    d.frames(2);
    assert_eq!(d.state()["open"], json!("runs"), "Show opens Runs");
    assert!(d.has(&format!("bashcut.runs.{bad}")), "on that run");
    assert!(toasts(&mut d, 0).is_empty());
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));

    // with Runs open, a finished run does not toast
    let seen = run(&mut d, "good");
    d.frames(3);
    assert!(toasts(&mut d, 0).is_empty());

    // Runs' status line: ✕ hides it, a new message brings it back
    let status = |d: &mut Driver| {
        d.ok("ui.elements", json!({"prefix": "bashcut.runs.status"}))
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == json!("bashcut.runs.status"))
            .map(|e| e["label"].as_str().unwrap_or_default().to_string())
    };
    d.click(&format!("bashcut.runs.{seen}.reveal.0"));
    assert_eq!(status(&mut d).as_deref(), Some("cannot reveal (1)"));
    d.click("bashcut.runs.status.dismiss");
    assert_eq!(status(&mut d), None);
    d.frames(5);
    assert_eq!(status(&mut d), None, "stays dismissed while the message is the same");
    d.click(&format!("bashcut.runs.{seen}.reveal.0"));
    assert_eq!(status(&mut d).as_deref(), Some("cannot reveal (2)"), "a new message comes back");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// RN2: Batch's status line gets the same ✕ as Plugins/Runs (Agent shares the helper): dismissed it
/// stays hidden, and comes back once the status clears and a new one arrives.
#[test]
fn batch_status_line_dismisses() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-rn2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(data.join("bashcut")).unwrap();
    let recent = json!({"recent": [{"job": data.join("no-job").to_string_lossy(), "name": "gone", "at": 1_792_627_200_000u64, "done": 0, "failed": 0}]});
    std::fs::write(data.join("bashcut").join("batch.json"), recent.to_string()).unwrap();
    let mut session = Session::default();
    session.start_autosave(filmcraft_engine::autosave::AutosaveConfig::new(&data)).expect("data dir");
    session.execute("file.openDemoProject", json!({})).expect("demo project");
    let (tx, rx) = channel();
    let app = FilmcraftApp::new(session).with_control(rx);
    let harness = Harness::builder().with_size(egui::vec2(W, H)).with_max_steps(10_000).build_eframe(move |_cc| app);
    let mut d = Driver { harness, tx };
    d.frames(4);
    d.click("bashcut.rail.batch");
    d.frames(2);
    let status = |d: &mut Driver| {
        d.ok("ui.elements", json!({"prefix": "bashcut.batch.status"}))
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == json!("bashcut.batch.status"))
            .map(|e| e["label"].as_str().unwrap_or_default().to_string())
    };
    assert_eq!(status(&mut d), None);
    d.click("bashcut.batch.recent.0");
    let msg = status(&mut d).expect("opening a missing job shows a status");
    assert!(d.has("bashcut.batch.status.dismiss"));
    d.click("bashcut.batch.status.dismiss");
    assert_eq!(status(&mut d), None);
    d.frames(5);
    assert_eq!(status(&mut d), None, "stays dismissed while the message is the same");
    d.click("bashcut.batch.recent.0");
    assert_eq!(status(&mut d), None, "the same message again stays dismissed");
    d.click("bashcut.batch.onFailure.skip");
    d.click("bashcut.batch.recent.0");
    assert_eq!(status(&mut d), Some(msg), "after the status clears, it comes back");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// CC1: the Agent section's "Allow AI agents" checkbox writes `<data>/bashcut/agents-off` and removes
/// it again; Plugins' "Turn on" shows only when this window started with the channel off by that
/// setting (`off_reason() == Setting`). The launch decision is process-wide, so that half runs in
/// a child test process that makes it.
#[test]
fn agent_setting_writes_the_marker_and_turn_on_shows_only_when_the_setting_turned_it_off() {
    use filmcraft_ui_egui::bashcut_ui::control;
    const CHILD: &str = "BASHCUT_UI_TEST_AGENTS_OFF";
    if std::env::var_os("FILMCRAFT_CONTROL_PORT").is_some() {
        eprintln!("FILMCRAFT_CONTROL_PORT is set: skipped");
        return;
    }
    let Some(data) = std::env::var_os(CHILD).map(std::path::PathBuf::from) else {
        let data = std::env::temp_dir().join(format!("bashcut-ui-agents-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let marker = control::marker(&data);
        let mut d = Driver::with_data(&data, |_| {});
        d.click("bashcut.rail.agent");
        assert_eq!(d.element("bashcut.agent.allowControl")["label"], json!(control::ALLOW));
        assert!(!marker.exists(), "on by default");
        d.click("bashcut.agent.allowControl");
        assert!(marker.exists(), "unticked writes the marker");
        // this window was not started by the launch hook: no reason to offer Turn on
        assert_eq!(control::off_reason(), control::Off::No);
        d.click("bashcut.rail.plugins");
        assert!(d.has("bashcut.plugins.setup"));
        assert!(!d.has("bashcut.plugins.turnOn"));
        d.click("bashcut.rail.agent");
        d.click("bashcut.agent.allowControl");
        assert!(!marker.exists(), "ticked removes it");
        assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
        drop(d);

        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(exe)
            .args(["--exact", "agent_setting_writes_the_marker_and_turn_on_shows_only_when_the_setting_turned_it_off", "--nocapture", "--test-threads=1"])
            .env(CHILD, &data)
            .env_remove(control::ENV)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&data);
        let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && log.contains("1 passed"), "{log}");
        return;
    };
    // the child: launched with the setting off
    control::set_allowed(&data, false).unwrap();
    assert_eq!(control::launch_port(None, Some(&data)), None);
    assert_eq!(control::off_reason(), control::Off::Setting);
    let mut d = Driver::with_data(&data, |_| {});
    d.click("bashcut.rail.plugins");
    assert_eq!(d.element("bashcut.plugins.turnOn")["label"], json!("Turn on"));
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    assert_eq!(d.element("bashcut.plugins.turnOn")["label"], json!("Bật"));
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    d.click("bashcut.plugins.turnOn");
    assert!(!control::marker(&data).exists(), "Turn on removes the marker");
    d.frames(2);
    assert!(!d.has("bashcut.plugins.turnOn"), "then it says restart instead");
    assert!(d.harness.query_by_label("Restart BashCutPro to apply.").is_some());
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
}

/// #281 moved every dim / danger line to `widgets::note`: Plugins, Batch and Agent still draw
/// theirs, in English and Vietnamese.
#[test]
fn notes_still_render_in_plugins_batch_and_agent() {
    if std::env::var_os("FILMCRAFT_CONTROL_PORT").is_some() {
        eprintln!("FILMCRAFT_CONTROL_PORT is set: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-notes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let mut d = Driver::with_data(&data, |_| {});
    // (language, Plugins, Batch, Agent) lines with no automation id of their own
    let rows = [
        (
            "en",
            ["No plugins installed yet."],
            [
                "Drop files or folders here, or add them below.",
                "Each input gets its own project; then these run on it in order.",
                "Add files to run the batch.",
            ],
            "Control channel off: the agent cannot edit the open project.",
        ),
        (
            "vi",
            ["Chưa cài plugin nào."],
            [
                "Thả tệp hoặc thư mục vào đây, hoặc thêm ở bên dưới.",
                "Mỗi đầu vào có dự án riêng; sau đó các bước này chạy lần lượt trên nó.",
                "Thêm tệp để chạy batch.",
            ],
            "Kênh điều khiển đang tắt: trợ lý không sửa được dự án đang mở.",
        ),
    ];
    for (lang, plugins, batch, agent) in rows {
        d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
        d.exec("bashcut.ui.show.plugins", json!({}));
        d.frames(2);
        let setup = d.element("bashcut.plugins.setup");
        assert!(setup["label"].as_str().is_some_and(|l| !l.is_empty()), "{lang}: {setup}");
        for line in plugins {
            assert!(d.harness.query_by_label(line).is_some(), "{lang}: Plugins shows {line}");
        }
        d.exec("bashcut.ui.show.batch", json!({}));
        d.frames(2);
        for line in batch {
            assert!(d.harness.query_by_label(line).is_some(), "{lang}: Batch shows {line}");
        }
        d.exec("bashcut.ui.show.agent", json!({}));
        d.frames(2);
        assert_eq!(d.element("bashcut.agent.ready.control")["label"], json!(agent), "{lang}");
        assert!(d.harness.query_by_label(agent).is_some(), "{lang}: drawn, not only registered");
        if let Some(dir) = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from) {
            std::fs::create_dir_all(&dir).unwrap();
            for sec in ["plugins", "batch", "agent"] {
                d.exec(&format!("bashcut.ui.show.{sec}"), json!({}));
                d.frames(3);
                d.harness.render().unwrap().save(dir.join(format!("notes-{sec}-{lang}.png"))).unwrap();
            }
        }
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// UX9: a section's dot is hidden while it is on screen, docked or popped out, and comes back once
/// it is closed.
#[test]
fn rail_dot_hides_while_its_section_is_docked_or_popped_out() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-dot-hidden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.slow");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.slow", "name": "Slow", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["sh", "-c", "sleep 6; exit 4"], "windows": ["powershell", "-NoProfile", "-Command", "Start-Sleep -Milliseconds 6000; exit 4"]}, "output": "bashcut-result/1",
        "actions": [{"id": "go", "title": "Go", "args": [], "needs": "none"}]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    let mut d = Driver::with_data(&data, |_| {});
    let label_of = |d: &mut Driver, want: &str| {
        let v = d.wait_element("bashcut.rail.runs", |e| e["label"] == json!(want));
        v["label"].as_str().unwrap_or_default().to_string()
    };
    // the first-open registry check of Plugins ends first
    d.click("bashcut.rail.plugins");
    d.wait(|d| d.exec("bashcut.plugins.runs", json!({})), |r| !r.as_array().is_some_and(|r| r.iter().any(|j| j["state"] == json!("running"))));
    d.exec("bashcut.ui.closePanel", json!({}));

    let job = d.exec("bashcut.plugins.run", json!({"plugin": "test.slow", "action": "go"}))["job"].as_u64().expect("a job");
    assert_eq!(label_of(&mut d, "Runs: running"), "Runs: running");
    d.exec("bashcut.ui.show.runs", json!({}));
    assert_eq!(label_of(&mut d, "Runs"), "Runs", "no dot while docked");
    d.exec("bashcut.ui.popOut", json!({"section": "runs"}));
    assert_eq!(d.state()["popped"], json!(["runs"]));
    assert_eq!(label_of(&mut d, "Runs"), "Runs", "no dot while popped out");
    d.exec("bashcut.ui.dockBack", json!({"section": "runs"}));
    d.exec("bashcut.ui.closePanel", json!({}));
    assert_eq!(d.exec("bashcut.plugins.job", json!({"job": job}))["state"], json!("running"), "still running: the test ran too slow");
    assert_eq!(label_of(&mut d, "Runs: running"), "Runs: running", "back once closed");

    assert_eq!(d.wait_job(job)["state"], json!("failed"));
    assert_eq!(label_of(&mut d, "Runs: failed"), "Runs: failed");
    d.exec("bashcut.ui.popOut", json!({"section": "runs"}));
    assert_eq!(label_of(&mut d, "Runs"), "Runs", "popped out shows the failure: no dot");
    d.exec("bashcut.ui.dockBack", json!({"section": "runs"}));
    d.exec("bashcut.ui.closePanel", json!({}));
    assert_eq!(label_of(&mut d, "Runs"), "Runs", "seen while popped out");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// The Agent rail button's tooltip names Focus Agent's keys (Cmd+Alt+Shift+A, shown the platform's
/// way), in English and Vietnamese.
#[test]
fn agent_tooltip_names_its_shortcut() {
    let mut d = Driver::demo();
    let keys = filmcraft_ui_egui::menus::shortcut_text("Cmd+Alt+Shift+A");
    let r = d.rect("bashcut.rail.agent").expect("rail button");
    let center = egui::pos2(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    for (lang, tip) in [("en", format!("Shortcut: {keys}")), ("vi", format!("Phím tắt: {keys}"))] {
        d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
        // a tooltip waits for the pointer to rest: move once, then let frames pass
        d.harness.input_mut().events.push(egui::Event::PointerMoved(center));
        let mut found = false;
        for _ in 0..40 {
            d.frames(1);
            if d.harness.query_by_label(&tip).is_some() {
                found = true;
                break;
            }
        }
        assert!(found, "{lang}: the tooltip says {tip}");
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
}

/// A key store whose writes wait until the test opens the gate, so the busy row can be seen.
#[derive(Default)]
struct GatedStore {
    inner: MemoryStore,
    shut: std::sync::Mutex<bool>,
    gate: std::sync::Condvar,
}

impl GatedStore {
    fn hold(&self, shut: bool) {
        *self.shut.lock().unwrap() = shut;
        self.gate.notify_all();
    }

    fn wait_open(&self) {
        let shut = self.shut.lock().unwrap();
        let _open = self.gate.wait_timeout_while(shut, Duration::from_secs(60), |s| *s).unwrap();
    }
}

impl SecretStore for GatedStore {
    fn get(&self, plugin: &str, key: &str) -> bashcut_plugin_host::Result<Option<String>> {
        self.inner.get(plugin, key)
    }
    fn set(&self, plugin: &str, key: &str, value: &str) -> bashcut_plugin_host::Result<()> {
        self.wait_open();
        self.inner.set(plugin, key, value)
    }
    fn delete(&self, plugin: &str, key: &str) -> bashcut_plugin_host::Result<()> {
        self.wait_open();
        self.inner.delete(plugin, key)
    }
}

/// RC13: a secret's Save shows the busy row ("Saving…", field and buttons gone) until the
/// side-thread write lands, then "stored" (Clear appears, the field empties); Clear the same way
/// with "Clearing…". The key store is a fake whose writes wait on the test; both languages.
#[test]
fn a_secret_save_shows_busy_then_stored() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-secret-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.secret");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.secret", "name": "Secret", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"]}, "output": "bashcut-result/1",
        "settings": [{"id": "apiKey", "type": "secret", "title": "API key", "env": ["TEST_SECRET_KEY"]}],
        "actions": []
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    let store = std::sync::Arc::new(GatedStore::default());
    let mut d = Driver::with_key_store(&data, store.clone());

    d.click("bashcut.rail.plugins");
    d.click("bashcut.plugins.test.secret");
    let field = "bashcut.plugins.test.secret.secret.apiKey";
    for (lang, saving, clearing) in [("en", "Saving…", "Clearing…"), ("vi", "Đang lưu…", "Đang xóa…")] {
        d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
        assert_eq!(d.element(field)["enabled"], json!(true), "{lang}: the field takes a secret");
        assert!(!d.has(&format!("{field}.clear")), "{lang}: nothing stored, no Clear");

        // Save: the write waits on the gate, so the row stays busy
        store.hold(true);
        d.click(field);
        d.ok("ui.type", json!({"text": format!("sk-{lang}")}));
        d.frames(2);
        d.click(&format!("{field}.save"));
        d.frames(3);
        assert!(d.harness.query_by_label(saving).is_some(), "{lang}: the row says {saving}");
        assert!(!d.has(&format!("{field}.save")), "{lang}: no Save while saving");
        assert_eq!(d.element(field)["enabled"], json!(false), "{lang}: the field waits");
        assert_eq!(store.inner.get("test.secret", "apiKey").unwrap(), None, "{lang}: not written yet");
        store.hold(false);
        let clear = d.wait_element(&format!("{field}.clear"), |e| !e.is_null());
        assert!(!clear.is_null(), "{lang}: stored, so Clear shows");
        assert!(d.harness.query_by_label(saving).is_none(), "{lang}: no longer busy");
        assert_eq!(store.inner.get("test.secret", "apiKey").unwrap().as_deref(), Some(format!("sk-{lang}").as_str()));
        assert_eq!(d.element(field)["enabled"], json!(true));
        assert_eq!(d.element(&format!("{field}.save"))["enabled"], json!(false), "{lang}: the field emptied, Save waits for text");
        assert_eq!(d.exec("bashcut.plugins.settings", json!({"plugin": "test.secret"}))["secrets"]["apiKey"], json!(true), "{lang}: the host sees it stored");

        // Clear: busy the same way, then not set
        store.hold(true);
        d.click(&format!("{field}.clear"));
        d.frames(3);
        assert!(d.harness.query_by_label(clearing).is_some(), "{lang}: the row says {clearing}");
        assert!(!d.has(&format!("{field}.clear")), "{lang}: no Clear while clearing");
        store.hold(false);
        let gone = d.wait(|d| json!(d.has(&format!("{field}.save"))), |v| *v == json!(true));
        assert_eq!(gone, json!(true), "{lang}: the row is back");
        assert!(!d.has(&format!("{field}.clear")), "{lang}: cleared, no Clear");
        assert_eq!(store.inner.get("test.secret", "apiKey").unwrap(), None, "{lang}: deleted in the store");
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// T8: the bashcut panels as PNGs in en and vi, for a pixel diff of two commits (`bashcut/in-app-ui.md`
/// §8): Agent with a tool group open, Runs with an error and its log, a toast, Batch's step list,
/// the palette's parameters, the rail tooltip, and a section docked and popped out. Ignored and
/// needs `BASHCUT_UI_SHOTS=<dir>`; it only renders, so it runs as-is on an older commit too. The
/// data dir has a fixed name so paths drawn are the same in every run.
#[test]
#[ignore = "pixel-diff pass: BASHCUT_UI_SHOTS=<dir> cargo test -p filmcraft-ui-egui --test bashcut_ui -- --ignored panels_render_for_a_pixel_diff"]
fn panels_render_for_a_pixel_diff() {
    let Some(dir) = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from) else {
        eprintln!("BASHCUT_UI_SHOTS is not set: skipped");
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let data = std::env::temp_dir().join("bashcut-ui-pixel-diff");
    let _ = std::fs::remove_dir_all(&data);
    // a plugin that logs a line and fails
    let plugin = data.join("bashcut-plugins").join("test.pixels");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.pixels", "name": "Pixels", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["sh", "-c", "echo 'reading captions' >&2; echo 'no captions in the sequence' >&2; exit 4"],
                  "windows": ["cmd", "/C", "echo reading captions 1>&2 & echo no captions in the sequence 1>&2 & exit 4"]},
        "output": "bashcut-result/1",
        "actions": [{"id": "go", "title": "Translate captions…", "args": [], "needs": "none", "params": [
            {"id": "target", "title": "Target language", "flag": "--target", "type": "enum", "default": "vi", "choices": ["vi", "en"]}]}]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    // a saved project whose sidecar holds a conversation with tool calls
    let project = data.join("film.fcproj");
    let conv = data.join("film.fcproj.bashcut").join("conversations");
    std::fs::create_dir_all(&conv).unwrap();
    let add = json!({"command": "markers.add", "params": {"name": "Hook"}});
    let items = json!([
        {"type": "user", "text": "Mark the hook and cut the intro"},
        {"type": "assistant", "text": "Adding a **marker** at the hook, then:\n\n- trim the intro\n- `markers.add` twice"},
        {"type": "tool", "id": "t1", "name": "mcp__filmcraft__execute", "input": add, "output": "{\"ok\":true}", "isError": false},
        {"type": "tool", "id": "t2", "name": "mcp__filmcraft__execute", "input": add, "output": "{\"ok\":true}", "isError": false},
        {"type": "tool", "id": "t3", "name": "mcp__filmcraft__execute", "input": {"command": "edit.trim"}, "output": "no clip selected", "isError": true},
        {"type": "gate", "stage": "rough cut", "state": "approved", "note": "keep the intro short"},
        {"type": "error", "message": "The agent stopped: exit code 1."},
        {"type": "notice", "message": "Resumed the earlier session."}
    ]);
    let c = json!({"schema": "bashcut-conversation/1", "id": "c1-0", "title": "Hook", "agent": "claude-code", "created": 1, "updated": 1, "transcript": {"items": items}});
    std::fs::write(conv.join("c1-0.json"), c.to_string()).unwrap();
    std::fs::write(data.join("a.mp4"), b"media").unwrap();

    let mut d = Driver::with_data(&data, |_| {});
    let shot = |d: &mut Driver, name: &str| {
        for lang in ["en", "vi"] {
            d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
            d.frames(4);
            d.harness.render().unwrap().save(dir.join(format!("{name}-{lang}.png"))).unwrap();
        }
        d.exec("bashcut.ui.setLanguage", json!({"language": "en"}));
    };
    // the first-open registry check of Plugins ends first, so no job runs under the shots
    d.click("bashcut.rail.plugins");
    d.wait(|d| d.exec("bashcut.plugins.runs", json!({})), |r| !r.as_array().is_some_and(|r| r.iter().any(|j| j["state"] == json!("running"))));
    d.exec("bashcut.ui.closePanel", json!({}));

    // Agent: the conversation with its first tool group open
    d.harness.state_mut().session.path = Some(project.to_string_lossy().into_owned());
    d.exec("bashcut.ui.show.agent", json!({}));
    d.exec("bashcut.agent.open", json!({"id": "c1-0"}));
    d.frames(3);
    let tools = d.ok("ui.elements", json!({"prefix": "bashcut.agent.tool."}));
    let first = tools.as_array().and_then(|a| a.iter().filter_map(|e| e["id"].as_str()).find(|id| id.matches('.').count() == 3).map(str::to_string));
    if let Some(id) = first {
        d.click(&id);
    }
    shot(&mut d, "agent-tools");
    d.exec("bashcut.ui.closePanel", json!({}));

    // a failed run while Runs is closed: its toast, then Runs with the error and the log open
    let job = d.exec("bashcut.plugins.run", json!({"plugin": "test.pixels", "action": "go"}))["job"].as_u64().expect("a job");
    d.wait_job(job);
    d.wait(|d| d.ok("ui.elements", json!({"prefix": format!("bashcut.toast.{job}")})), |v| v.as_array().is_some_and(|a| !a.is_empty()));
    shot(&mut d, "toast");
    d.exec("bashcut.ui.show.runs", json!({}));
    d.wait_element(&format!("bashcut.runs.{job}.error"), |e| !e.is_null());
    d.click(&format!("bashcut.runs.{job}.log"));
    shot(&mut d, "runs-error-log");

    // the same section popped out, then docked back
    d.exec("bashcut.ui.popOut", json!({"section": "runs"}));
    d.frames(3);
    shot(&mut d, "runs-popped");
    d.exec("bashcut.ui.dockBack", json!({"section": "runs"}));
    d.frames(3);
    shot(&mut d, "runs-docked");
    d.exec("bashcut.ui.closePanel", json!({}));

    // Batch: an input, a plugin step and an engine step (open, with its signature)
    d.exec("bashcut.ui.show.batch", json!({}));
    d.exec("bashcut.batch.addInput", json!({"paths": [data.join("a.mp4")]}));
    d.exec("bashcut.batch.addStep", json!({"plugin": "test.pixels", "action": "go", "params": {"target": "en"}}));
    d.exec("bashcut.batch.addStep", json!({"command": "markers.add", "params": {"name": "Hook"}}));
    d.click("bashcut.batch.step.1");
    shot(&mut d, "batch-steps");
    d.exec("bashcut.ui.closePanel", json!({}));

    // the palette's parameters for an engine command
    d.exec("bashcut.palette.open", json!({"query": "markers.add"}));
    d.frames(2);
    d.harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    d.frames(3);
    shot(&mut d, "palette-params");
    d.exec("bashcut.palette.close", json!({}));

    // the rail tooltip: the pointer rests on Agent's button
    let r = d.rect("bashcut.rail.agent").expect("rail button");
    d.harness.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)));
    d.frames(40);
    shot(&mut d, "rail-tooltip");
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    let _ = std::fs::remove_dir_all(&data);
}
