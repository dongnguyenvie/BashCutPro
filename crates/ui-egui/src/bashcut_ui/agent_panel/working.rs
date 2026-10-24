//! The working row at the bottom of the transcript (Agent panel redesign §3.3): a pulsing dot,
//! `Working · 0:12`, and the phase from the last event (Thinking / Running {tool} / Writing /
//! Stopping), with the 30 s quiet hint under it.

use std::time::Duration;

use bashcut_agent::{Item, Status};

use super::super::motion;
use super::super::strings::{f, s};
use super::super::widgets::{NOTE, note};
use crate::FilmcraftApp;

/// Motion off: the elapsed timer needs a repaint this often.
const STEADY_REPAINT: Duration = Duration::from_secs(1);
/// Longest tool name shown in the phase word (characters).
const TOOL_MAX: usize = 28;

/// What the agent is doing now, from the status and the last transcript item.
fn phase(items: &[Item], status: &Status) -> String {
    if *status == Status::Stopping {
        return s("Stopping…").to_string();
    }
    match items.last() {
        Some(item @ Item::Tool { output: None, .. }) => {
            let tool = bashcut_agent::clip(&item.tool_key().unwrap_or_default(), TOOL_MAX);
            f("Running {tool}…", &[("tool", &tool)])
        }
        Some(Item::Assistant { .. }) => s("Writing…").to_string(),
        _ => s("Thinking…").to_string(),
    }
}

/// `m:ss` (`h:mm:ss` from an hour).
fn clock(secs: f64) -> String {
    let total = if secs.is_finite() && secs > 0.0 { secs as u64 } else { 0 };
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Draws the row for a running turn that began at `started` (egui time); `quiet` adds the hint line.
pub(super) fn row(app: &mut FilmcraftApp, ui: &mut egui::Ui, items: &[Item], status: &Status, started: f64, quiet: Option<&str>) {
    let t = app.tokens;
    let ctx = ui.ctx().clone();
    let elapsed = clock(ctx.input(|i| i.time) - started);
    let text = format!("{} · {elapsed} · {}", s("Working"), phase(items, status));
    let pulse = motion::pulse(&ctx, 1.4);
    if !motion::on(&ctx) {
        ctx.request_repaint_after(STEADY_REPAINT);
    }
    ui.add_space(6.0);
    let r = ui
        .horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.5, t.accent.gamma_multiply(pulse));
            ui.add(egui::Label::new(egui::RichText::new(&text).size(NOTE).color(t.text_dim)).wrap())
        })
        .inner;
    app.auto.add_response("bashcut.agent.working", &r, &text);
    if let Some(line) = quiet {
        let r = note(ui, s(line), NOTE, t.text);
        app.auto.add_response("bashcut.agent.quiet", &r, s(line));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "test ticket: phase word follows fake_claude steps (Thinking, Running {tool}, Writing, Stopping); elapsed advances when the driver jumps time; no repaint requested after the turn ends"]
    fn working_row_phases() {}
}
