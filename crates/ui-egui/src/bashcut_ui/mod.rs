//! BashCutPro's in-app UI (fork-only; design: `bashcut/in-app-ui.md`).
//!
//! A rail on the right edge of the window and a side panel left of it, reserved by one hook in
//! `FilmcraftApp::frame` ([`layout`]), so FilmCraft's dock, `PanelKind` and `UiState` are not
//! touched. Commands (`bashcut.ui.*`) reach us through three one-line hooks in `menus.rs`
//! (`route!`, [`commands`], [`menu_items`]). State lives in egui context data ([`state`]).
//!
//! Adding a section: one row in [`SECTIONS`] and a `draw` function in its own file; its rail
//! badge and shortcut are fields of that row. A section with commands of its own adds one row to
//! [`MODULES`] (routing and [`commands`]).

pub mod agent;
pub mod agent_panel;
pub mod batch;
pub mod batch_panel;
pub mod connect;
pub mod control;
pub mod hints;
pub mod library_panel;
pub mod looks_panel;
pub mod markdown;
pub mod palette;
pub mod plugins;
pub mod plugins_panel;
pub mod prefs;
pub mod rail;
pub mod runs_panel;
pub mod shared;
pub mod side_panel;
pub mod state;
pub mod strings;
pub mod toast;
pub mod view;
pub mod widgets;

use std::path::PathBuf;
use std::sync::OnceLock;

use filmcraft_engine::shortcuts::CommandInfo;
use serde_json::{Value, json};

use crate::FilmcraftApp;
use crate::icons::Icon;
use crate::menus::MenuItem;
pub use state::State;

/// One rail section: an icon on the rail and what the side panel (or its floating window) draws.
pub struct Section {
    /// Stable id: `bashcut.rail.<id>`, `bashcut.ui.show.<id>`.
    pub id: &'static str,
    /// English title (translated through [`strings::s`]).
    pub title: &'static str,
    pub icon: Icon,
    /// One line shown under the title.
    pub blurb: &'static str,
    pub draw: fn(&mut FilmcraftApp, &mut egui::Ui),
    /// The dot on its rail icon: called every frame with whether the section is on screen
    /// (docked or popped out); the rail hides the dot while it is.
    pub badge: Option<fn(&egui::Context, &mut State, bool) -> Option<rail::Badge>>,
    /// A command whose keys the rail tooltip shows before `bashcut.ui.show.<id>`'s.
    pub shortcut: Option<&'static str>,
}

/// Section ids: the `id` of each [`SECTIONS`] row; code that opens or checks a section names it
/// by one of these, never by a string literal.
pub const AGENT: &str = "agent";
pub const BATCH: &str = "batch";
pub const LOOKS: &str = "looks";
pub const LIBRARY: &str = "library";
pub const PLUGINS: &str = "plugins";
pub const RUNS: &str = "runs";

/// The rail, top to bottom (`bashcut/in-app-ui.md` §1).
pub const SECTIONS: &[Section] = &[
    Section {
        id: AGENT,
        title: "Agent",
        icon: Icon::Chat,
        blurb: "Chat with the editing agent (Claude Code, Codex) about this project.",
        draw: agent_panel::draw,
        badge: None,
        shortcut: Some("bashcut.ui.focusAgent"),
    },
    Section {
        id: BATCH,
        title: "Batch",
        icon: Icon::ListView,
        blurb: "Run steps on many files at once, watch them and fix the ones that fail.",
        draw: batch_panel::draw,
        badge: Some(rail::batch_badge),
        shortcut: None,
    },
    Section {
        id: LOOKS,
        title: "Looks",
        icon: Icon::Sparkle,
        blurb: "Curated looks with a purpose each: preview, apply to the selection, favourite.",
        draw: looks_panel::draw,
        badge: None,
        shortcut: None,
    },
    Section {
        id: LIBRARY,
        title: "Library",
        icon: Icon::Folder,
        blurb: "Your saved pieces and open media online, with the licence shown before download.",
        draw: library_panel::draw,
        badge: None,
        shortcut: None,
    },
    Section {
        id: PLUGINS,
        title: "Plugins",
        icon: Icon::Grid,
        blurb: "Installed plugins, their settings and actions.",
        draw: plugins_panel::draw,
        badge: None,
        shortcut: None,
    },
    Section {
        id: RUNS,
        title: "Runs",
        icon: Icon::Stopwatch,
        blurb: "Plugin runs, installs and registry checks, with results.",
        draw: runs_panel::draw,
        badge: Some(rail::runs_badge),
        shortcut: None,
    },
];

