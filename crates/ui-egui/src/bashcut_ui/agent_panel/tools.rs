//! Tool calls of the transcript, one row per group of calls with the same key (Agent panel
//! redesign §3.2): `[icon] title · summary   [chip] [0.8 s] [▸]`, a failed call's first error line
//! under the row, and the input and output pretty-printed with a Copy link when expanded.

use std::collections::HashMap;

use bashcut_agent::{Item, Status};
use egui::text::{LayoutJob, TextFormat};
use serde_json::{Value, json};

use super::super::strings::s;
use super::super::widgets::{CODE, META, button, code_text};
use super::super::{agent, motion, shared};
use super::tool_summary::{self, ToolStatus};
use crate::FilmcraftApp;

/// Height of a tool row (px).
const ROW: f32 = 22.0;
/// Durations are shown from this long (seconds).
const SHOW_AFTER: f64 = 0.5;
/// Tool ids whose start and end are remembered.
const TIMES_MAX: usize = 512;
/// An expanded output shows this many lines before "Show all".
const OUTPUT_LINES: usize = 12;
/// The most of an input or output shown, and the most copied.
const SHOWN_MAX: usize = 4000;
const COPY_MAX: usize = 20_000;
/// "Copied" stays this long (seconds).
const COPIED_FOR: f64 = 1.5;

/// When each tool call was first seen without output and when its output came (egui time). Live
/// only: the transcript has no timestamps, so a restored conversation shows no durations.
#[derive(Clone, Default)]
struct Times(HashMap<String, (f64, Option<f64>)>);

fn times_key() -> egui::Id {
    egui::Id::new("bashcut.agent.tool.times")
}

/// Note what `group` shows of the clock; the span of the group in seconds, when it is worth showing.
fn track(ctx: &egui::Context, group: &[Item], now: f64, running: bool) -> Option<f64> {
    let mut times = shared::temp::<Times>(ctx, times_key());
    let mut span: Option<(f64, f64)> = None;
    for g in group {
        let Item::Tool { id, output, .. } = g else { continue };
        if id.is_empty() {
            continue;
        }
        if times.0.len() >= TIMES_MAX && !times.0.contains_key(id) {
            times.0.clear();
        }
        let waiting = output.is_none();
        if waiting && !running && !times.0.contains_key(id) {
            continue;
        }
        let entry = times.0.entry(id.clone()).or_insert((now, None));
        if !waiting && entry.1.is_none() {
            entry.1 = Some(now);
        }
        // a call that never returned has no end: only a running one counts up to now
        let end = entry.1.or(running.then_some(now));
        if let Some(end) = end {
            span = Some(span.map_or((entry.0, end), |(a, b)| (a.min(entry.0), b.max(end))));
        }
    }
    shared::put_temp(ctx, times_key(), times);
    span.map(|(a, b)| b - a).filter(|d| *d >= SHOW_AFTER)
}

fn seconds_text(d: f64) -> String {
    if d < 60.0 { format!("{d:.1} s") } else { format!("{}:{:02}", (d / 60.0) as u64, (d % 60.0) as u64) }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

/// An output as pretty JSON when it is JSON, else as it came.
fn pretty_output(o: &str) -> String {
    match serde_json::from_str::<Value>(o) {
        Ok(v @ (Value::Object(_) | Value::Array(_))) => pretty(&v),
        _ => o.to_string(),
    }
}

/// The first non-empty line of a failed call's output.
fn first_error_line(group: &[Item]) -> Option<String> {
    group.iter().find_map(|g| match g {
        Item::Tool { is_error: true, output: Some(o), .. } => o.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| bashcut_agent::clip(l, 240)),
        _ => None,
    })
}

