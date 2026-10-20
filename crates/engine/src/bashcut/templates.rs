//! BashCutPro: project templates with slots (#42).
//!
//! A template is a bundle folder:
//!
//! ```text
//! <name>/template.json   {"schema":"bashcut-template/1","name","project":"project.fcproj","slots":[…]}
//! <name>/project.fcproj  a copy of the project
//! <name>/assets/         each slot's original media (used when a fill is missing)
//! <name>/poster.png      a program frame (best effort)
//! ```
//!
//! A slot is a file-based media item of the project (video, audio such as a music bed, or an image such as a logo).
//! Instantiating opens the template's project, replaces each slot's footage with its fill through
//! `media.replaceFootage` (clips, effects, keyframes and caption positions stay), folds those replacements into one
//! undo step and saves the result under `saveAs`. Everything goes through upstream commands; nothing upstream changes.

use std::path::{Path, PathBuf};

use filmcraft_media::MediaKind;
use filmcraft_project::{ItemId, MediaRef};
use serde_json::{Value, json};

use super::data;
use crate::commands::{CommandSpec, always, bad, bool_p, str_p};
use crate::{EngineError, Result, Session};

pub const SCHEMA: &str = "bashcut-template/1";
const MANIFEST: &str = "template.json";
const PROJECT: &str = "project.fcproj";
const MAX_SLOTS: usize = 64;
const UNDO_LABEL: &str = "Instantiate Template";

pub fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "bashcut.template.save",
            label: "Save as Template…",
            menu: &[],
            shortcut: None,
            params: r#"{"path":str (bundle folder) | "name":str (in <data dir>/bashcut/templates),"slots":[id | {"item":id,"name":str?}],"title":str?,"poster":bool=true,"overwrite":bool=false,"dir":str?}"#,
            enabled: always,
            run: save,
            journal: true,
        },
        CommandSpec {
            id: "bashcut.template.instantiate",
            label: "New Project from Template…",
            menu: &[],
            shortcut: None,
            params: r#"{"template":str (folder or name),"fills":[{"slot":str,"file":str}],"saveAs":str?,"dir":str?}"#,
            enabled: always,
            run: instantiate,
            journal: true,
        },
        CommandSpec {
            id: "bashcut.template.list",
            label: "List Templates",
            menu: &[],
            shortcut: None,
            params: r#"{"dir":str?}"#,
            enabled: always,
            run: |s, p| list(s, p),
            journal: false,
        },
    ]
}

fn err(cmd: &str, msg: impl Into<String>) -> EngineError {
    bad(cmd, msg)
}

/// A template folder from `path` / `template` (a folder) or a plain name under `<data dir>/bashcut/templates`.
fn bundle_dir(s: &Session, p: &Value, key: &str, cmd: &str) -> Result<PathBuf> {
    let v = str_p(p, key).or_else(|| str_p(p, "name")).filter(|v| !v.is_empty()).ok_or_else(|| err(cmd, format!("need `{key}`")))?;
    if data::plain_name(v) && !Path::new(v).is_dir() {
        let root = data::folder(s, p, "templates").ok_or_else(|| err(cmd, "no data directory"))?;
        return Ok(root.join(v));
    }
    Ok(PathBuf::from(v))
}

