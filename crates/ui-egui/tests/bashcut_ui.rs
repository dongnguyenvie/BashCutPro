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
fn rail_shows_in_export_mode_and_hides_on_toggle() {
    let mut d = Driver::demo();
    d.exec("bashcut.ui.toggleRail", json!({}));
    assert!(!d.has("bashcut.rail.agent"));
    d.exec("bashcut.ui.toggleRail", json!({}));
    assert!(d.has("bashcut.rail.agent"));
    d.exec("mode.export", json!({}));
    assert!(d.has("bashcut.rail.agent"), "the rail shows in Export mode too (AG13)");
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
    assert!(shown[0].1.ends_with("finished"), "{shown:?}");
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
    assert!(shown[0].1.ends_with("bị lỗi"), "{shown:?}");
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

/// FX10: a batch step built from a plugin action keeps its settings in `env` but no machine paths
/// (`BASHCUTPRO_DATA_DIR`, `BASHCUTPRO_CLI`): the runner sets both from the job's `--data-dir` and `--cli`.
#[test]
fn batch_plugin_steps_save_no_machine_paths() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-fx10-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.env");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.env", "name": "Env", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"]}, "output": "bashcut-result/1",
        "connect": {"project": ["--project", "{project}"]},
        "settings": [{"id": "service", "title": "Service", "type": "enum", "choices": ["a", "b"], "default": "b", "env": ["FAKE_SERVICE_FX10"]}],
        "actions": [{"id": "go", "title": "Go", "args": ["go"], "needs": "project", "params": []}]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    let mut d = Driver::with_data(&data, |_| {});
    d.click("bashcut.rail.batch");
    d.exec("bashcut.batch.addStep", json!({"plugin": "test.env", "action": "go"}));
    let step = d.exec("bashcut.batch.state", json!({}))["draft"]["steps"][0].clone();
    let env = &step["plugin"]["env"];
    assert_eq!(env["FAKE_SERVICE_FX10"], json!("b"), "settings stay: {step}");
    assert!(env.get("BASHCUTPRO_DATA_DIR").is_none(), "no data folder in the saved step: {step}");
    assert!(env.get("BASHCUTPRO_CLI").is_none(), "no engine path in the saved step: {step}");
    let _ = std::fs::remove_dir_all(&data);
}

/// A silent agent for [`a_silent_agent_gets_the_quiet_hint_until_it_speaks`]: says nothing until
/// `speak` appears next to it, then one assistant line; ends once `stop` appears.
const QUIET_AGENT: &str = r#"
import json, os, sys, time
here = os.path.dirname(os.path.abspath(__file__))
spoke = False
for _ in range(1200):
    if os.path.exists(os.path.join(here, "stop")):
        break
    if not spoke and os.path.exists(os.path.join(here, "speak")):
        print(json.dumps({"type": "assistant", "message": {"content": [{"type": "text", "text": "Looking."}]}}), flush=True)
        spoke = True
    time.sleep(0.05)
print(json.dumps({"type": "result", "subtype": "success", "is_error": False, "result": "Done.", "session_id": "s1"}), flush=True)
"#;

/// AG1: a running agent that sends nothing for 30 s gets `bashcut.agent.quiet` (both languages);
/// its next event clears it. The clock jumps ahead instead of waiting (each harness frame is also
/// 0.25 s of egui time, so the checks keep a margin around the 30 s).
#[test]
fn a_silent_agent_gets_the_quiet_hint_until_it_speaks() {
    if !has_python() || cfg!(windows) {
        eprintln!("python3 not found: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-quiet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.quiet");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("agent.py"), QUIET_AGENT).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.quiet", "name": "Quiet", "version": "0.1.0", "summary": "test agent",
        "kind": "agent", "entry": {"command": ["python3", plugin.join("agent.py").to_string_lossy()]}, "output": "bashcut-result/1",
        "agent": {"args": []},
        "actions": [{"id": "noop", "title": "Noop", "args": ["noop"], "needs": "none", "params": []}]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    let mut d = Driver::with_data(&data, |_| {});
    d.click("bashcut.rail.agent");
    let agents = d.exec("bashcut.agent.agents", json!({}));
    assert!(agents.as_array().is_some_and(|a| a.iter().any(|a| a["id"] == json!("test.quiet"))), "{agents}");
    d.exec("bashcut.agent.use", json!({"agent": "test.quiet"}));
    d.exec("bashcut.agent.send", json!({"text": "cut the intro"}));
    assert_eq!(d.exec("bashcut.agent.state", json!({}))["status"]["state"], json!("running"));
    d.frames(5);
    assert!(d.element("bashcut.agent.quiet").is_null(), "no hint before 30 s");

    let jump = |d: &mut Driver, secs: f64| {
        let t = d.harness.ctx.input(|i| i.time);
        d.harness.input_mut().time = Some(t + secs);
        d.frames(3);
    };
    jump(&mut d, 20.0);
    assert!(d.element("bashcut.agent.quiet").is_null(), "20 s is not yet quiet");
    jump(&mut d, 11.0);
    let hint = d.element("bashcut.agent.quiet");
    let en = hint["label"].as_str().unwrap_or_default().to_string();
    assert!(en.starts_with("No output for 30 s"), "{hint}");
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    d.frames(2);
    let vi = d.element("bashcut.agent.quiet");
    assert!(vi["label"].as_str().is_some_and(|l| l.starts_with("Không có phản hồi sau 30 giây")), "{vi}");
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));

    // the agent speaks: the hint goes (over 30 s after the send, so the silence counts from the
    // event), and comes back 30 s after it
    std::fs::write(plugin.join("speak"), b"").unwrap();
    let gone = d.wait_element("bashcut.agent.quiet", Value::is_null);
    assert!(gone.is_null(), "an event clears the hint: {gone}");
    assert_eq!(d.exec("bashcut.agent.state", json!({}))["status"]["state"], json!("running"), "still working");
    jump(&mut d, 31.0);
    assert!(!d.element("bashcut.agent.quiet").is_null(), "quiet again 30 s after the last event");

    std::fs::write(plugin.join("stop"), b"").unwrap();
    let st = d.wait(|d| d.exec("bashcut.agent.state", json!({})), |s| s["status"]["state"] != json!("running"));
    assert_ne!(st["status"]["state"], json!("running"), "{st}");
    assert!(d.element("bashcut.agent.quiet").is_null(), "no hint once the turn ended");
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// CK1 (BR3): Help ▸ BashCutPro Help… reads in the UI language and `help.filmcraftHelp` opens
/// our guide in it: `bashcut/guide/en`, or `bashcut/guide/vi` once our strings are Vietnamese.
#[test]
fn help_menu_names_bashcutpro_and_opens_the_guide_in_both_languages() {
    let mut d = Driver::demo();
    d.harness.state_mut().hooks.open_url = Some(Box::new(|_| Ok(())));
    let help = |d: &mut Driver| {
        let menus = d.ok("ui.menu.list", json!({}));
        menus.as_array().and_then(|m| m.iter().find(|m| m["id"] == json!("help.filmcraftHelp")).cloned()).unwrap_or(Value::Null)
    };
    let en = help(&mut d);
    assert_eq!(en["label"], json!("BashCutPro Help…"), "{en}");
    assert_eq!(en["path"], json!(["Help"]));
    let r = d.ok("ui.menu.invoke", json!({"id": "help.filmcraftHelp"}));
    assert_eq!(r["url"], json!(filmcraft_ui_egui::bashcut_brand::GUIDE_URL));

    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    let vi = help(&mut d);
    assert_eq!(vi["label"], json!("Trợ giúp BashCutPro…"), "{vi}");
    let r = d.ok("ui.menu.invoke", json!({"id": "help.filmcraftHelp"}));
    assert_eq!(r["url"], json!(filmcraft_ui_egui::bashcut_brand::GUIDE_URL_VI));
    assert!(r["url"].as_str().is_some_and(|u| u.ends_with("/bashcut/guide/vi")), "{r}");
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
}

