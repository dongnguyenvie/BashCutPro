//! BashCutPro's in-app UI (fork-only; design: `bashcut/in-app-ui.md`).
//!
//! A rail on the right edge of the window and a side panel left of it, reserved by one hook in
//! `FilmcraftApp::frame` ([`layout`]), so FilmCraft's dock, `PanelKind` and `UiState` are not
//! touched. Commands (`bashcut.ui.*`) reach us through three one-line hooks in `menus.rs`
//! ([`route!`], [`commands`], [`menu_items`]). State lives in egui context data ([`state`]).
//!
//! Adding a section: one row in [`SECTIONS`] and a `draw` function in its own file.

pub mod agent;
pub mod agent_panel;
pub mod batch;
pub mod batch_panel;
pub mod connect;
pub mod library_panel;
pub mod looks_panel;
pub mod palette;
pub mod plugins;
pub mod plugins_panel;
pub mod rail;
pub mod runs_panel;
pub mod side_panel;
pub mod state;
pub mod strings;
pub mod view;

use std::path::PathBuf;

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
}

/// The rail, top to bottom (`bashcut/in-app-ui.md` §1).
pub const SECTIONS: &[Section] = &[
    Section {
        id: "agent",
        title: "Agent",
        icon: Icon::Chat,
        blurb: "Chat with the editing agent (Claude Code, Codex) about this project.",
        draw: agent_panel::draw,
    },
    Section {
        id: "batch",
        title: "Batch",
        icon: Icon::ListView,
        blurb: "Run steps on many files at once, watch them and fix the ones that fail.",
        draw: batch_panel::draw,
    },
    Section {
        id: "looks",
        title: "Looks",
        icon: Icon::Sparkle,
        blurb: "Curated looks with a purpose each: preview, apply to the selection, favourite.",
        draw: looks_panel::draw,
    },
    Section {
        id: "library",
        title: "Library",
        icon: Icon::Folder,
        blurb: "Your saved pieces and open media online, with the licence shown before download.",
        draw: library_panel::draw,
    },
    Section { id: "plugins", title: "Plugins", icon: Icon::Grid, blurb: "Installed plugins, their settings and actions.", draw: plugins_panel::draw },
    Section { id: "runs", title: "Runs", icon: Icon::Stopwatch, blurb: "Plugin runs and agent turns with progress and results.", draw: runs_panel::draw },
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
    st.put(&ctx);
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
    if let Some(name) = id.strip_prefix("bashcut.plugins.") {
        return Some(plugins::invoke(app, ctx, name, params));
    }
    if let Some(name) = id.strip_prefix("bashcut.ui.library.") {
        return Some(library_panel::invoke(app, ctx, name, params));
    }
    if let Some(name) = id.strip_prefix("bashcut.ui.looks.") {
        return Some(looks_panel::invoke(app, ctx, name, params));
    }
    if let Some(name) = id.strip_prefix("bashcut.palette.") {
        return Some(palette::invoke(app, ctx, name, params));
    }
    if let Some(name) = id.strip_prefix("bashcut.agent.") {
        return Some(agent::invoke(app, ctx, name, params));
    }
    if let Some(name) = id.strip_prefix("bashcut.batch.") {
        return Some(batch::invoke(app, ctx, name, params));
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
            State::update(ctx, |st| st.show("agent"));
            Ok(json!("agent"))
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
pub fn params(id: &str) -> &'static str {
    match id {
        "bashcut.ui.section" => r#"{"section":"agent|batch|looks|library|plugins|runs","open":bool?}"#,
        "bashcut.ui.setWidth" => r#"{"width":px}"#,
        "bashcut.ui.popOut" => r#"{"section":"agent|batch|looks|library|plugins|runs"?}"#,
        "bashcut.ui.dockBack" => r#"{"section":"agent|batch|looks|library|plugins|runs"}"#,
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
    v.extend(plugins::commands());
    v.extend(palette::commands());
    v.extend(looks_panel::commands());
    v.extend(library_panel::commands());
    v.extend(agent::commands());
    v.extend(batch::commands());
    v
}

/// Window ▸ BashCutPro (`menus::menu_items_for`).
pub fn menu_items(session: &filmcraft_engine::Session) -> Vec<MenuItem> {
    commands()
        .into_iter()
        .filter(|c| !c.menu.is_empty())
        .map(|c| MenuItem { shortcut: session.shortcuts.primary(&c.id), id: c.id, label: c.label, path: c.menu, enabled: true, checked: None })
        .collect()
}
