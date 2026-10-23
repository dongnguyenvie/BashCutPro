//! BashCutPro's name in the command catalog agents read (RC14): one rule for `bashcutpro-cli`'s MCP server and
//! CLI (`mcp_brand`, BR5) and the app's control channel (`bashcut_brand::engine_commands!`, BR6), so they can't
//! drift. Deps: serde_json only (layer 0), so any crate may use it.

use serde_json::Value;

/// The product name shown instead of upstream's.
pub const NAME: &str = "BashCutPro";

/// The user text of a `command.list` result names BashCutPro: "FilmCraft" → [`NAME`] in each command's `label`,
/// `menu` and `params`, and the program name `filmcraft-cli ` → `bashcutpro-cli ` in `params`. English, like the
/// rest of the catalog agents read; ids, shortcuts and the rest stay as they are. Only touches a non-empty array
/// whose items all look like commands (`id`, `label`, `params` strings). Whether anything changed.
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
        for m in c.get_mut("menu").and_then(Value::as_array_mut).into_iter().flatten() {
            changed |= rename_str(m);
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

/// [`brand_commands`] by value: the branded list, or `list` unchanged when it is not a command list.
pub fn branded_commands(mut list: Value) -> Value {
    brand_commands(&mut list);
    list
}

/// "FilmCraft" → [`NAME`] in a string value (MCP's server info and tool texts use it too); whether it changed.
pub fn rename_str(v: &mut Value) -> bool {
    match v.as_str() {
        Some(s) if s.contains("FilmCraft") => {
            *v = Value::from(s.replace("FilmCraft", NAME));
            true
        }
        _ => false,
    }
}
