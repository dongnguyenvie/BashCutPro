//! `bashcutpro-cli mcp` answers as BashCutPro (BR4, `bashcut/in-app-ui.md` §6): upstream's server
//! (`filmcraft_automation::FilmcraftMcp`) says `serverInfo.name = "filmcraft"` and "FilmCraft video
//! editor" in `instructions`, both of which land in the agent's context. `build.rs` routes `mcp`
//! through [`serve_stdio`], which runs upstream's `serve_io` into a pipe and rewrites the
//! server's outgoing lines on their way to stdout; the upstream crate stays untouched. Tool names,
//! resource URIs (`filmcraft://…`) and command ids are a protocol and keep upstream's spelling.
//!
//! What gets rewritten: the initialize result (`serverInfo.name`/`title`, `instructions`), the
//! `title`/`description` of every tool in `tools/list`, the text items in [`TEXTS`] (the
//! upstream messages that name the app), and a command catalog in a tool result or resource
//! (`command_list`, `command_run command.list`, `filmcraft://commands`; BR5, [`brand_commands`]).
//! `build.rs` fails if upstream's server grows another FilmCraft string. Every other line is
//! passed through byte for byte.
//!
//! tokio's `io-std`/`io-util` come from `filmcraft-automation`'s features (our `[dependencies]`
//! must mirror upstream's list, so the tokio line can't name them).

use filmcraft_automation::{AutomationError, FilmcraftMcp};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The name the agent sees for this server.
pub const SERVER_NAME: &str = "bashcutpro";

// `TEXTS`: upstream's tool-result texts that name the app, and ours (`build.rs` includes the
// same file and checks each one is still in `crates/automation/src/server.rs`).
include!("mcp_texts.rs");

/// Upstream's `FilmcraftMcp::serve_stdio`, with the outgoing lines rebranded.
pub async fn serve_stdio(server: FilmcraftMcp) -> Result<(), AutomationError> {
    let (service_out, from_service) = tokio::io::duplex(1 << 20);
    let pump = tokio::spawn(async move {
        let mut lines = BufReader::new(from_service).lines();
        let mut stdout = tokio::io::stdout();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = brand_line(&line).unwrap_or(line);
            if stdout.write_all(format!("{line}\n").as_bytes()).await.is_err() || stdout.flush().await.is_err() {
                break;
            }
        }
    });
    // `serve_io` drops its end of the pipe when it returns, which ends the pump.
    let served = server.serve_io(tokio::io::stdin(), service_out).await;
    let _ = pump.await;
    served
}

/// The rebranded line, or `None` when nothing in it changes (it is then sent as it came).
pub fn brand_line(line: &str) -> Option<String> {
    let mut v: Value = serde_json::from_str(line).ok()?;
    let result = v.get_mut("result")?.as_object_mut()?;
    let mut changed = false;
    if let Some(info) = result.get_mut("serverInfo").and_then(Value::as_object_mut) {
        info.insert("name".into(), Value::from(SERVER_NAME));
        if info.contains_key("title") {
            info.insert("title".into(), Value::from("BashCutPro"));
        }
        changed = true;
        if let Some(s) = result.get_mut("instructions") {
            rename_str(s);
        }
    }
    if let Some(tools) = result.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools.iter_mut().filter_map(Value::as_object_mut) {
            for key in ["title", "description"] {
                if let Some(s) = tool.get_mut(key) {
                    changed |= rename_str(s);
                }
            }
        }
    }
    if let Some(content) = result.get_mut("content").and_then(Value::as_array_mut) {
        for item in content.iter_mut().filter_map(Value::as_object_mut) {
            let Some(text) = item.get_mut("text") else { continue };
            if let Some((_, to)) = TEXTS.iter().find(|(from, _)| text.as_str() == Some(from)) {
                *text = Value::from(*to);
                changed = true;
            } else {
                changed |= brand_catalog_text(text);
            }
        }
    }
    // `resources/read` of the command catalog
    if let Some(contents) = result.get_mut("contents").and_then(Value::as_array_mut) {
        for item in contents.iter_mut().filter_map(Value::as_object_mut) {
            if let Some(text) = item.get_mut("text") {
                changed |= brand_catalog_text(text);
            }
        }
    }
    changed.then(|| serde_json::to_string(&v).ok()).flatten()
}

/// BR5: a text item holding a command catalog (upstream sends it as pretty JSON) gets
/// [`brand_commands`]; any other text is left alone. Whether it changed.
fn brand_catalog_text(text: &mut Value) -> bool {
    let Some(mut list) = text.as_str().and_then(|t| serde_json::from_str::<Value>(t).ok()) else { return false };
    if !brand_commands(&mut list) {
        return false;
    }
    match serde_json::to_string_pretty(&list) {
        Ok(t) => {
            *text = Value::from(t);
            true
        }
        Err(_) => false,
    }
}

/// BR5: the user text of a `command.list` result names BashCutPro: "FilmCraft" → "BashCutPro" in
/// each command's `label`, `menu` and `params`, and the program name `filmcraft-cli` →
/// `bashcutpro-cli` in `params`. Ids, shortcuts and the rest stay as they are. Only touches an array
/// whose items all look like commands (`id`, `label`, `params`). Whether anything changed.
pub fn brand_commands(list: &mut Value) -> bool {
    let Some(items) = list.as_array_mut() else { return false };
    let is_command = |c: &Value| ["id", "label", "params"].iter().all(|k| c.get(k).is_some_and(Value::is_string));
    if items.is_empty() || !items.iter().all(is_command) {
        return false;
    }
    let mut changed = false;
    for c in items.iter_mut().filter_map(Value::as_object_mut) {
        if let Some(label) = c.get_mut("label") {
            changed |= rename_str(label);
        }
        if let Some(menu) = c.get_mut("menu").and_then(Value::as_array_mut) {
            for m in menu.iter_mut() {
                changed |= rename_str(m);
            }
        }
        if let Some(params) = c.get_mut("params") {
            changed |= rename_str(params);
            if let Some(p) = params.as_str().filter(|p| p.contains("filmcraft-cli ")) {
                *params = Value::from(p.replace("filmcraft-cli ", "bashcutpro-cli "));
                changed = true;
            }
        }
    }
    changed
}

/// The CLI's `commands` / `describe` list (anchor in `build.rs`), branded by [`brand_commands`].
pub fn branded_commands(mut list: Value) -> Value {
    brand_commands(&mut list);
    list
}

/// The CLI's `exec <id>` result: a `command.list` result is branded (anchor in `build.rs`).
pub fn branded_exec(id: &str, v: Value) -> Value {
    if id == "command.list" { branded_commands(v) } else { v }
}

/// "FilmCraft" → "BashCutPro" in a string value; whether it changed.
fn rename_str(v: &mut Value) -> bool {
    match v.as_str() {
        Some(s) if s.contains("FilmCraft") => {
            *v = Value::from(s.replace("FilmCraft", "BashCutPro"));
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "test ticket: initialize result gets serverInfo.name bashcutpro and BashCutPro instructions"]
    fn initialize_is_branded() {}

    #[test]
    #[ignore = "test ticket: tools/list descriptions and the TEXTS tool results lose FilmCraft; other lines pass through unchanged"]
    fn other_lines_pass_through() {}

    #[test]
    #[ignore = "test ticket: command_list / command_run command.list / filmcraft://commands results lose FilmCraft and filmcraft-cli in label, menu and params; ids unchanged; a non-catalog JSON text is untouched"]
    fn command_catalog_is_branded() {}
}