/// FX16: during a long turn each queued message's ✕ lies inside the transcript, clear of its
/// (floating) vertical scroll bar, and a real pointer click on it removes the message; at the
/// narrowest panel too, in English and Vietnamese.
#[test]
fn queued_x_stays_clear_of_the_transcript_scroll_bar() {
    let mode = [("FAKE_MODE", "receipt,cancel-queued")];
    let Some(fake) = fake_claude("queued_x_stays_clear_of_the_transcript_scroll_bar", "fx16", &mode) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.agent.send", json!({"text": "hold first, long"}));
    d.wait(|d| d.exec("bashcut.agent.transcript", json!({})), |t| t["items"].to_string().contains("line 59 of a long answer"));
    for text in ["second", "third", "fourth"] {
        d.exec("bashcut.agent.send", json!({"text": text}));
    }
    let bar = 10.0;
    for (lang, width) in [("en", 340.0), ("vi", 340.0), ("en", 240.0), ("vi", 240.0)] {
        d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
        d.exec("bashcut.ui.setWidth", json!({"width": width}));
        d.frames(3);
        let t = d.rect("bashcut.agent.transcript").expect("transcript");
        for i in 0..3 {
            let x = d.wait(|d| json!(d.rect(&format!("bashcut.agent.queued.{i}.cancel"))), |r| !r.is_null());
            let x: Vec<f32> = x.as_array().expect("✕ shown").iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
            let (left, top, right, bottom) = (x[0], x[1], x[0] + x[2], x[1] + x[3]);
            assert!(left >= t[0] && top >= t[1] && bottom <= t[1] + t[3], "{lang} {width}: ✕ {i} {x:?} inside the transcript {t:?}");
            assert!(right <= t[0] + t[2] - bar, "{lang} {width}: ✕ {i} {x:?} clear of the scroll bar of {t:?}");
        }
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    // a real pointer click at the ✕'s centre reaches it
    d.click("bashcut.agent.queued.1.cancel");
    let st = d.wait(|d| d.exec("bashcut.agent.state", json!({})), |s| s["queued"].as_array().is_some_and(|q| q.len() == 2));
    assert_eq!(st["queued"], json!(["second", "fourth"]), "{st}");
    d.call("engine.execute", json!({"command": "bashcut.agent.stop", "params": {}}));
    d.no_ui_error();
}

/// The rect of automation entry `id` as left, top, right, bottom.
fn edges(d: &mut Driver, id: &str) -> Option<[f32; 4]> {
    d.rect(id).map(|r| [r[0], r[1], r[0] + r[2], r[1] + r[3]])
}

/// FX18: while the transcript scrolls, a permission card and its answer buttons end clear of the
/// transcript's floating scroll bar, at the narrowest panel too, in English and Vietnamese; a real
/// pointer click on Deny answers it.
#[test]
fn permission_card_stays_clear_of_the_transcript_scroll_bar() {
    let Some(fake) = fake_claude("permission_card_stays_clear_of_the_transcript_scroll_bar", "fx18", &[]) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.agent.send", json!({"text": "please ask, long"}));
    d.wait(|d| d.exec("bashcut.agent.state", json!({})), |s| s["permissions"].as_array().is_some_and(|p| p.len() == 1));
    let shots = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from);
    let bar = 10.0;
    for (lang, width) in [("en", 340.0), ("vi", 340.0), ("en", 240.0), ("vi", 240.0)] {
        d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
        d.exec("bashcut.ui.setWidth", json!({"width": width}));
        d.frames(3);
        d.wait(|d| json!(d.rect("bashcut.agent.permission.0.deny")), |r| !r.is_null());
        if let Some(dir) = &shots {
            std::fs::create_dir_all(dir).unwrap();
            d.harness.render().unwrap().save(dir.join(format!("fx18-permission-{lang}-{width}.png"))).unwrap();
        }
        let t = edges(&mut d, "bashcut.agent.transcript").expect("transcript");
        for id in ["bashcut.agent.permission.0", "bashcut.agent.permission.0.once", "bashcut.agent.permission.0.deny"] {
            let r = edges(&mut d, id).unwrap_or_else(|| panic!("{lang} {width}: {id} shown"));
            assert!(r[0] >= t[0] && r[2] <= t[2] - bar, "{lang} {width}: {id} {r:?} clear of the scroll bar of {t:?}");
        }
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    d.click("bashcut.agent.permission.0.deny");
    let st = d.wait(|d| d.exec("bashcut.agent.state", json!({})), |s| s["permissions"].as_array().is_some_and(Vec::is_empty));
    assert_eq!(st["permissions"], json!([]), "{st}");
    let answer = d.wait(|_| json!(fake.notes("answer")), |a| a.as_array().is_some_and(|a| a.len() == 1));
    assert_eq!(answer[0]["response"]["response"]["behavior"], json!("deny"), "{answer}");
    d.agent_idle();
    d.no_ui_error();
}

/// FX18: with enough runs that Runs scrolls, every run's card (its state label right-aligned in it)
/// ends clear of the section's floating scroll bar, at the narrowest panel too, in English and
/// Vietnamese, and so does each card's Reveal.
#[test]
fn runs_rows_stay_clear_of_the_section_scroll_bar() {
    if !has_python() {
        eprintln!("python not found: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-fx18-runs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.runs");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.py"), RUNS_PLUGIN).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.runs", "name": "Runs", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]}, "output": "bashcut-result/1",
        "actions": [{"id": "make", "title": "Make", "args": ["make"], "needs": "none", "params": []}]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();

    let mut d = Driver::with_data(&data, |_| {});
    let mut jobs = Vec::new();
    for _ in 0..16 {
        let job = d.exec("bashcut.plugins.run", json!({"plugin": "test.runs", "action": "make"}))["job"].as_u64().unwrap();
        assert_eq!(d.wait_job(job)["state"], json!("succeeded"));
        jobs.push(job);
    }
    d.click("bashcut.rail.runs");
    let shots = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from);
    let bar = 10.0;
    for (lang, width) in [("en", 340.0), ("vi", 340.0), ("en", 240.0), ("vi", 240.0)] {
        d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
        d.exec("bashcut.ui.setWidth", json!({"width": width}));
        d.frames(3);
        if let Some(dir) = &shots {
            std::fs::create_dir_all(dir).unwrap();
            d.harness.render().unwrap().save(dir.join(format!("fx18-runs-{lang}-{width}.png"))).unwrap();
        }
        let area = edges(&mut d, "bashcut.panel.runs.scroll").expect("Runs scroll area");
        let mut seen = 0;
        for job in &jobs {
            let Some(r) = edges(&mut d, &format!("bashcut.runs.{job}")) else { continue };
            seen += 1;
            assert!(r[2] <= area[2] - bar, "{lang} {width}: run {job} {r:?} clear of the scroll bar of {area:?}");
            if let Some(b) = edges(&mut d, &format!("bashcut.runs.{job}.reveal.0")) {
                assert!(b[2] <= area[2] - bar, "{lang} {width}: run {job}'s Reveal {b:?} clear of the scroll bar of {area:?}");
            }
        }
        assert!(seen >= 3, "{lang} {width}: {seen} runs on screen");
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// A scripted live Claude Code for the Agent tests below (TA2's `LiveFake` in
/// `crates/bashcut-agent/tests/fake_agent.rs`, cut down): it keeps reading stdin and takes one message
/// at a time (`system/init` with its capabilities and plugins, the `isReplay` echo with the message's
/// uuid, the answer, a `result`), answering control requests while it works. A message containing
/// `hold` works (an assistant line `working on …` first) until the `FAKE_GO` file exists; `please
/// sign in` answers like a signed-out Claude Code; `please ask` asks to run `rm -rf build` (a
/// `can_use_tool` request) and waits for the answer; `long` first answers with 60 lines (the
/// transcript scrolls) and makes `please ask`'s command a long one; `no kit` leaves `bcp` out of the
/// init's plugins.
/// The init lists the slash commands `bcp:setup`, `bcp:review` (skills), `compact` and the terminal-only `doctor`.
/// `FAKE_MODE` (comma separated): `receipt` / `cancel-queued` announce `interrupt_receipt_v1` /
/// `interrupt_cancel_queued_v1`; `slow-stop` / `slow-cancel` answer the interrupt / a cancel after a
/// pause; `outdated` refuses a flag like an old Claude Code. Every start, control request, answer,
/// taken message and EOF is a JSON line in `FAKE_LOG` ([`Fake::notes`]).
const LIVE_FAKE: &str = r#"
import json, os, queue, sys, threading, time
args = sys.argv[1:]
opts = set(os.environ.get("FAKE_MODE", "").split(","))
resumed = args[args.index("--resume") + 1] if "--resume" in args else ""
session = resumed or "live-1"
log = open(os.environ["FAKE_LOG"], "a")
go = os.environ.get("FAKE_GO", "")
def note(kind, **kw):
    log.write(json.dumps(dict(pid=os.getpid(), kind=kind, **kw)) + "\n")
    log.flush()
def out(v):
    sys.stdout.write(json.dumps(v) + "\n")
    sys.stdout.flush()
note("start", args=args)
if "outdated" in opts:
    sys.stderr.write("error: unknown option '--replay-user-messages'\n")
    sys.exit(1)
caps = [c for o, c in (("receipt", "interrupt_receipt_v1"), ("cancel-queued", "interrupt_cancel_queued_v1")) if o in opts]
lines = queue.Queue()
def read():
    for line in sys.stdin:
        try:
            lines.put(json.loads(line))
        except ValueError:
            note("bad", line=line)
    lines.put(None)
threading.Thread(target=read, daemon=True).start()
waiting = []
answers = {}
eof = False
def text_of(m):
    # the user's line, after the app's context line
    return m["message"]["content"][0]["text"].splitlines()[-1]
def control(m):
    r, rid = m.get("request", {}), m.get("request_id", "")
    note("control", request=r)
    def answer(subtype, **kw):
        out({"type": "control_response", "response": dict(subtype=subtype, request_id=rid, **kw)})
    if r.get("subtype") == "interrupt":
        if "slow-stop" in opts:
            time.sleep(2)
        answer("success")
        return "stop"
    if r.get("subtype") == "cancel_async_message":
        if "slow-cancel" in opts:
            time.sleep(1.5)
        hit = [w for w in waiting if w.get("uuid") and w.get("uuid") == r.get("message_uuid")]
        for w in hit:
            waiting.remove(w)
        answer("success", response={"cancelled": bool(hit)})
    return None
def pump(timeout):
    global eof
    try:
        m = lines.get(timeout=timeout)
    except queue.Empty:
        return None
    if m is None:
        eof = True
        return None
    if m.get("type") == "control_request":
        return control(m)
    if m.get("type") == "control_response":
        r = m.get("response", {})
        note("answer", response=r)
        answers[r.get("request_id", "")] = r
    if m.get("type") == "user":
        waiting.append(m)
    return None
def turn(m):
    text = text_of(m)
    note("take", text=text, uuid=m.get("uuid", ""))
    plugins = [] if "no kit" in text else [{"name": "bcp", "path": "/kit", "source": "bcp@bashcutpro-agent-kit"}]
    out({"type": "system", "subtype": "init", "session_id": session, "model": "fake", "capabilities": caps, "plugins": plugins,
         "slash_commands": ["bcp:setup", "bcp:review", "compact", "doctor"], "skills": ["bcp:setup", "bcp:review"], "terminal_slash_commands": ["doctor"]})
    out({"type": "user", "isReplay": True, "uuid": m.get("uuid", ""), "message": m["message"]})
    if "please sign in" in text:
        line = "Not logged in · Please run /login"
        out({"type": "assistant", "error": "authentication_failed", "message": {"content": [{"type": "text", "text": line}]}})
        out({"type": "result", "subtype": "success", "is_error": True, "result": line, "session_id": session})
        return
    if "long" in text:
        out({"type": "assistant", "message": {"content": [{"type": "text", "text": "\n\n".join("line %d of a long answer" % i for i in range(60))}]}})
    if "please ask" in text:
        command = " && ".join("echo step %d of a long command" % i for i in range(20)) if "long" in text else "rm -rf build"
        out({"type": "control_request", "request_id": "perm-1", "request": {"subtype": "can_use_tool", "tool_name": "Bash",
             "input": {"command": command}, "tool_use_id": "t9", "permission_suggestions": [
                 {"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": command}], "behavior": "allow", "destination": "localSettings"}]}})
        while "perm-1" not in answers and not eof:
            pump(0.05)
    if "hold" in text:
        out({"type": "assistant", "message": {"content": [{"type": "text", "text": "working on " + text}]}})
        while not (go and os.path.exists(go)) and not eof:
            if pump(0.05) == "stop":
                out({"type": "result", "subtype": "error_during_execution", "is_error": True, "session_id": session})
                return
    out({"type": "assistant", "message": {"content": [{"type": "text", "text": "echo " + text}]}})
    out({"type": "result", "subtype": "success", "is_error": False, "result": "done", "session_id": session, "num_turns": 1})
