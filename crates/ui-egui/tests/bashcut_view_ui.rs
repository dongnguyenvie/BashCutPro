//! Headless UI test of plugin views (`bashcut-view/1`, #37): every row kind draws with an automation id, a button
//! press carries the panel's values, and a live update of the view keeps what the user typed. Fork-only file.

use bashcut_plugin_host::view::{self, View};
use egui::{Event, Modifiers, PointerButton, Rect, pos2, vec2};
use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::bashcut_ui::view::{Pressed, show};
use serde_json::{Value, json};

const JOB: u64 = 7;

fn view_json(glossary: &str, extra_row: bool) -> Value {
    let mut rows = vec![
        json!({"type": "text", "text": "3 captions", "style": "title"}),
        json!({"type": "select", "id": "tone", "label": "Tone", "options": ["natural", "formal"], "value": "natural"}),
        json!({"type": "input", "id": "glossary", "label": "Glossary", "value": glossary}),
        json!({"type": "toggle", "id": "blur", "label": "Blur", "value": false}),
        json!({"type": "button", "label": "Translate", "action": "translate", "params": {"target": "vi"}, "primary": true}),
        json!({"type": "list", "id": "lines", "items": [{"id": "c1", "text": "Xin chào", "detail": "Hello", "buttons": [{"label": "Redo", "action": "translate"}]}]}),
        json!({"type": "image", "path": "missing.png"}),
        json!({"type": "progress", "value": 0.5, "label": "half"}),
    ];
    if extra_row {
        rows.push(json!({"type": "text", "text": "updated", "style": "dim"}));
    }
    json!({"schema": view::SCHEMA, "title": "Review", "rows": rows})
}

fn key_view() -> egui::Id {
    egui::Id::new("test.view")
}
fn key_pressed() -> egui::Id {
    egui::Id::new("test.pressed")
}

fn harness() -> Harness<'static, FilmcraftApp> {
    let mut h = Harness::builder().with_size(vec2(420.0, 700.0)).build_ui_state(
        |ui, app: &mut FilmcraftApp| {
            app.auto.begin_frame();
            let v: View = ui.ctx().data(|d| d.get_temp::<View>(key_view())).unwrap_or_default();
            if let Some(p) = show(app, ui, JOB, &v, None) {
                ui.ctx().data_mut(|d| d.insert_temp(key_pressed(), Some(p)));
            }
        },
        FilmcraftApp::new(Session::default()),
    );
    set_view(&mut h, &view_json("", false));
    h
}

fn set_view(h: &mut Harness<'_, FilmcraftApp>, v: &Value) {
    let v = view::parse(v).expect("a valid view");
    h.ctx.data_mut(|d| d.insert_temp(key_view(), v));
    h.run_steps(3);
}

fn rect(h: &Harness<'_, FilmcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.elements.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("missing {id}: {:?}", ids(h)));
    let [x, y, w, hh] = e.rect;
    Rect::from_min_size(pos2(x, y), vec2(w, hh))
}

fn ids(h: &Harness<'_, FilmcraftApp>) -> Vec<String> {
    h.state().auto.elements.iter().map(|e| e.id.clone()).collect()
}

fn click(h: &mut Harness<'_, FilmcraftApp>, id: &str) {
    let pos = rect(h, id).center();
    h.event(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.step();
    }
    h.run_steps(2);
}

fn pressed(h: &Harness<'_, FilmcraftApp>) -> Option<Pressed> {
    h.ctx.data(|d| d.get_temp::<Option<Pressed>>(key_pressed())).flatten()
}

#[test]
fn every_row_kind_draws_with_an_automation_id() {
    let h = harness();
    let all = ids(&h);
    for id in ["title", "text0", "tone", "glossary", "blur", "button4", "lines.c1", "lines.c1.0", "progress7"] {
        let full = format!("bashcut.view.{JOB}.{id}");
        assert!(all.contains(&full), "{full} in {all:?}");
    }
}

#[test]
fn a_press_carries_the_typed_values_and_survives_a_live_update() {
    let mut h = harness();
    click(&mut h, &format!("bashcut.view.{JOB}.glossary"));
    h.event(Event::Text("grizzly = gấu".into()));
    h.run_steps(2);
    // the plugin sends a new view (a live stdout line): the typed glossary stays
    set_view(&mut h, &view_json("", true));
    assert!(ids(&h).contains(&format!("bashcut.view.{JOB}.text8")), "the update is drawn");
    click(&mut h, &format!("bashcut.view.{JOB}.button4"));
    let p = pressed(&h).expect("Translate pressed");
    assert_eq!(p.action, "translate");
    assert_eq!(p.params["glossary"], "grizzly = gấu");
    assert_eq!(p.params["target"], "vi", "the button's own params");
    assert_eq!(p.params["tone"], "natural");
    // a list row button adds the row's id
    click(&mut h, &format!("bashcut.view.{JOB}.lines.c1.0"));
    assert_eq!(pressed(&h).expect("Redo pressed").params["item"], "c1");
}
