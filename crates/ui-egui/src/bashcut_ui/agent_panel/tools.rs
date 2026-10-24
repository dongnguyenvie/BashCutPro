//! Tool calls of the transcript, grouped by command.

use bashcut_agent::Item;
use serde_json::json;

use super::super::strings::s;
use super::super::widgets::{CODE, META, button, code_note, code_text};
use crate::FilmcraftApp;

pub(super) fn tool_group(app: &mut FilmcraftApp, ui: &mut egui::Ui, at: usize, key: &str, group: &[Item]) {
    let t = app.tokens;
    let failed = group.iter().any(|g| matches!(g, Item::Tool { is_error: true, .. }));
    let waiting = group.iter().any(|g| matches!(g, Item::Tool { output: None, .. }));
    let mark = if failed {
        "×"
    } else if waiting {
        "…"
    } else {
        "•"
    };
    let title = if group.len() > 1 { format!("{mark} {key} ×{}", group.len()) } else { format!("{mark} {key}") };
    let head = egui::CollapsingHeader::new(code_text(&title, META, t.text_dim)).id_salt(("bashcut.agent.tool", at)).show(ui, |ui| {
        for g in group {
            let Item::Tool { input, output, .. } = g else { continue };
            code_note(ui, bashcut_agent::clip(&input.to_string(), 2000), CODE, t.text_dim);
            if let Some(o) = output {
                code_note(ui, bashcut_agent::clip(o, 4000), CODE, t.text);
            }
        }
        // an engine command or plugin action the agent ran once can run on many files (#46)
        let first = group.iter().find_map(|g| match g {
            Item::Tool { input, .. } => Some(input.get("params").cloned().unwrap_or(json!({}))),
            _ => None,
        });
        if let Some(prefill) = first.and_then(|p| super::super::batch::prefill_params(key, &p))
            && button(app, ui, &format!("bashcut.agent.tool.{at}.batch"), s("Run on many files…"), true)
        {
            let ctx = ui.ctx().clone();
            if let Err(e) = super::super::batch::invoke(app, &ctx, "prefill", &prefill) {
                app.ui.status = e;
            }
        }
    });
    app.auto.add_response(&format!("bashcut.agent.tool.{at}"), &head.header_response, &title);
}