/// A module's command runner: its command name (the id after the prefix) and params.
type Invoke = fn(&mut FilmcraftApp, &egui::Context, &str, &Value) -> Result<Value, String>;

/// Modules with commands of their own: id prefix, runner and command list, in [`commands`] order.
const MODULES: &[(&str, Invoke, fn() -> Vec<CommandInfo>)] = &[
    ("bashcut.plugins.", plugins::invoke, plugins::commands),
    ("bashcut.palette.", palette::invoke, palette::commands),
    ("bashcut.ui.looks.", looks_panel::invoke, looks_panel::commands),
    ("bashcut.ui.library.", library_panel::invoke, library_panel::commands),
    ("bashcut.agent.", agent::invoke, agent::commands),
    ("bashcut.batch.", batch::invoke, batch::commands),
];

/// The ids of [`SECTIONS`].
pub fn section_ids() -> Vec<&'static str> {
    SECTIONS.iter().map(|s| s.id).collect()
}

fn is_section(id: &str) -> bool {
    SECTIONS.iter().any(|s| s.id == id)
}

pub fn section(id: &str) -> Option<&'static Section> {
    SECTIONS.iter().find(|s| s.id == id)
}

/// The data directory our files go under (`<data dir>/bashcut/…`, `<data dir>/bashcut-plugins/…`):
/// the session's auto-save data directory. `None` (headless tests, no persistence) means nothing
/// is read or written.
pub fn data_dir(app: &FilmcraftApp) -> Option<PathBuf> {
    app.session.persistence.as_ref().map(|p| p.data_dir.clone())
}

/// The logic hook (`lib.rs`, `logic()`, after the control requests): runs every pass, also while an
/// occluded macOS window skips `ui` (locked screen, minimised, behind a full-screen terminal), so a
/// project opened or saved over the control channel is still published to `automation.json` (#191).
pub fn logic(app: &FilmcraftApp, ctx: &egui::Context) {
    connect::publish(app, ctx, data_dir(app).as_deref());
}

/// The layout hook (`lib.rs`, `frame()`): draws the rail, the side panel, floating sections and
/// overlays, and returns what is left of `body` for FilmCraft's dock. In Import / Export mode, or
/// with the rail hidden, `body` comes back unchanged.
pub fn layout(app: &mut FilmcraftApp, ui: &mut egui::Ui, body: egui::Rect) -> egui::Rect {
    let ctx = ui.ctx().clone();
    let known = section_ids();
    let dir = data_dir(app);
    let mut st = State::get(&ctx);
    st.load_once(dir.as_deref(), &known);
    strings::set_language(st.language.as_deref());
    plugins::poll(&ctx);
    agent::poll(&ctx);
    batch::poll(&ctx, dir.as_deref());
    let mut dock = body;
    if st.rail && app.ui.mode == crate::state::Mode::Edit {
        let rail_rect = egui::Rect::from_min_max(egui::pos2(body.max.x - rail::WIDTH, body.min.y), body.max);
        rail::show(app, ui, rail_rect, &mut st);
        dock.max.x = rail_rect.min.x - 1.0;
        // Panels switch sections through `State::update` ("Open Plugins", "Send to agent");
        // store our copy first and keep their switch, or the `put` below would undo it.
        st.clone().put(&ctx);
        let before = st.open.clone();
        if let Some(open) = st.open.clone()
            && let Some(sec) = section(&open)
        {
            dock = side_panel::show(app, ui, dock, sec, &mut st);
        }
        side_panel::floating(app, &ctx, &mut st);
        if let Some(sec) = State::get(&ctx).open.filter(|s| before.as_deref() != Some(s.as_str())) {
            st.show(&sec);
        }
    }
    if !ctx.input(|i| i.pointer.any_down()) {
        st.save_if_changed(dir.as_deref());
    }
    // Runs counts as on screen only where it draws: the rail shown, Edit mode, docked or popped out
    let runs_visible = st.rail && app.ui.mode == crate::state::Mode::Edit && st.on_screen(RUNS);
    st.put(&ctx);
    toast::show(app, &ctx, runs_visible);
    palette::show(app, &ctx);
    dock
}

/// The `menus::invoke` hook as one line: returns early with our result for our command ids.
macro_rules! route {
    ($app:expr, $ctx:expr, $id:expr, $params:expr) => {
        if let Some(r) = $crate::bashcut_ui::invoke($app, $ctx, $id, $params) {
            return r;
        }
    };
}
pub(crate) use route;