while True:
    if waiting:
        turn(waiting.pop(0))
    elif eof:
        note("eof")
        sys.exit(0)
    else:
        pump(0.2)
"#;

/// The child test process of [`fake_claude`]: the test it runs, the case, and the fake's folder.
const FAKE_CHILD: &str = "BASHCUT_UI_TEST_FAKE_CLAUDE";
const FAKE_CASE: &str = "BASHCUT_UI_TEST_FAKE_CASE";
const FAKE_DIR: &str = "BASHCUT_UI_TEST_FAKE_DIR";

type Opened = (std::path::PathBuf, filmcraft_ui_egui::bashcut_ui::terminal::Job, Option<String>);

/// What `terminal::open` was asked to open in a [`fake_claude`] child: folder, job and hint, oldest
/// first (no window opens).
static OPENED: std::sync::Mutex<Vec<Opened>> = std::sync::Mutex::new(Vec::new());

fn record_terminal(dir: &Path, job: &filmcraft_ui_egui::bashcut_ui::terminal::Job, hint: Option<&str>) -> Result<(), String> {
    OPENED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((dir.to_path_buf(), job.clone(), hint.map(str::to_string)));
    Ok(())
}

fn opened() -> Vec<Opened> {
    OPENED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// The real Python interpreter (`py -3`'s on Windows, so the wrapper needs no launcher in between).
fn python_exe() -> Option<String> {
    let (exe, pre): (&str, &[&str]) = if cfg!(windows) { ("py", &["-3"]) } else { ("python3", &[]) };
    let out = std::process::Command::new(exe).args(pre).args(["-c", "import sys; print(sys.executable)"]).output().ok().filter(|o| o.status.success())?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

/// A [`fake_claude`] child's fake: its folder, the program `BASHCUT_CLAUDE` names, and the case.
struct Fake {
    dir: std::path::PathBuf,
    claude: String,
    case: String,
}

impl Fake {
    /// The log lines of `kind` (`start`, `control`, `answer`, `take`, `eof`), oldest first.
    fn notes(&self, kind: &str) -> Vec<Value> {
        let text = std::fs::read_to_string(self.dir.join("fake.log")).unwrap_or_default();
        text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v["kind"] == json!(kind)).collect()
    }

    /// A `hold` turn ends.
    fn go(&self) {
        std::fs::write(self.dir.join("go"), b"").unwrap();
    }

    /// The app with the demo project saved as `film.fcproj` in the fake's folder (conversations and
    /// the agent's folder stay there), terminals recorded instead of opened, the Agent section open.
    fn driver(&self) -> Driver {
        assert!(filmcraft_ui_egui::bashcut_ui::terminal::stand_in(record_terminal), "one driver per child");
        let mut d = Driver::with_data(&self.dir.join("data"), |_| {});
        d.exec("file.saveAs", json!({"path": self.dir.join("film.fcproj").to_string_lossy()}));
        d.click("bashcut.rail.agent");
        d
    }
}

/// The Agent tests against a fake Claude Code ([`LIVE_FAKE`]). `BASHCUT_CLAUDE` is read for the
/// whole process (when the agent is created and by `bashcut.agent.agents`), and Claude Code's MCP
/// config needs the engine next to the app and a control port, so `test` runs again in a child
/// test process: from a hard link (or copy) of this test binary beside a stand-in `bashcutpro-cli`,
/// with `FILMCRAFT_CONTROL_PORT` set, `BASHCUT_CLAUDE` naming a wrapper around [`LIVE_FAKE`], and
/// `env` (`FAKE_MODE`, or another `BASHCUT_CLAUDE`). `Some` in the child (the test body runs), `None`
/// in the parent once the child passed, or without Python.
fn fake_claude(test: &str, case: &str, env: &[(&str, &str)]) -> Option<Fake> {
    if std::env::var(FAKE_CHILD).as_deref() == Ok(test) {
        let dir = std::path::PathBuf::from(std::env::var_os(FAKE_DIR)?);
        return Some(Fake { dir, claude: std::env::var("BASHCUT_CLAUDE").unwrap_or_default(), case: std::env::var(FAKE_CASE).unwrap_or_default() });
    }
    let Some(python) = python_exe() else {
        eprintln!("python not found: skipped");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("bashcut-ui-fake-{case}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = std::env::current_exe().unwrap();
    let link = bin.join(exe.file_name().unwrap());
    if std::fs::hard_link(&exe, &link).is_err() {
        std::fs::copy(&exe, &link).unwrap();
    }
    std::fs::write(bin.join(format!("bashcutpro-cli{}", std::env::consts::EXE_SUFFIX)), b"").unwrap();
    std::fs::write(dir.join("live.py"), LIVE_FAKE).unwrap();
    let claude = if cfg!(windows) {
        let w = dir.join("claude.cmd");
        std::fs::write(&w, format!("@\"{python}\" \"%~dp0live.py\" %*\r\n")).unwrap();
        w
    } else {
        let w = dir.join("claude");
        std::fs::write(&w, format!("#!/bin/sh\nexec '{python}' \"$(dirname \"$0\")/live.py\" \"$@\"\n")).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&w, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        w
    };
    let mut cmd = std::process::Command::new(&link);
    cmd.args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(FAKE_CHILD, test)
        .env(FAKE_CASE, case)
        .env(FAKE_DIR, &dir)
        .env("FILMCRAFT_CONTROL_PORT", "1")
        .env("BASHCUT_CLAUDE", &claude)
        .env("FAKE_LOG", dir.join("fake.log"))
        .env("FAKE_GO", dir.join("go"))
        .env_remove("BASHCUT_CODEX");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success() && log.contains("1 passed"), "{test} ({case}):\n{log}");
    None
}

impl Driver {
    /// The text in the Agent message box.
    fn message(&mut self) -> String {
        egui_kittest::kittest::NodeT::accesskit_node(&self.harness.get_by_role(egui::accesskit::Role::MultilineTextInput))
            .value()
            .unwrap_or_default()
            .to_string()
    }

    /// Empty the Agent message box with Backspace (the caret is at its end).
    fn clear_message(&mut self) {
        for _ in 0..self.message().chars().count() {
            self.ok("ui.key", json!({"key": "Backspace"}));
        }
        self.frames(2);
        assert_eq!(self.message(), "");
    }

    fn agent_state(&mut self) -> Value {
        self.exec("bashcut.agent.state", json!({}))
    }

    /// The agent's state once its turn (and the queue) ended.
    fn agent_idle(&mut self) -> Value {
        self.wait(|d| d.agent_state(), |s| s["status"]["state"] != json!("running") && s["status"]["state"] != json!("stopping"))
    }

    fn agent_items(&mut self) -> Vec<Value> {
        self.exec("bashcut.agent.transcript", json!({}))["items"].as_array().cloned().unwrap_or_default()
    }

    /// Wait until the transcript has the assistant line `text`.
    fn agent_said(&mut self, text: &str) {
        let said = |v: &Value| v.as_array().is_some_and(|a| a.iter().any(|i| i["type"] == json!("assistant") && i["text"] == json!(text)));
        let items = self.wait(|d| json!(d.agent_items()), said);
        assert!(said(&items), "no {text:?}: {items}");
    }

    /// The label of `id` in English, then in Vietnamese (the language is reset after).
    fn labels(&mut self, id: &str) -> (Value, Value) {
        let en = self.element(id)["label"].clone();
        self.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
        self.frames(2);
        let vi = self.element(id)["label"].clone();
        self.exec("bashcut.ui.setLanguage", json!({"language": null}));
        self.frames(2);
        (en, vi)
    }

    fn no_ui_error(&mut self) {
        assert!(self.harness.state().ui_error.is_none(), "{:?}", self.harness.state().ui_error);
    }
}

/// AG6: a turn whose Claude Code is signed out shows "Claude Code is not signed in" with Sign in…
/// and the agent's own message (selectable), not a generic error; Sign in… opens `claude` in a
/// terminal with the /login hint; the next turn clears the line.
#[test]
fn a_sign_in_failure_shows_the_not_signed_in_line_with_sign_in() {
    let Some(fake) = fake_claude("a_sign_in_failure_shows_the_not_signed_in_line_with_sign_in", "auth", &[]) else { return };
    let mut d = fake.driver();
    assert!(d.element("bashcut.agent.ready.auth").is_null(), "no line before a turn");
    d.exec("bashcut.agent.send", json!({"text": "please sign in"}));
    let st = d.agent_idle();
    assert_ne!(st["status"]["state"], json!("failed"), "no generic failure: {st}");
    d.frames(2);
    let (en, vi) = d.labels("bashcut.agent.ready.auth");
    assert_eq!((en, vi), (json!("Claude Code is not signed in"), json!("Claude Code chưa đăng nhập")));
    assert_eq!(d.element("bashcut.agent.ready.auth.message")["label"], json!("Not logged in · Please run /login"));
    let items = d.agent_items();
    assert!(!items.iter().any(|i| i["type"] == json!("error")), "no generic error: {items:?}");
    let (en, vi) = d.labels("bashcut.agent.signIn");
    assert_eq!((en, vi), (json!("Sign in…"), json!("Đăng nhập…")));
    d.click("bashcut.agent.signIn");
    let (_, job, hint) = opened().pop().expect("Sign in opened a terminal");
    assert_eq!(job, filmcraft_ui_egui::bashcut_ui::terminal::Job::Program(vec![fake.claude.clone()]));
    assert!(hint.is_some_and(|h| h.contains("/login")));

    d.exec("bashcut.agent.send", json!({"text": "hello"}));
    d.agent_said("echo hello");
    d.agent_idle();
    d.frames(2);
    assert!(d.element("bashcut.agent.ready.auth").is_null(), "the next turn clears it");
    d.no_ui_error();
}

/// AG3: with the rail hidden, a control-channel client sends twice to one live process (not closed
/// after each turn); once the Agent section was shown, hiding it while idle closes the process.
#[test]
fn a_control_channel_client_keeps_the_hidden_agent_alive() {
    let Some(fake) = fake_claude("a_control_channel_client_keeps_the_hidden_agent_alive", "hidden", &[]) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.ui.toggleRail", json!({}));
    assert!(!d.has("bashcut.panel.agent"), "hidden");
    for text in ["first", "second"] {
        d.exec("bashcut.agent.send", json!({"text": text}));
        d.agent_said(&format!("echo {text}"));
        d.agent_idle();
        d.frames(10);
        assert_eq!(d.agent_state()["live"], json!(true), "still open after the turn {text}");
    }
    assert_eq!(fake.notes("start").len(), 1, "one process for both: {:?}", fake.notes("start"));

    d.exec("bashcut.ui.toggleRail", json!({}));
    d.exec("bashcut.ui.show.agent", json!({}));
    d.frames(3);
    assert!(d.has("bashcut.panel.agent"));
    d.exec("bashcut.ui.toggleRail", json!({}));
    let st = d.wait(|d| d.agent_state(), |s| s["live"] == json!(false));
    assert_eq!(st["live"], json!(false), "hidden while idle closes it: {st}");
    let eof = d.wait(|_| json!(fake.notes("eof").len()), |n| *n == json!(1));
    assert_eq!(eof, json!(1), "the process read EOF and ended");
    d.no_ui_error();
}

/// AG3/AG10: Open in Terminal while the live agent works says "Ending the in-app session…" (both
/// languages), stops the turn and closes the process, then opens the terminal on `claude --resume
/// <session>`, and the transcript ends with "Continued in Terminal." (Vietnamese when drawn in vi).
#[test]
fn open_in_terminal_ends_the_live_agent_first() {
    let Some(fake) = fake_claude("open_in_terminal_ends_the_live_agent_first", "click", &[("FAKE_MODE", "receipt,slow-stop")]) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.agent.send", json!({"text": "hold the cut"}));
    d.agent_said("working on hold the cut");
    d.click("bashcut.agent.terminal");
    let (en, vi) = d.labels("bashcut.agent.handoff");
    assert_eq!(en, json!("Ending the in-app session before the terminal opens…"));
    assert_eq!(vi, json!("Đang đóng phiên trong ứng dụng trước khi mở Terminal…"));
    assert!(opened().is_empty(), "not before the agent ended");

    let n = d.wait(|_| json!(opened().len()), |n| *n == json!(1));
    assert_eq!(n, json!(1), "the terminal opened once the agent ended");
    let st = d.agent_state();
    assert_eq!(st["live"], json!(false), "{st}");
    assert!(fake.notes("control").iter().any(|c| c["request"]["subtype"] == json!("interrupt")), "the turn was stopped");
    let (_, job, _) = opened().remove(0);
    let filmcraft_ui_egui::bashcut_ui::terminal::Job::Program(argv) = job else { panic!("a program: {job:?}") };
    assert_eq!(argv.first(), Some(&fake.claude));
    assert!(argv.windows(2).any(|w| w == ["--resume", "live-1"]), "{argv:?}");
    let items = d.agent_items();
    assert_eq!(items.last().map(|i| i["message"].clone()), Some(json!("Continued in Terminal.")), "{items:?}");
    d.frames(2);
    assert!(d.element("bashcut.agent.handoff").is_null(), "the line goes");
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    d.frames(3);
    assert!(d.harness.query_by_label("Đã chuyển sang tiếp tục trong Terminal.").is_some(), "drawn in Vietnamese");
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    d.no_ui_error();
}

/// AG10: `bashcut.agent.terminal` while a turn runs answers `pending`; the window opens once the
/// agent ended (the frame loop polls it), and when idle it opens at once.
#[test]
fn terminal_command_hands_off_after_the_turn_ends() {
    let Some(fake) = fake_claude("terminal_command_hands_off_after_the_turn_ends", "command", &[("FAKE_MODE", "receipt")]) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.agent.send", json!({"text": "hold it"}));
    d.agent_said("working on hold it");
    let r = d.exec("bashcut.agent.terminal", json!({}));
    assert_eq!(r, json!({"opened": false, "pending": true}));
    let n = d.wait(|_| json!(opened().len()), |n| *n == json!(1));
    assert_eq!(n, json!(1), "opened once the agent ended");
    assert_eq!(d.agent_state()["live"], json!(false));
    let items = d.agent_items();
    assert_eq!(items.last().map(|i| i["message"].clone()), Some(json!("Continued in Terminal.")), "{items:?}");
    // idle: at once
    let r = d.exec("bashcut.agent.terminal", json!({}));
    assert_eq!(r, json!({"opened": true, "pending": false}));
    assert_eq!(opened().len(), 2);
    d.no_ui_error();
}

/// AG9: a session whose init lists no `bcp` plugin shows "BashCutPro skills are not installed" (both
/// languages) with Download kit… and the global install command; Download kit… opens the kit download in a
/// terminal. Before any init, or with `bcp`, the line is absent and `installKit` is refused.
#[test]
fn a_session_without_the_bcp_plugin_shows_the_kit_line() {
    use filmcraft_ui_egui::bashcut_ui::terminal;
    let Some(fake) = fake_claude("a_session_without_the_bcp_plugin_shows_the_kit_line", "kit", &[]) else { return };
    let mut d = fake.driver();
    assert!(d.element("bashcut.agent.ready.kit").is_null(), "unknown before an init");
    d.exec("bashcut.agent.send", json!({"text": "no kit here"}));
    d.agent_said("echo no kit here");
    d.agent_idle();
    d.frames(2);
    let (en, vi) = d.labels("bashcut.agent.ready.kit");
    assert_eq!((en, vi), (json!("BashCutPro skills are not downloaded"), json!("Chưa tải kỹ năng BashCutPro")));
    let (en, vi) = d.labels("bashcut.agent.installKit");
    assert_eq!((en, vi), (json!("Download kit…"), json!("Tải bộ công cụ…")));
    let cmds = d.element("bashcut.agent.ready.kit.commands")["label"].as_str().unwrap_or_default().to_string();
    assert!(cmds.contains(terminal::KIT_MARKETPLACE) && cmds.contains(terminal::KIT_INSTALL), "{cmds}");
    d.click("bashcut.agent.installKit");
    let (_, job, hint) = opened().pop().expect("Download kit opened a terminal");
    assert!(matches!(&job, terminal::Job::Script(l) if l.contains("git") && (l.contains(terminal::KIT_REPO_URL) || l.contains("pull"))), "{job:?}");
    assert!(hint.is_some_and(|h| h.contains("kit")));

    d.exec("bashcut.agent.send", json!({"text": "with the kit"}));
    d.agent_said("echo with the kit");
    d.agent_idle();
    d.frames(2);
    assert!(d.element("bashcut.agent.ready.kit").is_null(), "bcp listed: no line");
    assert!(!d.has("bashcut.agent.installKit"));
    let r = d.call("engine.execute", json!({"command": "bashcut.agent.installKit", "params": {}}));
    assert_eq!(r["ok"], json!(false), "{r}");
    assert!(r.to_string().contains("already installed"), "{r}");
    d.no_ui_error();
}

/// AG4 (and AG2's queued items): three messages sent while the live agent works show "You · queued
/// 1..3" (vi: "Bạn · đang chờ 1..3") with ✕; ✕ on the second shows "Removing…" until the agent
/// confirms, then it is gone, and only the first and third join the transcript.
#[test]
fn queued_messages_are_numbered_and_cancel_with_x() {
    let mode = [("FAKE_MODE", "receipt,cancel-queued,slow-cancel")];
    let Some(fake) = fake_claude("queued_messages_are_numbered_and_cancel_with_x", "queue", &mode) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.agent.send", json!({"text": "hold first"}));
    d.agent_said("working on hold first");
    for q in ["q1", "q2", "q3"] {
        d.exec("bashcut.agent.send", json!({"text": q}));
    }
    d.frames(3);
    for (i, q) in ["q1", "q2", "q3"].iter().enumerate() {
        assert_eq!(d.element(&format!("bashcut.agent.queued.{i}"))["label"], json!(q));
        assert_eq!(d.element(&format!("bashcut.agent.queued.{i}.cancel"))["label"], json!("Remove this message from the queue"));
        assert!(d.harness.query_by_label(&format!("You · queued {}", i + 1)).is_some(), "queued {}", i + 1);
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    d.frames(2);
    for n in 1..=3 {
        assert!(d.harness.query_by_label(&format!("Bạn · đang chờ {n}")).is_some(), "vi queued {n}");
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    d.frames(2);

    d.click("bashcut.agent.queued.1.cancel");
    assert!(d.harness.query_by_label("Removing…").is_some(), "waits for the agent");
    assert!(d.element("bashcut.agent.queued.1.cancel").is_null(), "no second ✕ meanwhile");
    let st = d.wait(|d| d.agent_state(), |s| s["queued"] == json!(["q1", "q3"]));
    assert_eq!(st["queued"], json!(["q1", "q3"]), "{st}");
    d.frames(2);
    assert_eq!(d.element("bashcut.agent.queued.1")["label"], json!("q3"));
    assert!(d.harness.query_by_label("Removing…").is_none());

    fake.go();
    d.agent_said("echo q3");
    d.agent_idle();
    let users: Vec<Value> = d.agent_items().into_iter().filter(|i| i["type"] == json!("user")).map(|i| i["text"].clone()).collect();
    assert_eq!(users, [json!("hold first"), json!("q1"), json!("q3")]);
    let taken: Vec<Value> = fake.notes("take").into_iter().map(|n| n["text"].clone()).collect();
    assert!(!taken.contains(&json!("q2")), "{taken:?}");
    d.no_ui_error();
}

/// AG8: a permission request shows its card (tool, command, Allow once / Always for this project /
/// Deny, both languages); Allow once writes an allow answer for that request and the turn goes on.
#[test]
fn a_permission_request_shows_a_card_and_the_click_answers_it() {
    let Some(fake) = fake_claude("a_permission_request_shows_a_card_and_the_click_answers_it", "permission", &[]) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.agent.send", json!({"text": "please ask"}));
    let st = d.wait(|d| d.agent_state(), |s| s["permissions"].as_array().is_some_and(|p| p.len() == 1));
    assert_eq!(st["permissions"][0]["tool"], json!("Bash"), "{st}");
    d.frames(2);
    assert!(d.has("bashcut.agent.permission.0"));
    assert_eq!(d.element("bashcut.agent.permission.0.detail")["label"], json!("rm -rf build"));
    assert!(d.harness.query_by_label("Claude Code wants to use Bash").is_some());
    let (en, vi) = d.labels("bashcut.agent.permission.0.once");
    assert_eq!((en, vi), (json!("Allow once"), json!("Cho phép lần này")));
    let (en, vi) = d.labels("bashcut.agent.permission.0.always");
    assert_eq!((en, vi), (json!("Always for this project"), json!("Luôn cho phép trong dự án này")));
    let (en, vi) = d.labels("bashcut.agent.permission.0.deny");
    assert_eq!((en, vi), (json!("Deny"), json!("Từ chối")));

    d.click("bashcut.agent.permission.0.once");
    let n = d.wait(|_| json!(fake.notes("answer").len()), |n| *n == json!(1));
    assert_eq!(n, json!(1), "the answer reached the agent");
    let answer = fake.notes("answer").remove(0);
    assert_eq!(answer["response"]["request_id"], json!("perm-1"), "{answer}");
    assert_eq!(answer["response"]["response"]["behavior"], json!("allow"), "{answer}");
    assert_eq!(answer["response"]["response"]["updatedInput"], json!({"command": "rm -rf build"}), "{answer}");
    d.agent_said("echo please ask");
    d.agent_idle();
    d.frames(2);
    assert!(!d.has("bashcut.agent.permission.0"), "the card goes");
    let items = d.agent_items();
    assert!(items.iter().any(|i| i["type"] == json!("permission") && i["tool"] == json!("Bash")), "{items:?}");
    d.no_ui_error();
}

/// AG5: a found Claude Code offers Open in Terminal and Sign in… (and `bashcut.agent.agents` lists
/// them); with `BASHCUT_CLAUDE` pointing nowhere it offers Install (both languages) next to the
/// not-found line, and Install opens the official installer in a terminal.
#[test]
fn terminal_sign_in_and_install_follow_whether_claude_is_found() {
    use filmcraft_ui_egui::bashcut_ui::terminal;
    let test = "terminal_sign_in_and_install_follow_whether_claude_is_found";
    let nowhere = std::env::temp_dir().join(format!("bashcut-ui-no-claude-{}", std::process::id())).join("claude");
    let nowhere = nowhere.to_string_lossy().into_owned();
    for (case, env) in [("found", vec![]), ("missing", vec![("BASHCUT_CLAUDE", nowhere.as_str())])] {
        let Some(fake) = fake_claude(test, case, &env) else { continue };
        let mut d = fake.driver();
        let agents = d.exec("bashcut.agent.agents", json!({}));
        let claude = agents.as_array().and_then(|a| a.iter().find(|a| a["id"] == json!("claude-code")).cloned()).unwrap_or_default();
        if fake.case == "found" {
            assert_eq!(claude["actions"], json!(["terminal", "signIn", "installKit"]), "{agents}");
            let (en, vi) = d.labels("bashcut.agent.terminal");
            assert_eq!((en, vi), (json!("Open in Terminal"), json!("Mở trong Terminal")));
            let (en, vi) = d.labels("bashcut.agent.signIn");
            assert_eq!((en, vi), (json!("Sign in…"), json!("Đăng nhập…")));
            assert!(!d.has("bashcut.agent.install"));
            assert!(d.element("bashcut.agent.ready.agent").is_null());
        } else {
            assert_eq!(claude["actions"], json!(["install"]), "{agents}");
            let (en, vi) = d.labels("bashcut.agent.install");
            assert_eq!((en, vi), (json!("Install"), json!("Cài đặt")));
            let (en, vi) = d.labels("bashcut.agent.ready.agent");
            assert_eq!(en, json!("Claude Code not found: install it, or set BASHCUT_CLAUDE"));
            assert_eq!(vi, json!("Claude Code chưa được cài: hãy cài đặt, hoặc đặt biến BASHCUT_CLAUDE"));
            assert!(!d.has("bashcut.agent.terminal") && !d.has("bashcut.agent.signIn"));
            d.click("bashcut.agent.install");
            let (_, job, hint) = opened().pop().expect("Install opened a terminal");
            assert_eq!(job, terminal::install_job(terminal::Os::current()));
            assert!(hint.is_some_and(|h| h.contains("official installer")));
        }
        d.no_ui_error();
        return;
    }
}

/// AG2: a Claude Code that refuses a flag of the live session shows "Update Claude Code: run `claude
/// update`" (both languages).
#[test]
fn an_outdated_claude_code_shows_the_update_line() {
    let Some(fake) = fake_claude("an_outdated_claude_code_shows_the_update_line", "outdated", &[("FAKE_MODE", "outdated")]) else { return };
    let mut d = fake.driver();
    assert!(d.element("bashcut.agent.ready.update").is_null());
    d.exec("bashcut.agent.send", json!({"text": "cut the intro"}));
    d.agent_idle();
    d.frames(2);
    let (en, vi) = d.labels("bashcut.agent.ready.update");
    assert_eq!((en, vi), (json!("Update Claude Code: run `claude update`"), json!("Hãy cập nhật Claude Code: chạy `claude update`")));
    d.no_ui_error();
}

/// AG11: every `bashcut.agent.*` command reads in Vietnamese in the palette under vi (English under
/// en, as MCP's command list keeps it).
#[test]
fn agent_palette_labels_are_vietnamese_under_vi() {
    let mut d = Driver::demo();
    let commands = filmcraft_ui_egui::bashcut_ui::agent::commands();
    assert!(commands.len() >= 18, "{}", commands.len());
    let label = |d: &mut Driver, id: &str| {
        let found = d.exec("bashcut.palette.search", json!({"query": id, "limit": 50}));
        let key = json!(format!("command:{id}"));
        found.as_array().and_then(|a| a.iter().find(|r| r["key"] == key).cloned()).map(|r| r["label"].clone()).unwrap_or_default()
    };
    for c in &commands {
        assert_eq!(label(&mut d, &c.id), json!(c.label), "en {}", c.id);
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    for c in &commands {
        let vi = label(&mut d, &c.id);
        assert!(vi.as_str().is_some_and(|l| !l.is_empty() && l != c.label), "{}: {vi} (English: {})", c.id, c.label);
    }
    assert_eq!(label(&mut d, "bashcut.agent.installKit"), json!("Tải bộ công cụ BashCutPro"));
    assert_eq!(label(&mut d, "bashcut.agent.send"), json!("Gửi cho trợ lý"));
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
}

/// The next launch after a session crashed with unsaved edits in `data`: one recovery candidate
/// (as `dialog_buttons_ui.rs` makes it).
fn session_after_crash(data: &Path) -> Session {
    let mut crashed = Session::default();
    crashed.execute("file.openDemoProject", json!({})).unwrap();
    let mut cfg = filmcraft_engine::autosave::AutosaveConfig::new(data);
    cfg.journal_debounce = Duration::from_millis(20);
    crashed.start_autosave(cfg).unwrap();
    crashed.execute("sequence.addEdit", json!({"seconds": 1.5})).unwrap();
    let t0 = std::time::Instant::now();
    loop {
        crashed.poll_persistence();
        if crashed.persistence.as_ref().unwrap().status.journaled_revision == crashed.revision {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "timed out waiting for the journal");
        std::thread::sleep(Duration::from_millis(10));
    }
    crashed.persistence.take().unwrap().simulate_crash();
    let mut next = Session::default();
    next.start_autosave(filmcraft_engine::autosave::AutosaveConfig::new(data)).unwrap();
    assert_eq!(next.recovery_candidates().len(), 1);
    next
}

impl Driver {
    fn from_session(session: Session) -> Self {
        let (tx, rx) = channel();
        let app = FilmcraftApp::new(session).with_control(rx);
        let harness = Harness::builder().with_size(egui::vec2(W, H)).with_max_steps(10_000).build_eframe(move |_cc| app);
        let mut d = Driver { harness, tx };
        d.frames(4);
        d
    }

    /// `ui.click` at the centre of `id`'s rect.
    fn click_at(&mut self, id: &str) -> Value {
        let r = self.rect(id).unwrap_or_else(|| panic!("{id} not on screen"));
        self.call("ui.click", json!({"x": r[0] + r[2] / 2.0, "y": r[1] + r[3] / 2.0}))
    }
}

/// FX14: with Recover Unsaved Changes open, `ui.click` on a rail id answers `ok: false` naming the
/// dialog and its ids; `recovery.later` by id still works and closes it, and the rail then opens.
/// The same for Revert, answered with an x,y click inside it.
#[test]
fn a_click_behind_the_recovery_modal_names_it() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-modal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let mut d = Driver::from_session(session_after_crash(&data));
    assert!(d.has("recovery.later"), "the prompt opens at start-up");
    let r = d.call("ui.click", json!({"id": "bashcut.rail.agent"}));
    assert_eq!(r["ok"], json!(false), "{r}");
    let e = r["error"].as_str().unwrap_or_default();
    assert!(e.contains("Recover Unsaved Changes") && e.contains("bashcut.rail.agent"), "{e}");
    for id in ["recovery.recover", "recovery.discard", "recovery.later"] {
        assert!(e.contains(id), "{id} listed: {e}");
    }
    assert!(!d.has("bashcut.panel.agent"), "nothing happened behind it");
    d.click("recovery.later");
    assert!(!d.has("recovery.later"), "Not Now closes it");
    d.click("bashcut.rail.agent");
    assert!(d.has("bashcut.panel.agent"), "the rail works again");

    d.exec("file.openDemoProject", json!({}));
    d.exec("file.saveAs", json!({"path": data.join("film.fcproj").to_string_lossy()}));
    d.exec("sequence.addEdit", json!({"seconds": 2.5}));
    d.ok("ui.menu.invoke", json!({"id": "file.revert"}));
    d.frames(3);
    assert!(d.has("revert.no"), "Revert asks first");
    let r = d.call("ui.click", json!({"id": "bashcut.rail.plugins"}));
    assert_eq!(r["ok"], json!(false), "{r}");
    let e = r["error"].as_str().unwrap_or_default();
    assert!(e.contains("Revert") && e.contains("revert.yes") && e.contains("revert.no"), "{e}");
    let r = d.click_at("revert.no");
    assert_eq!(r["ok"], json!(true), "an x,y click inside the dialog goes through: {r}");
    d.frames(3);
    assert!(!d.has("revert.no"), "No closes it");
    d.no_ui_error();
    drop(d);
    let _ = std::fs::remove_dir_all(&data);
}

/// BR6: `engine.commands` over the control channel names BashCutPro in every label and menu (File ▸
/// Export ▸ Selection as BashCutPro Project… among them), ids as `command.list` has them.
#[test]
fn control_channel_commands_name_no_filmcraft() {
    let mut d = Driver::demo();
    let list = d.ok("engine.commands", json!({}));
    let list = list.as_array().expect("a list of commands");
    let ids = d.exec("command.list", json!({}));
    let ids: Vec<&Value> = ids.as_array().expect("command.list").iter().map(|c| &c["id"]).collect();
    assert_eq!(list.iter().map(|c| &c["id"]).collect::<Vec<_>>(), ids, "same commands, same ids");
    for c in list {
        assert!(!c["label"].to_string().contains("FilmCraft") && !c["menu"].to_string().contains("FilmCraft"), "{c}");
    }
    let export = list.iter().find(|c| c["id"] == json!("file.exportSelectionProject")).expect("file.exportSelectionProject");
    assert_eq!(export["label"], json!("Selection as BashCutPro Project…"));
    d.no_ui_error();
}

/// FX24: the queued-message, dismiss and remove buttons are painted icons with a non-empty tooltip
/// label, not a ✕ font glyph (an empty box without CRAFT_FONTS_DIR).
#[test]
fn close_buttons_are_icons_with_a_label() {
    let mode = [("FAKE_MODE", "receipt,cancel-queued")];
    let Some(fake) = fake_claude("close_buttons_are_icons_with_a_label", "fx24", &mode) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.agent.send", json!({"text": "hold first"}));
    d.exec("bashcut.agent.send", json!({"text": "second"}));
    let id = "bashcut.agent.queued.0.cancel";
    let (en, vi) = {
        d.wait(|d| d.element(id), |e| !e.is_null());
        d.labels(id)
    };
    assert_eq!((en, vi), (json!("Remove this message from the queue"), json!("Bỏ tin nhắn này khỏi hàng chờ")));
    // the label is the tooltip, and no ✕ glyph is drawn as text
    let e = d.element(id);
    assert!(!e.is_null() && !e["label"].as_str().unwrap_or_default().contains('✕'), "{e}");
    let shown = d.ok("ui.elements", json!({"prefix": "bashcut.agent"}));
    assert!(!shown.to_string().contains('✕'), "no ✕ text in the Agent section: {shown}");
    d.call("engine.execute", json!({"command": "bashcut.agent.stop", "params": {}}));
    d.exec("bashcut.ui.closePanel", json!({}));

    // the Batch remove buttons and the Runs/Plugins dismiss buttons: every close icon on screen has a label
    d.click("bashcut.rail.batch");
    d.frames(2);
    let all = d.ok("ui.elements", json!({"prefix": "bashcut."}));
    for e in all.as_array().into_iter().flatten() {
        let eid = e["id"].as_str().unwrap_or_default();
        if eid.ends_with(".remove") || eid.ends_with(".dismiss") || eid.ends_with(".cancel") || eid.ends_with(".delete") {
            assert!(e["label"].as_str().is_some_and(|l| !l.trim().is_empty() && !l.contains('✕')), "{eid} has no label: {e}");
        }
    }
    d.no_ui_error();
}

/// AG12: with a fake Claude Code whose `system/init` lists `bcp:setup`, `bcp:review` and `compact`
/// (skills: the two `bcp:` ones), typing `/se` in the message box shows `/bcp:setup` first
/// (`bashcut.agent.slash.0`); Enter inserts `/bcp:setup ` without sending, ↑/↓ move the pick, Esc
/// closes, a click on a row inserts it; before the first init the list says "Start a message to
/// load skills" (`bashcut.agent.slash.hint`); Codex shows no list; English and Vietnamese.
#[test]
fn agent_slash_list_suggests_skills() {
    let Some(fake) = fake_claude("agent_slash_list_suggests_skills", "ag12", &[]) else { return };
    let mut d = fake.driver();
    let ready = |d: &mut Driver| {
        d.click("bashcut.agent.input");
        d.ok("ui.type", json!({"text": "/"}));
        d.frames(3);
    };
    // before the first init the list says a message loads the skills
    ready(&mut d);
    let (en, vi) = d.labels("bashcut.agent.slash.hint");
    assert_eq!((en, vi), (json!("Start a message to load skills"), json!("Gửi một tin nhắn để tải danh sách kỹ năng")));
    assert!(!d.has("bashcut.agent.slash.0"), "no rows yet");
    d.ok("ui.key", json!({"key": "Escape"}));
    d.frames(2);
    assert!(!d.has("bashcut.agent.slash.hint"), "Esc closes it");
    d.ok("ui.key", json!({"key": "Backspace"}));
    d.frames(2);

    // one turn: the init lists the commands
    d.exec("bashcut.agent.send", json!({"text": "hello"}));
    d.agent_said("echo hello");
    d.agent_idle();
    d.frames(3);

    // '/se' -> /bcp:setup first (prefix after the namespace), then nothing else: bcp:review and compact do not match
    ready(&mut d);
    d.ok("ui.type", json!({"text": "se"}));
    d.frames(3);
    let first = d.wait_element("bashcut.agent.slash.0", |e| !e.is_null());
    assert_eq!(first["label"], json!("/bcp:setup"), "{first}");
    assert!(!d.has("bashcut.agent.slash.1"), "only one match for /se");
    let (en, vi) = d.labels("bashcut.agent.slash.0");
    assert_eq!((en, vi), (json!("/bcp:setup"), json!("/bcp:setup")), "names are not translated");

    // Enter inserts without sending
    d.ok("ui.key", json!({"key": "Enter"}));
    d.frames(3);
    assert!(!d.has("bashcut.agent.slash.0"), "closed after the insert");
    assert_eq!(d.agent_state()["status"]["state"], json!("idle"), "Enter did not send");
    assert_eq!(d.agent_items().iter().filter(|i| i["type"] == json!("user")).count(), 1, "still only 'hello'");
    assert_eq!(d.message().trim_end(), "/bcp:setup");

    // clear the box: '/' lists skills first in the agent's order (compact is a command, doctor is terminal-only)
    d.clear_message();
    d.ok("ui.type", json!({"text": "/"}));
    d.frames(3);
    let labels: Vec<Value> = (0..4).map(|i| d.element(&format!("bashcut.agent.slash.{i}"))["label"].clone()).collect();
    assert_eq!(labels, vec![json!("/bcp:setup"), json!("/bcp:review"), json!("/compact"), Value::Null], "{labels:?}");
    // arrows move the pick, Tab inserts the picked one
    d.ok("ui.key", json!({"key": "Down"}));
    d.frames(2);
    d.ok("ui.key", json!({"key": "Tab"}));
    d.frames(3);
    assert_eq!(d.message().trim_end(), "/bcp:review");
    // Up from the first wraps to the last
    d.clear_message();
    d.ok("ui.type", json!({"text": "/"}));
    d.frames(3);
    d.ok("ui.key", json!({"key": "Up"}));
    d.frames(2);
    d.ok("ui.key", json!({"key": "Enter"}));
    d.frames(3);
    assert_eq!(d.message().trim_end(), "/compact");

    // Esc closes it; a click on a row inserts it
    d.clear_message();
    d.ok("ui.type", json!({"text": "/rev"}));
    d.frames(3);
    assert!(d.has("bashcut.agent.slash.0"));
    d.ok("ui.key", json!({"key": "Escape"}));
    d.frames(3);
    assert!(!d.has("bashcut.agent.slash.0"), "Esc closed it");
    d.ok("ui.key", json!({"key": "Backspace"}));
    d.ok("ui.type", json!({"text": "v"}));
    d.frames(3);
    d.click("bashcut.agent.slash.0");
    assert_eq!(d.message().trim_end(), "/bcp:review");
    assert_eq!(d.agent_items().iter().filter(|i| i["type"] == json!("user")).count(), 1, "a click does not send");

    // the row kind and the footer note in Vietnamese
    d.clear_message();
    d.ok("ui.type", json!({"text": "/"}));
    d.exec("bashcut.ui.setLanguage", json!({"language": "vi"}));
    d.frames(3);
    assert!(d.has("bashcut.agent.slash.0"), "still open in vi");
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));

    // Codex has no list
    d.clear_message();
    d.exec("bashcut.agent.use", json!({"agent": "codex"}));
    assert_eq!(d.agent_state()["agent"]["id"], json!("codex"));
    d.click("bashcut.agent.input");
    d.ok("ui.type", json!({"text": "/"}));
    d.frames(3);
    assert!(!d.has("bashcut.agent.slash.0") && !d.has("bashcut.agent.slash.hint"), "Codex has no slash list");
    d.no_ui_error();
}

/// FX25: at the narrowest panel, at scale 1.0 and 1.5, in English and Vietnamese, each queued
/// message's ✕ lies inside the transcript clear of its scroll bar (a wider line above it no longer
/// pushes it out), and the 'Allow AI agents to control…' checkbox, wrapped in Vietnamese, ends above
/// the transcript; a real pointer click on a ✕ still removes its message.
#[test]
fn narrow_agent_panel_keeps_queued_x_and_checkbox_in_place() {
    let mode = [("FAKE_MODE", "receipt,cancel-queued")];
    let Some(fake) = fake_claude("narrow_agent_panel_keeps_queued_x_and_checkbox_in_place", "fx25", &mode) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.agent.send", json!({"text": "hold first, long"}));
    d.wait(|d| d.exec("bashcut.agent.transcript", json!({})), |t| t["items"].to_string().contains("line 59 of a long answer"));
    for text in ["second", "third", "fourth"] {
        d.exec("bashcut.agent.send", json!({"text": text}));
    }
    let shots = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from);
    let bar = 10.0;
    for scale in [1.0, 1.5] {
        d.harness.set_pixels_per_point(scale);
        for lang in ["en", "vi"] {
            d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
            d.exec("bashcut.ui.setWidth", json!({"width": 240.0}));
            d.frames(3);
            if let Some(dir) = &shots {
                std::fs::create_dir_all(dir).unwrap();
                d.harness.render().unwrap().save(dir.join(format!("fx25-agent-{lang}-{scale}.png"))).unwrap();
            }
            let t = edges(&mut d, "bashcut.agent.transcript").expect("transcript");
            for i in 0..3 {
                let id = format!("bashcut.agent.queued.{i}.cancel");
                let x = d.wait(|d| json!(edges(d, &id)), |r| !r.is_null());
                let x: Vec<f32> = x.as_array().expect("✕ shown").iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
                assert!(x[0] >= t[0] && x[1] >= t[1] && x[3] <= t[3], "{lang} {scale}: ✕ {i} {x:?} inside the transcript {t:?}");
                assert!(x[2] <= t[2] - bar, "{lang} {scale}: ✕ {i} {x:?} clear of the scroll bar of {t:?}");
            }
            let c = edges(&mut d, "bashcut.agent.allowControl").expect("allow control checkbox");
            assert!(c[3] <= t[1], "{lang} {scale}: checkbox {c:?} ends above the transcript {t:?}");
            assert!(c[2] <= t[2] + 1.0, "{lang} {scale}: checkbox {c:?} within the panel {t:?}");
        }
    }
    d.harness.set_pixels_per_point(1.0);
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    d.frames(3);
    d.click("bashcut.agent.queued.1.cancel");
    let st = d.wait(|d| d.exec("bashcut.agent.state", json!({})), |s| s["queued"].as_array().is_some_and(|q| q.len() == 2));
    assert_eq!(st["queued"], json!(["second", "fourth"]), "{st}");
    d.call("engine.execute", json!({"command": "bashcut.agent.stop", "params": {}}));
    d.no_ui_error();
}

/// FX28: after a sign-in failure, a Claude Code that is then not found (its program removed) shows
/// only the not-found line (with Install), not the old "not signed in" line too.
#[test]
fn a_missing_claude_code_hides_the_old_not_signed_in_line() {
    let Some(fake) = fake_claude("a_missing_claude_code_hides_the_old_not_signed_in_line", "auth-missing", &[]) else { return };
    let mut d = fake.driver();
    d.exec("bashcut.agent.send", json!({"text": "please sign in"}));
    d.agent_idle();
    d.frames(2);
    assert!(!d.element("bashcut.agent.ready.auth").is_null(), "the sign-in line shows while Claude Code is found");
    std::fs::remove_file(&fake.claude).expect("remove the fake Claude Code");
    d.frames(3);
    assert!(!d.element("bashcut.agent.ready.agent").is_null(), "the not-found line shows");
    assert!(d.element("bashcut.agent.ready.auth").is_null(), "not-found wins over the old sign-in line");
    assert!(d.element("bashcut.agent.ready.auth.message").is_null());
    d.no_ui_error();
}

/// AG13: the rail and an open section show on the Import and Export pages, and Looks / Library
/// offer Open the editor there (en + vi).
#[test]
fn rail_and_panel_show_in_import_and_export_modes() {
    if !has_python() {
        eprintln!("python not found: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-rail-pages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("bashcut.resources");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.py"), RESOURCES_PLUGIN).unwrap();
    std::fs::write(plugin.join("tone.wav"), silent_wav()).unwrap();
    let string = |id: &str, flag: &str| json!({"id": id, "flag": flag, "type": "string", "optional": true});
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "bashcut.resources", "name": "Online Resources", "version": "0.1.0", "summary": "test plugin",
        "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]}, "output": "bashcut-result/1",
        "actions": [{"id": "search", "title": "Search", "args": ["search"], "needs": "none", "params": [string("kind", "--kind"), string("query", "--query"),
            {"id": "count", "flag": "--count", "type": "integer", "optional": true}]}]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();
    let mut d = Driver::with_data(&data, |_| {});
    d.exec("bashcut.ui.library.search", json!({"tab": "online", "kind": "music", "query": "calm"}));
    d.wait(|d| d.exec("bashcut.ui.library.results", json!({})), |r| r["items"].as_array().is_some_and(|a| !a.is_empty()));
    // a clip selected in Edit, so a disabled Apply is down to the page, not to the selection
    d.exec("timeline.selectNextClip", json!({}));
    assert!(!d.ok("ui.inspect", json!({}))["selection"].as_array().unwrap().is_empty(), "a clip is selected");

    for mode in ["mode.import", "mode.export"] {
        for lang in ["en", "vi"] {
            d.exec("mode.edit", json!({}));
            d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
            d.exec(mode, json!({}));
            for id in ["agent", "plugins", "runs", "looks", "library"] {
                assert!(d.has(&format!("bashcut.rail.{id}")), "{mode} {lang}: rail button {id}");
            }
            for (open, editor) in [("looks", "bashcut.looks.editor"), ("library", "bashcut.library.editor")] {
                d.click(&format!("bashcut.rail.{open}"));
                assert!(d.has(&format!("bashcut.panel.{open}")), "{mode} {lang}: panel {open}");
                assert!(d.has(editor), "{mode} {lang}: {editor} offered");
            }
            d.exec("bashcut.ui.closePanel", json!({}));
        }
    }

    // with a clip selected, Apply still waits for Edit: enabled in Edit, disabled on the other pages
    d.exec("mode.edit", json!({}));
    d.click("bashcut.rail.looks");
    let apply = d
        .ok("ui.elements", json!({"prefix": "bashcut.looks."}))
        .as_array()
        .and_then(|a| a.iter().filter_map(|e| e["id"].as_str()).find(|id| id.ends_with(".apply")).map(str::to_string))
        .expect("a look card with Apply");
    for (mode, enabled) in [("mode.edit", true), ("mode.import", false), ("mode.export", false)] {
        d.exec(mode, json!({}));
        assert!(!d.ok("ui.inspect", json!({}))["selection"].as_array().unwrap().is_empty(), "{mode}: the selection stays");
        assert_eq!(d.element(&apply)["enabled"], json!(enabled), "{mode}: Apply enabled={enabled}");
    }

    // Open the editor switches to Edit, from Looks and from Library
    for (open, editor) in [("looks", "bashcut.looks.editor"), ("library", "bashcut.library.editor")] {
        d.exec("bashcut.ui.closePanel", json!({}));
        d.exec("mode.import", json!({}));
        d.click(&format!("bashcut.rail.{open}"));
        d.click(editor);
        assert_eq!(d.ok("ui.inspect", json!({}))["ui"]["mode"], json!("Edit"), "{editor} switches to Edit");
        assert!(!d.has(editor), "{editor} is gone in Edit");
    }
    d.no_ui_error();
}

/// AG13: on Import with the Agent panel open over the page's cards, a click on the panel where a
/// card (Bars and Tone) sits behind it never reaches the card, and the mode stays Import.
#[test]
fn a_click_on_the_panel_over_an_import_card_does_not_switch_mode() {
    let mut d = Driver::demo();
    d.exec("mode.import", json!({}));
    d.click("bashcut.rail.agent");
    d.exec("bashcut.ui.setWidth", json!({"width": 720.0}));
    d.frames(3);
    let card = d.rect("import.file.newBarsAndTone").expect("the Bars and Tone card");
    let panel = d.rect("bashcut.panel.agent").expect("the Agent panel");
    let (cx, cy) = (card[0] + card[2] / 2.0, card[1] + card[3] / 2.0);
    assert!(cx > panel[0] && cx < panel[0] + panel[2], "the card's centre {cx} is under the panel {panel:?}");
    let r = d.call("ui.click", json!({"x": cx, "y": cy}));
    assert_eq!(r["ok"], json!(true), "{r}");
    d.frames(3);
    assert_eq!(d.ok("ui.inspect", json!({}))["ui"]["mode"], json!("Import"), "the click stopped at the panel");
    d.no_ui_error();
}

/// AG13: in Edit the dock is the body minus the rail and the open panel, as before the rail went
/// to every page: panel and rail sit side by side at the right edge, the panel's left edge is
/// where the dock ends.
#[test]
fn edit_mode_layout_is_the_body_minus_rail_and_panel() {
    let mut d = Driver::demo();
    d.exec("mode.edit", json!({}));
    let rail = d.rect("bashcut.rail.agent").expect("rail");
    d.click("bashcut.rail.agent");
    let panel = d.rect("bashcut.panel.agent").expect("panel");
    let rail_left = rail[0];
    let end = panel[0] + panel[2];
    assert!(end <= rail_left && end > rail_left - 12.0, "panel {panel:?} ends at the rail {rail:?}");
    let els = d.ok("ui.elements", json!({}));
    let right_of_dock: Vec<String> = els
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| !e["id"].as_str().unwrap_or_default().starts_with("bashcut."))
        .filter(|e| e["rect"][0].as_f64().unwrap_or(0.0) as f32 + e["rect"][2].as_f64().unwrap_or(0.0) as f32 > panel[0] + 2.0)
        .filter(|e| e["rect"][1].as_f64().unwrap_or(0.0) as f32 > panel[1])
        .map(|e| format!("{} {}", e["id"], e["rect"]))
        .collect();
    assert!(right_of_dock.is_empty(), "nothing of the dock runs under the panel {panel:?}: {right_of_dock:?}");
    d.no_ui_error();
}