fn slot_kind(kind: MediaKind, has_video: bool) -> &'static str {
    match kind {
        MediaKind::AudioOnly => "audio",
        MediaKind::Still => "image",
        MediaKind::Movie if !has_video => "audio",
        _ => "video",
    }
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "bashcut.template.save";
    let dir = bundle_dir(s, p, "path", CMD)?;
    let manifest = dir.join(MANIFEST);
    if manifest.exists() && !bool_p(p, "overwrite").unwrap_or(false) {
        return Err(err(CMD, format!("{} already holds a template (pass overwrite:true)", dir.display())));
    }
    let wanted = p.get("slots").and_then(Value::as_array).ok_or_else(|| err(CMD, "need `slots`: [item id | {item, name}]"))?;
    if wanted.is_empty() || wanted.len() > MAX_SLOTS {
        return Err(err(CMD, format!("`slots` needs 1 to {MAX_SLOTS} items")));
    }
    // validate every slot before writing anything
    let mut slots: Vec<(String, u64, String, String, &'static str)> = Vec::with_capacity(wanted.len());
    for (i, w) in wanted.iter().enumerate() {
        let id = w.as_u64().or_else(|| w.get("item").and_then(Value::as_u64)).ok_or_else(|| err(CMD, format!("slot {i}: need an item id")))?;
        let item = s.project.item(ItemId(id)).ok_or_else(|| err(CMD, format!("slot {i}: no item {id}")))?;
        let m = item.as_media().ok_or_else(|| err(CMD, format!("slot {i}: item {id} is not media")))?;
        let MediaRef::File { path } = &m.media else { return Err(err(CMD, format!("slot {i}: item {id} is not a media file"))) };
        if slots.iter().any(|t| t.1 == id) {
            return Err(err(CMD, format!("slot {i}: item {id} is listed twice")));
        }
        let name = w.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| item.name.clone());
        let kind = slot_kind(m.info.kind, m.info.video.is_some());
        slots.push((format!("slot{}", i + 1), id, name, path.clone(), kind));
    }
    let assets = dir.join("assets");
    std::fs::create_dir_all(&assets).map_err(|e| EngineError::Other(format!("{}: {e}", assets.display())))?;
    let mut out = Vec::with_capacity(slots.len());
    for (sid, item, name, path, kind) in &slots {
        // the slot's own media travels with the template, so a missing fill still plays
        let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("bin");
        let rel = format!("assets/{sid}.{ext}");
        let asset = match std::fs::copy(path, dir.join(&rel)) {
            Ok(_) => Some(rel),
            Err(e) => {
                log::warn!("template asset {path}: {e}");
                None
            }
        };
        out.push(json!({"id": sid, "name": name, "item": item, "kind": kind, "file": asset, "original": path}));
    }
    let project_path = dir.join(PROJECT);
    s.execute("file.saveCopy", json!({"path": project_path.to_string_lossy()}))?;
    let mut poster = Value::Null;
    if bool_p(p, "poster").unwrap_or(true) && s.state.active_sequence.is_some() {
        let pp = dir.join("poster.png");
        match s.execute("file.exportFrame", json!({"target": "program", "path": pp.to_string_lossy()})) {
            Ok(_) => poster = json!("poster.png"),
            Err(e) => log::warn!("template poster: {e}"),
        }
    }
    let title = str_p(p, "title").map(str::to_string).or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
    let doc = json!({"schema": SCHEMA, "name": title, "project": PROJECT, "poster": poster, "slots": out});
    data::write_json(&manifest, &doc).map_err(EngineError::Other)?;
    Ok(json!({"path": dir.to_string_lossy(), "template": doc}))
}

struct Slot {
    id: String,
    name: String,
    item: u64,
    file: Option<String>,
}