pub(super) fn tool_group(app: &mut FilmcraftApp, ui: &mut egui::Ui, at: usize, key: &str, group: &[Item]) {
    let ctx = ui.ctx().clone();
    let t = app.tokens;
    let now = ctx.input(|i| i.time);
    let name = group.iter().find_map(|g| if let Item::Tool { name, .. } = g { Some(name.as_str()) } else { None }).unwrap_or_default();
    // an output-less call is running only in the turn that is running: not in an older one, not after Stop
    let in_running_turn = agent::with(app, &ctx, |a| {
        matches!(a.status, Status::Running | Status::Stopping) && !a.conversation.transcript.items.iter().skip(at).any(|i| matches!(i, Item::User { .. }))
    });
    let status = tool_summary::status(group, in_running_turn);
    let took = track(&ctx, group, now, status == ToolStatus::Running);

    let inputs: Vec<&Value> = group.iter().filter_map(|g| if let Item::Tool { input, .. } = g { Some(input) } else { None }).collect();
    let outputs: Vec<Option<&str>> = group.iter().filter_map(|g| if let Item::Tool { output, .. } = g { Some(output.as_deref()) } else { None }).collect();
    let title = tool_summary::title(key, name);
    let summary = tool_summary::summary(key, name, &inputs, &outputs);
    let icon = tool_summary::icon_for(key, name);
    let bash = tool_summary::short_name(name) == "Bash";

    let id = egui::Id::new(("bashcut.agent.tool", at));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, id, false);

    // the row
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = motion::hover_mix(&ctx, id.with("hover"), resp.hovered());
    let painter = ui.painter().clone();
    if hover > 0.0 {
        painter.rect_filled(rect, t.radius_sm, t.hover.gamma_multiply(hover));
    }
    let mid = rect.center().y;
    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + 10.0, mid), egui::vec2(12.0, 12.0));
    crate::icons::paint(&painter, icon_rect, icon, if status == ToolStatus::Failed { t.danger } else { t.text_dim });

    // right to left: chevron, duration, chip
    let mut right = rect.right() - 4.0;
    let chevron = if state.is_open() { crate::icons::Icon::ChevronDown } else { crate::icons::Icon::ChevronRight };
    crate::icons::paint(&painter, egui::Rect::from_center_size(egui::pos2(right - 5.0, mid), egui::vec2(10.0, 10.0)), chevron, t.text_dim);
    right -= 14.0;
    let font = egui::FontId::proportional(META);
    if let Some(d) = took {
        let g = painter.layout_no_wrap(seconds_text(d), font.clone(), t.text_faint);
        right -= g.size().x;
        painter.galley(egui::pos2(right, mid - g.size().y / 2.0), g, t.text_faint);
        right -= 6.0;
    }
    match status {
        ToolStatus::Running => {
            let a = motion::pulse(&ctx, 1.2);
            painter.circle_filled(egui::pos2(right - 3.0, mid), 3.0, t.accent.gamma_multiply(a));
            right -= 12.0;
        }
        ToolStatus::Failed | ToolStatus::Stopped => {
            let failed = status == ToolStatus::Failed;
            let (label, color) = if failed { (s("Failed"), t.danger) } else { (s("Stopped"), t.text_dim) };
            let g = painter.layout_no_wrap(label.to_string(), font.clone(), color);
            let pill = egui::Rect::from_min_size(egui::pos2(right - g.size().x - 8.0, mid - 7.5), egui::vec2(g.size().x + 8.0, 15.0));
            if failed {
                painter.rect_filled(pill, t.radius_sm, t.danger.gamma_multiply(0.18));
            }
            painter.galley(egui::pos2(pill.left() + 4.0, mid - g.size().y / 2.0), g, color);
            right = pill.left() - 6.0;
        }
        ToolStatus::Ok => {}
    }

    // title · summary, ellipsized to what is left
    let left = rect.left() + 22.0;
    let mut job = LayoutJob::default();
    job.wrap.max_width = (right - left).max(8.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let mono = egui::FontId::monospace(META);
    job.append(&title, 0.0, TextFormat::simple(if bash { font.clone() } else { mono.clone() }, t.text));
    if !summary.is_empty() {
        let f = if bash { mono } else { font };
        job.append(" · ", 0.0, TextFormat::simple(egui::FontId::proportional(META), t.text_faint));
        job.append(&summary, 0.0, TextFormat::simple(f, t.text_dim));
    }
    let galley = painter.layout_job(job);
    painter.galley(egui::pos2(left, mid - galley.size().y / 2.0), galley, t.text);

    if resp.clicked() {
        state.toggle(ui);
    }
    app.auto.add_response(&format!("bashcut.agent.tool.{at}"), &resp, &title);

    if status == ToolStatus::Failed
        && let Some(line) = first_error_line(group)
    {
        ui.add(egui::Label::new(egui::RichText::new(line).size(META).color(t.danger)).wrap());
    }

    if let Some(target) = super::reveal::group_target(group) {
        super::reveal::link(app, ui, at, &target);
    }

    state.show_body_unindented(ui, |ui| {
        for (i, g) in group.iter().enumerate() {
            let Item::Tool { input, output, .. } = g else { continue };
            code_frame(app, ui, at, i, "input", &pretty(input), t.text_dim);
            if let Some(o) = output {
                code_frame(app, ui, at, i, "output", &pretty_output(o), t.text);
            }
        }
        // an engine command or plugin action the agent ran once can run on many files (#46)
        let first = group.iter().find_map(|g| match g {
            Item::Tool { input, .. } => Some(input.get("params").cloned().unwrap_or(json!({}))),
            _ => None,
        });
        if let Some(prefill) = first.and_then(|p| super::super::batch::prefill_params(key, &p))
            && button(app, ui, &format!("bashcut.agent.tool.{at}.batch"), s("Run on many files…"), true)
            && let Err(e) = super::super::batch::invoke(app, &ctx, "prefill", &prefill)
        {
            app.ui.status = e;
        }
    });
    state.store(&ctx);
}