/// Run one of our UI commands; `None` for any other id (upstream handles it).
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !id.starts_with("bashcut.") {
        return None;
    }
    for (prefix, run, _) in MODULES {
        if let Some(name) = id.strip_prefix(prefix) {
            return Some(run(app, ctx, name, params));
        }
    }
    // Window ▸ BashCutPro ▸ Language: menus pass no params, so the choice is in the id
    if let Some((_, code)) = LANGUAGE_MENU.iter().find(|(menu_id, _)| *menu_id == id) {
        return invoke(app, ctx, "bashcut.ui.setLanguage", &json!({ "language": code }));
    }
    let known = section_ids();
    let section_param = || params.get("section").and_then(Value::as_str).filter(|s| is_section(s)).map(str::to_string);
    let r = match id {
        "bashcut.ui.state" => Ok(serde_json::to_value(State::get(ctx)).unwrap_or(Value::Null)),
        "bashcut.ui.toggleRail" => Ok(json!(State::update(ctx, |st| {
            st.rail = !st.rail;
            st.rail
        }))),
        "bashcut.ui.focusAgent" => {
            State::update(ctx, |st| st.show(AGENT));
            Ok(json!(AGENT))
        }
        "bashcut.ui.section" => {
            let Some(sec) = section_param() else { return Some(Err(format!("section must be one of {}", known.join(", ")))) };
            let open = params.get("open").and_then(Value::as_bool);
            State::update(ctx, |st| match open {
                Some(true) => st.show(&sec),
                Some(false) if st.open.as_deref() == Some(sec.as_str()) => st.open = None,
                Some(false) => {}
                None => st.toggle(&sec),
            });
            Ok(serde_json::to_value(State::get(ctx)).unwrap_or(Value::Null))
        }
        "bashcut.ui.closePanel" => {
            State::update(ctx, |st| st.open = None);
            Ok(Value::Null)
        }
        "bashcut.ui.setWidth" => {
            let Some(w) = params.get("width").and_then(Value::as_f64) else { return Some(Err("width (px) is required".into())) };
            let w = State::update(ctx, |st| {
                st.width = w as f32;
                st.sanitize(&known);
                st.width
            });
            Ok(json!(w))
        }
        "bashcut.ui.popOut" => {
            let sec = section_param().or_else(|| State::get(ctx).open);
            let Some(sec) = sec else { return Some(Err("no section open; pass section".into())) };
            State::update(ctx, |st| st.pop_out(&sec));
            Ok(json!(sec))
        }
        "bashcut.ui.dockBack" => {
            let Some(sec) = section_param() else { return Some(Err(format!("section must be one of {}", known.join(", ")))) };
            State::update(ctx, |st| st.dock_back(&sec));
            Ok(json!(sec))
        }
        "bashcut.ui.setLanguage" => {
            let lang = params.get("language").and_then(Value::as_str).filter(|l| strings::LANGUAGES.iter().any(|k| k == l)).map(str::to_string);
            State::update(ctx, |st| st.language = lang.clone());
            Ok(json!(lang))
        }
        _ => {
            let sec = id.strip_prefix("bashcut.ui.show.").filter(|s| is_section(s))?;
            State::update(ctx, |st| st.show(sec));
            Ok(json!(sec))
        }
    };
    Some(r)
}