/// FX26: at the narrowest panel (and at 340 px), in English and Vietnamese, every Looks card's
/// ☆ and Apply lie inside the section, clear of its floating scroll bar, and Batch's preset name
/// field and Save as preset ("Lưu thành mẫu") lie inside the panel.
#[test]
fn looks_stars_and_batch_save_fit_a_narrow_panel() {
    let data = std::env::temp_dir().join(format!("bashcut-ui-fx26-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let mut d = Driver::with_data(&data, |_| {});
    let shots = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from);
    let bar = 10.0;
    for (lang, width) in [("en", 340.0), ("vi", 340.0), ("en", 240.0), ("vi", 240.0)] {
        d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
        d.click("bashcut.rail.looks");
        d.exec("bashcut.ui.setWidth", json!({"width": width}));
        d.frames(6);
        if let Some(dir) = &shots {
            std::fs::create_dir_all(dir).unwrap();
            d.harness.render().unwrap().save(dir.join(format!("fx26-looks-{lang}-{width}.png"))).unwrap();
        }
        let area = edges(&mut d, "bashcut.panel.looks.scroll").expect("Looks scroll area");
        let all = d.ok("ui.elements", json!({"prefix": "bashcut.looks."}));
        let mut seen = 0;
        for e in all.as_array().expect("elements") {
            let id = e["id"].as_str().unwrap_or_default();
            if !(id.ends_with(".favourite") || id.ends_with(".apply")) {
                continue;
            }
            let r = edges(&mut d, id).expect("rect");
            if r[1] < area[1] || r[3] > area[3] {
                continue; // scrolled out of view
            }
            seen += 1;
            assert!(r[0] >= area[0] && r[2] <= area[2] - bar, "{lang} {width}: {id} {r:?} inside Looks {area:?}, clear of the bar");
        }
        assert!(seen >= 4, "{lang} {width}: {seen} stars/Apply on screen");

        d.click("bashcut.rail.batch");
        d.frames(6);
        if let Some(dir) = &shots {
            d.harness.render().unwrap().save(dir.join(format!("fx26-batch-{lang}-{width}.png"))).unwrap();
        }
        let panel = edges(&mut d, "bashcut.panel.batch").expect("Batch panel");
        for id in ["bashcut.batch.preset.name", "bashcut.batch.preset.save"] {
            let r = edges(&mut d, id).unwrap_or_else(|| panic!("{lang} {width}: {id} shown"));
            assert!(r[0] >= panel[0] && r[2] <= panel[2], "{lang} {width}: {id} {r:?} inside Batch {panel:?}");
        }
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// FX30: at the narrowest panel (and at 340 px), in English and Vietnamese, every widget of
/// Plugins (a card with settings, a parameter and a failed run), Library, Agent and Runs that is
/// on screen lies inside its panel: no non-wrapping row of a long note or label widens the
/// content and pushes the buttons below it out (failed before for Plugins' rows).
#[test]
fn plugins_library_agent_and_runs_fit_a_narrow_panel() {
    if !has_python() {
        eprintln!("python not found: skipped");
        return;
    }
    let data = std::env::temp_dir().join(format!("bashcut-ui-fx30-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let plugin = data.join("bashcut-plugins").join("test.runs");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.py"), RUNS_PLUGIN).unwrap();
    let manifest = json!({
        "schema": "bashcut-plugin/1", "id": "test.runs", "name": "Runs", "version": "0.1.0",
        "summary": "A test plugin with a summary long enough to need several lines in a narrow panel",
        "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]}, "output": "bashcut-result/1",
        "settings": [
            {"id": "apiKey", "type": "secret", "title": "API key of the service", "env": ["TEST_SECRET_KEY"]},
            {"id": "voice", "type": "string", "title": "Voice used for every line", "env": ["TEST_VOICE"]}
        ],
        "actions": [{
            "id": "fail", "title": "Fail", "args": ["fail"], "needs": "none",
            "params": [{"id": "word", "flag": "--word", "type": "string", "title": "A word to say", "optional": true}]
        }]
    });
    std::fs::write(plugin.join("bashcut-plugin.json"), manifest.to_string()).unwrap();

    let mut d = Driver::with_data(&data, |_| {});
    let job = d.exec("bashcut.plugins.run", json!({"plugin": "test.runs", "action": "fail"}))["job"].as_u64().unwrap();
    d.wait_job(job);
    let shots = std::env::var_os("BASHCUT_UI_SHOTS").map(std::path::PathBuf::from);
    let sections = [
        ("plugins", &["bashcut.plugins."][..]),
        ("library", &["bashcut.library."][..]),
        ("agent", &["bashcut.agent.", "bashcut.gate."][..]),
        ("runs", &["bashcut.runs."][..]),
    ];
    for (lang, width) in [("en", 340.0), ("vi", 340.0), ("en", 240.0), ("vi", 240.0)] {
        d.exec("bashcut.ui.setLanguage", json!({"language": lang}));
        for (section, prefixes) in sections {
            d.click(&format!("bashcut.rail.{section}"));
            d.exec("bashcut.ui.setWidth", json!({"width": width}));
            d.frames(4);
            if section == "plugins" {
                d.click("bashcut.plugins.test.runs");
                d.frames(4);
            }
            if let Some(dir) = &shots {
                std::fs::create_dir_all(dir).unwrap();
                d.harness.render().unwrap().save(dir.join(format!("fx30-{section}-{lang}-{width}.png"))).unwrap();
            }
            let panel = edges(&mut d, &format!("bashcut.panel.{section}")).unwrap_or_else(|| panic!("{lang} {width}: {section} panel"));
            let mut seen = 0;
            for prefix in prefixes {
                let all = d.ok("ui.elements", json!({"prefix": prefix}));
                for e in all.as_array().expect("elements") {
                    let id = e["id"].as_str().unwrap_or_default();
                    let Some(r) = edges(&mut d, id) else { continue };
                    if r[1] < panel[1] || r[3] > panel[3] {
                        continue; // scrolled out of view
                    }
                    seen += 1;
                    assert!(r[0] >= panel[0] - 1.0 && r[2] <= panel[2] + 1.0, "{lang} {width}: {id} {r:?} inside the {section} panel {panel:?}");
                }
            }
            assert!(seen >= 2, "{lang} {width}: {seen} widgets of {section} on screen");
        }
    }
    d.exec("bashcut.ui.setLanguage", json!({"language": null}));
    assert!(d.harness.state().ui_error.is_none(), "{:?}", d.harness.state().ui_error);
    let _ = std::fs::remove_dir_all(&data);
}

/// SY7: a plugin run or an agent gets the window's session token only for the window's own control
/// address; any other bridge gets none. The port and the token are process-wide, so the check runs in a
/// child of this test binary that launches on a port and holds a known `FILMCRAFT_CONTROL_TOKEN`.
#[test]
fn control_token_goes_only_with_the_windows_own_bridge() {
    use filmcraft_ui_egui::bashcut_ui::{control, plugins};
    const CHILD: &str = "BASHCUT_UI_TEST_TOKEN_CHILD";
    let token = "cd".repeat(32);
    if std::env::var_os(CHILD).is_none() {
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(exe)
            .args(["--exact", "control_token_goes_only_with_the_windows_own_bridge", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .env(control::ENV, "off")
            .env(bashcut_dirs::token::ENV, &token)
            .output()
            .unwrap();
        let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && log.contains("1 passed"), "{log}");
        return;
    }
    // before the launch hook ran there is no own address
    assert_eq!(plugins::control_token_for(Some("127.0.0.1:9876")), None);
    assert_eq!(control::launch_port(Some(9876), None), Some(9876));
    assert_eq!(plugins::control_token_for(Some("127.0.0.1:9876")), Some(token.clone()), "its own launched port");
    assert_eq!(plugins::control_token_for(Some("127.0.0.1:9877")), None, "another port");
    assert_eq!(plugins::control_token_for(Some("10.0.0.1:9876")), None, "another host");
    assert_eq!(plugins::control_token_for(Some("localhost:9876")), None, "only the exact address the window published");
    assert_eq!(plugins::control_token_for(None), None, "no bridge");
}