/// `text` in a code frame (`field_bg`) with a Copy link; an output of many lines shows 12 and "Show all".
fn code_frame(app: &mut FilmcraftApp, ui: &mut egui::Ui, at: usize, i: usize, what: &str, text: &str, color: egui::Color32) {
    let ctx = ui.ctx().clone();
    let t = app.tokens;
    let now = ctx.input(|i| i.time);
    let suffix = if i == 0 { String::new() } else { format!(".{i}") };
    let copy_id = format!("bashcut.agent.tool.{at}.copy.{what}{suffix}");
    let all_id = egui::Id::new(("bashcut.agent.tool.all", at, i, what));
    let all = ctx.data(|d| d.get_temp::<bool>(all_id)).unwrap_or(false);
    let total = text.lines().count();
    let cut = !all && what == "output" && total > OUTPUT_LINES;
    let shown = if cut { text.lines().take(OUTPUT_LINES).collect::<Vec<_>>().join("\n") } else { text.to_string() };
    let copied_at = ctx.data(|d| d.get_temp::<f64>(egui::Id::new(&copy_id))).filter(|c| now - c < COPIED_FOR);

    egui::Frame::new().fill(t.field_bg).stroke(egui::Stroke::new(1.0, t.field_border)).corner_radius(t.radius_sm).inner_margin(4.0).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.add(egui::Label::new(code_text(bashcut_agent::clip(&shown, SHOWN_MAX), CODE, color)).wrap().selectable(true));
        ui.horizontal(|ui| {
            let label = if copied_at.is_some() { s("Copied") } else { s("Copy") };
            let r = ui.add(egui::Button::new(egui::RichText::new(label).size(META).color(t.text_dim)).small().frame(false));
            app.auto.add_response(&copy_id, &r, label);
            if r.clicked() {
                ctx.copy_text(bashcut_agent::clip(text, COPY_MAX));
                ctx.data_mut(|d| d.insert_temp(egui::Id::new(&copy_id), now));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(COPIED_FOR + 0.1));
            }
            if cut {
                let r = ui.add(egui::Button::new(egui::RichText::new(s("Show all")).size(META).color(t.accent)).small().frame(false));
                app.auto.add_response(&format!("bashcut.agent.tool.{at}.showAll{suffix}"), &r, s("Show all"));
                if r.clicked() {
                    ctx.data_mut(|d| d.insert_temp(all_id, true));
                }
            }
        });
    });
}