/// The parameter line of a rail command, in the engine's notation, so the palette's editor says what to type.
/// The section hints are built once from [`SECTIONS`].
pub fn params(id: &str) -> &'static str {
    static SECTION_HINTS: OnceLock<[String; 3]> = OnceLock::new();
    let hints = SECTION_HINTS.get_or_init(|| {
        let list = section_ids().join("|");
        [format!(r#"{{"section":"{list}","open":bool?}}"#), format!(r#"{{"section":"{list}"?}}"#), format!(r#"{{"section":"{list}"}}"#)]
    });
    match id {
        "bashcut.ui.section" => &hints[0],
        "bashcut.ui.setWidth" => r#"{"width":px}"#,
        "bashcut.ui.popOut" => &hints[1],
        "bashcut.ui.dockBack" => &hints[2],
        "bashcut.ui.setLanguage" => r#"{"language":"en|vi"?}"#,
        _ => "",
    }
}

/// Our frontend commands, for the shortcut editor and the command list (`menus::external_commands`).
pub fn commands() -> Vec<CommandInfo> {
    let mut v = vec![
        CommandInfo::new("bashcut.ui.toggleRail", "Show Rail", &["Window", "BashCutPro"], None),
        // Cmd+Shift+A is upstream's Deselect All, so Agent takes the free Cmd+Alt+Shift+A
        CommandInfo::new("bashcut.ui.focusAgent", "Focus Agent", &[], Some("Cmd+Alt+Shift+A")),
        CommandInfo::new("bashcut.ui.section", "Toggle Section", &[], None),
        CommandInfo::new("bashcut.ui.closePanel", "Close Side Panel", &[], None),
        CommandInfo::new("bashcut.ui.setWidth", "Side Panel Width", &[], None),
        CommandInfo::new("bashcut.ui.popOut", "Pop Out Section", &[], None),
        CommandInfo::new("bashcut.ui.dockBack", "Dock Section Back", &[], None),
        CommandInfo::new("bashcut.ui.setLanguage", "BashCutPro Language", &[], None),
        CommandInfo::new("bashcut.ui.state", "Rail State", &[], None),
    ];
    for sec in SECTIONS {
        v.push(CommandInfo::new(&format!("bashcut.ui.show.{}", sec.id), sec.title, &["Window", "BashCutPro"], None));
    }
    for (_, _, list) in MODULES {
        v.extend(list());
    }
    // last, so the Language submenu ends Window ▸ BashCutPro
    v.extend([
        CommandInfo::new(LANGUAGE_MENU[0].0, "Follow App", LANGUAGE_PATH, None),
        CommandInfo::new(LANGUAGE_MENU[1].0, "English", LANGUAGE_PATH, None),
        CommandInfo::new(LANGUAGE_MENU[2].0, "Tiếng Việt", LANGUAGE_PATH, None),
    ]);
    v
}

/// Window ▸ BashCutPro ▸ Language: one item per choice, each running `bashcut.ui.setLanguage` with
/// its code (`None` follows the interface language).
const LANGUAGE_MENU: [(&str, Option<&str>); 3] =
    [("bashcut.ui.setLanguage.follow", None), ("bashcut.ui.setLanguage.en", Some("en")), ("bashcut.ui.setLanguage.vi", Some("vi"))];
const LANGUAGE_PATH: &[&str] = &["Window", "BashCutPro", "Language"];

/// Window ▸ BashCutPro (`menus::menu_items_for`), in our language (upstream's catalogs then
/// translate whatever we left in English), with a check on the current language.
pub fn menu_items(session: &filmcraft_engine::Session) -> Vec<MenuItem> {
    commands()
        .into_iter()
        .filter(|c| !c.menu.is_empty())
        .map(|c| {
            let checked = LANGUAGE_MENU.iter().find(|(id, _)| *id == c.id).map(|(_, code)| *code == strings::chosen());
            MenuItem {
                shortcut: session.shortcuts.primary(&c.id),
                label: strings::s(&c.label).to_string(),
                path: c.menu.iter().map(|p| strings::s(p).to_string()).collect(),
                id: c.id,
                enabled: true,
                checked,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `params` builds the section list from [`SECTIONS`]; this keeps its notation in check.
    #[test]
    fn params_list_every_section() {
        let list = section_ids().join("|");
        for id in ["bashcut.ui.section", "bashcut.ui.popOut", "bashcut.ui.dockBack"] {
            let p = params(id);
            assert!(p.contains(&format!(r#""section":"{list}""#)), "{id}: {p} should list {list}");
        }
    }

    /// RU6 made routing and [`commands`] one table; this pins what the hand-written code did.
    #[test]
    fn modules_route_and_list() {
        // the prefixes and the order before RU6: commands() listed plugins, palette, looks,
        // library, agent, batch after the section commands
        let prefixes: Vec<&str> = MODULES.iter().map(|(p, _, _)| *p).collect();
        assert_eq!(prefixes, ["bashcut.plugins.", "bashcut.palette.", "bashcut.ui.looks.", "bashcut.ui.library.", "bashcut.agent.", "bashcut.batch."]);
        let all: Vec<String> = commands().into_iter().map(|c| c.id).collect();
        let mut seen = std::collections::BTreeSet::new();
        assert!(all.iter().all(|id| seen.insert(id.clone())), "an id is listed twice: {all:?}");

        // each module's commands sit together, in MODULES order, after the section commands
        let mut at = all.iter().position(|id| id == &format!("bashcut.ui.show.{}", RUNS)).expect("section commands") + 1;
        for (i, (prefix, _, list)) in MODULES.iter().enumerate() {
            let ids: Vec<String> = list().into_iter().map(|c| c.id).collect();
            assert!(!ids.is_empty(), "{prefix} lists commands");
            assert_eq!(all.get(at..at + ids.len()), Some(ids.as_slice()), "{prefix} at {at}");
            at += ids.len();
            for id in &ids {
                // the first prefix that matches is the module's own: none routes to another
                let first = MODULES.iter().position(|(p, _, _)| id.starts_with(p));
                assert_eq!(first, Some(i), "{id} routes to {:?}, not {prefix}", first.and_then(|f| MODULES.get(f)).map(|m| m.0));
            }
        }
        // the Language submenu stays last
        assert_eq!(all.get(at..).map(<[String]>::len), Some(LANGUAGE_MENU.len()));
        assert!(LANGUAGE_MENU.iter().zip(&all[at..]).all(|((id, _), listed)| id == listed));
        // our own ui commands and the language items are not taken by a module prefix
        for id in ["bashcut.ui.state", "bashcut.ui.section", "bashcut.ui.show.agent", LANGUAGE_MENU[0].0] {
            assert!(!MODULES.iter().any(|(p, _, _)| id.starts_with(p)), "{id}");
        }
        // upstream's ids are left to upstream
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        assert!(invoke(&mut app, &egui::Context::default(), "edit.undo", &json!({})).is_none());
    }

    /// Our default keys never land on an upstream default (any command's own key, the panel
    /// tables, a Premiere key the audit or a preset can adopt, the other presets) on macOS, Windows
    /// or Linux, in any context (a panel key shadows an application one), and every built-in preset
    /// keeps them.
    #[test]
    fn our_default_keys_never_share_a_chord_with_upstream() {
        use filmcraft_engine::shortcut_presets as presets;
        use filmcraft_engine::shortcuts::{BUILTIN_PRESETS, Chord, Platform, Shortcuts};
        let mut sc = Shortcuts::new();
        sc.register_external(crate::menus::external_commands());
        let all = sc.commands();
        let ours: Vec<(String, String)> =
            all.iter().filter(|c| c.id.starts_with("bashcut.")).flat_map(|c| c.defaults.iter().map(|(k, _)| (c.id.clone(), k.clone()))).collect();
        assert!(ours.iter().any(|(id, _)| id == "bashcut.palette.open"), "{ours:?}");
        assert!(ours.iter().any(|(id, _)| id == "bashcut.ui.focusAgent"), "{ours:?}");
        let mut upstream: Vec<(String, String)> =
            all.iter().filter(|c| !c.id.starts_with("bashcut.")).flat_map(|c| c.defaults.iter().map(|(k, _)| (c.id.clone(), k.clone()))).collect();
        for t in [presets::FILMCRAFT_PANEL, presets::PREMIERE, presets::PREMIERE_PANEL, presets::FINAL_CUT, presets::AVID] {
            upstream.extend(t.iter().map(|(c, k, _)| (c.to_string(), k.to_string())));
        }
        assert!(upstream.len() > 100, "the upstream tables are read: {}", upstream.len());
        for p in [Platform::Mac, Platform::Windows, Platform::Linux] {
            for (id, keys) in &ours {
                let mine = Chord::parse(keys).map(|c| c.effective(p)).unwrap_or_else(|e| panic!("{id}: {keys}: {e}"));
                for (other, k) in &upstream {
                    let Ok(theirs) = Chord::parse(k) else { continue };
                    assert_ne!(theirs.effective(p), mine, "{p:?}: {id} ({keys}) shares a key with {other} ({k})");
                }
            }
        }
        // what the app builds here: no conflict or panel shadow names ours, and each preset keeps them
        let here = Platform::current();
        assert!(sc.conflicts(here).iter().all(|(_, _, v)| !v.iter().any(|id| id.starts_with("bashcut."))), "{:?}", sc.conflicts(here));
        assert!(sc.overrides(here).iter().all(|o| !o.2.starts_with("bashcut.") && !o.3.starts_with("bashcut.")), "{:?}", sc.overrides(here));
        for name in BUILTIN_PRESETS {
            let b = sc.builtin(name).unwrap_or_default();
            for (id, keys) in &ours {
                assert!(b.iter().any(|x| &x.command == id && &x.keys == keys), "{name} keeps {id} on {keys}");
            }
        }
    }
}