fn read_manifest(dir: &Path, cmd: &str) -> Result<(Value, Vec<Slot>)> {
    let doc = data::read_json(&dir.join(MANIFEST), 1 << 20).map_err(|e| err(cmd, e))?;
    if doc.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
        return Err(err(cmd, format!("{} is not a {SCHEMA} template", dir.display())));
    }
    let slots = doc
        .get("slots")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(MAX_SLOTS)
                .filter_map(|v| {
                    Some(Slot {
                        id: v.get("id")?.as_str()?.to_string(),
                        name: v.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                        item: v.get("item")?.as_u64()?,
                        file: v.get("file").and_then(Value::as_str).filter(|f| !f.is_empty()).map(str::to_string),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok((doc, slots))
}

fn instantiate(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "bashcut.template.instantiate";
    let dir = bundle_dir(s, p, "template", CMD)?;
    let (doc, slots) = read_manifest(&dir, CMD)?;
    let project_rel = doc.get("project").and_then(Value::as_str).filter(|r| data::plain_name(r)).unwrap_or(PROJECT);
    let project_path = dir.join(project_rel);

    // resolve and check every fill before the open project is replaced
    let fills = match p.get("fills") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if a.len() <= MAX_SLOTS => a.clone(),
        Some(_) => return Err(err(CMD, format!("`fills` must be a list of at most {MAX_SLOTS} {{slot, file}}"))),
    };
    let mut chosen: Vec<(u64, String, String, &'static str)> = Vec::new();
    for f in &fills {
        let key = f.get("slot").and_then(Value::as_str).ok_or_else(|| err(CMD, "each fill needs `slot`"))?;
        let file = f.get("file").and_then(Value::as_str).filter(|x| !x.is_empty()).ok_or_else(|| err(CMD, format!("fill `{key}` needs `file`")))?;
        let slot = slots.iter().find(|x| x.id.eq_ignore_ascii_case(key.trim()) || x.name.eq_ignore_ascii_case(key.trim())).ok_or_else(|| {
            let names: Vec<String> = slots.iter().map(|x| format!("`{}` ({})", x.id, x.name)).collect();
            err(CMD, format!("no slot `{key}`; this template has {}", names.join(", ")))
        })?;
        if chosen.iter().any(|c| c.0 == slot.item) {
            return Err(err(CMD, format!("slot `{key}` is filled twice")));
        }
        s.services.file_size(file).map_err(|e| err(CMD, format!("{file}: {e}")))?;
        chosen.push((slot.item, slot.id.clone(), file.to_string(), "fill"));
    }
    for slot in &slots {
        if chosen.iter().any(|c| c.0 == slot.item) {
            continue;
        }
        // a missing fill keeps the template's media (its copy in assets/ when the original moved)
        if let Some(rel) = slot.file.as_deref().filter(|r| r.starts_with("assets/") && !r.contains("..")) {
            let asset = dir.join(rel);
            if asset.is_file() {
                chosen.push((slot.item, slot.id.clone(), asset.to_string_lossy().into_owned(), "template"));
            }
        }
    }

    s.execute("file.open", json!({"path": project_path.to_string_lossy()}))?;
    let group = super::undo::UndoGroup::start(s);
    let mut done = Vec::with_capacity(chosen.len());
    for (item, sid, file, from) in &chosen {
        if s.project.item(ItemId(*item)).and_then(|i| i.as_media()).is_none() {
            group.finish(s, UNDO_LABEL);
            return Err(err(CMD, format!("slot `{sid}`: item {item} is missing from the template's project")));
        }
        if let Err(e) = s.execute("media.replaceFootage", json!({"item": item, "path": file})) {
            group.finish(s, UNDO_LABEL);
            return Err(e);
        }
        done.push(json!({"slot": sid, "item": item, "file": file, "from": from}));
    }
    group.finish(s, UNDO_LABEL);
    // never write back over the template's own project
    s.path = None;
    let mut saved = Value::Null;
    if let Some(path) = str_p(p, "saveAs").filter(|x| !x.is_empty()) {
        saved = s.execute("file.saveAs", json!({"path": path}))?;
    }
    Ok(json!({"template": dir.to_string_lossy(), "slots": done, "saved": saved}))
}

fn list(s: &Session, p: &Value) -> Result<Value> {
    let Some(root) = data::folder(s, p, "templates") else { return Ok(json!({"templates": []})) };
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten().take(1000) {
            let dir = e.path();
            let Ok((doc, slots)) = read_manifest(&dir, "bashcut.template.list") else { continue };
            out.push(json!({
                "path": dir.to_string_lossy(),
                "name": doc.get("name").cloned().unwrap_or(Value::Null),
                "poster": doc.get("poster").and_then(Value::as_str).map(|x| dir.join(x).to_string_lossy().into_owned()),
                "slots": slots.iter().map(|x| json!({"id": x.id, "name": x.name})).collect::<Vec<_>>(),
            }));
        }
    }
    out.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(json!({"dir": root.to_string_lossy(), "templates": out}))
}
