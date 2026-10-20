//! BashCutPro: where burned-in captions and on-screen text land in the frame (#205), for `bashcut.review.run`.
//!
//! [`caption_box`] lays a caption out as the renderer does (`filmcraft_captions::burn::render_caption`: the track
//! style's size at 1080 lines, wrapped to 90 % of the width, the anchor and margin, WebVTT `line:` / `align:`
//! settings, the background padding and the outline) without rasterising it, so a review can measure every cue:
//! the box in frame pixels and the lines the renderer draws after wrapping. [`text_boxes`] gives the frame bounds of
//! each text layer of the graphic clips on the video tracks (layer quad through the clip's Motion). A test keeps
//! [`caption_box`] equal to the rendered overlay's box, so an upstream layout change shows up there.

use filmcraft_project::graphic::LayerContent;
use filmcraft_project::{Caption, CaptionAlign, CaptionAnchor, CaptionStyle, ClipId, Sequence};
use filmcraft_text::TextStyle;
use filmcraft_time::Tick;

use crate::Session;

/// At most this many text clips are measured.
const MAX_CLIPS: usize = 5_000;

/// A laid-out text block in frame pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    /// Lines drawn (after wrapping).
    pub lines: usize,
}

/// Insets of the frame in pixels that platform UI covers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

impl Insets {
    /// Insets of a `w`×`h` frame from fractions.
    pub fn of(a: &super::profiles::SafeArea, w: f64, h: f64) -> Insets {
        let f = |v: f64| if v.is_finite() { v.clamp(0.0, 0.45) } else { 0.0 };
        Insets { top: f(a.top) * h, bottom: f(a.bottom) * h, left: f(a.left) * w, right: f(a.right) * w }
    }

    pub fn is_empty(&self) -> bool {
        self.top <= 0.0 && self.bottom <= 0.0 && self.left <= 0.0 && self.right <= 0.0
    }

    /// How far a box reaches into each band, pixels: `[top, bottom, left, right]` (0 = clear).
    pub fn intrusion(&self, b: &TextBox, w: f64, h: f64) -> [f64; 4] {
        [(self.top - b.y0).max(0.0), (b.y1 - (h - self.bottom)).max(0.0), (self.left - b.x0).max(0.0), (b.x1 - (w - self.right)).max(0.0)]
            .map(|v| if self.is_empty() { 0.0 } else { v })
    }
}

/// The caption face, as the renderer picks it: the track's family (Inter when empty) in SemiBold at `px`.
fn caption_style(font: &str, px: f32) -> TextStyle {
    let family = if font.trim().is_empty() { filmcraft_text::fonts::DEFAULT_FAMILY } else { font.trim() };
    TextStyle { family: family.into(), style: "SemiBold".into(), size: px, ..Default::default() }
}

/// Word-wrap one line to `max_w` pixels, as the renderer does.
fn wrap(line: &str, ts: &TextStyle, max_w: f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in line.split(' ') {
        let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if !cur.is_empty() && filmcraft_text::measure(&cand, ts) > max_w {
            out.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = cand;
        }
    }
    out.push(cur);
    out
}

/// WebVTT `line:N%` and `align:` cue settings.
fn vtt_overrides(settings: &str) -> (Option<f32>, Option<CaptionAlign>) {
    let mut line = None;
    let mut align = None;
    for kv in settings.split_whitespace() {
        let Some((k, v)) = kv.split_once(':') else { continue };
        match k {
            "line" => {
                let v = v.split(',').next().unwrap_or("");
                if let Some(p) = v.strip_suffix('%').and_then(|p| p.parse::<f32>().ok()) {
                    line = Some((p / 100.0).clamp(0.0, 1.0));
                }
            }
            "align" => {
                align = match v {
                    "start" | "left" => Some(CaptionAlign::Left),
                    "end" | "right" => Some(CaptionAlign::Right),
                    "center" | "middle" => Some(CaptionAlign::Center),
                    _ => None,
                }
            }
            _ => {}
        }
    }
    (line, align)
}

/// Box and drawn lines of caption `c` in a `w`×`h` frame; `None` when nothing is drawn.
pub fn caption_box(c: &Caption, style: &CaptionStyle, w: usize, h: usize) -> Option<TextBox> {
    if w == 0 || h == 0 {
        return None;
    }
    let (wf, hf) = (w as f32, h as f32);
    let scale = hf / 1080.0;
    let px = (style.size * scale).max(4.0);
    let ts = caption_style(&style.font, px);
    let mut lines: Vec<String> = Vec::new();
    for l in c.plain_lines() {
        lines.extend(wrap(l.trim(), &ts, wf * 0.9));
    }
    lines.retain(|l| !l.is_empty());
    if lines.is_empty() {
        return None;
    }
    let lh = px * style.line_spacing.max(0.8);
    let pad_x = (px * 0.3).round();
    let block_h = lh * lines.len() as f32;
    let (line_override, align_override) = vtt_overrides(&c.settings);
    let align = align_override.unwrap_or(style.align);
    let margin = style.margin.clamp(0.0, 0.45) * hf;
    let top = match (line_override, style.anchor) {
        (Some(f), _) => (f * hf).min(hf - block_h),
        (None, CaptionAnchor::Top) => margin,
        (None, CaptionAnchor::Middle) => (hf - block_h) / 2.0,
        (None, CaptionAnchor::Bottom) => hf - margin - block_h,
    }
    .max(0.0)
    .round();
    let side = wf * 0.05;
    let grow = (style.outline * scale).max(0.0).ceil() + 1.0;
    let (mut x0, mut x1) = (f32::MAX, f32::MIN);
    for l in &lines {
        let lw = filmcraft_text::measure(l, &ts);
        let x = match align {
            CaptionAlign::Left => side + pad_x,
            CaptionAlign::Right => wf - side - pad_x - lw,
            CaptionAlign::Center => (wf - lw) / 2.0,
        };
        x0 = x0.min(x - pad_x - grow);
        x1 = x1.max(x + lw + pad_x + grow);
    }
    Some(TextBox {
        x0: f64::from(x0.floor()),
        y0: f64::from((top - grow).floor()),
        x1: f64::from(x1.ceil()),
        y1: f64::from((top + block_h + grow).ceil()),
        lines: lines.len(),
    })
}

/// A text layer of a graphic clip on a video track, in frame pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipText {
    pub clip: ClipId,
    pub start: Tick,
    pub end: Tick,
    pub text: String,
    pub bounds: TextBox,
}

/// Text layers of the graphic clips on the enabled video tracks in `[a, b)`, measured at each clip's middle.
pub fn text_boxes(s: &Session, q: &Sequence, a: Tick, b: Tick) -> Vec<ClipText> {
    let mut out = Vec::new();
    let items = q.video_tracks.iter().filter(|t| t.enabled).flat_map(|t| t.items.iter()).filter(|i| i.end() > a && i.start < b);
    for it in items.take(MAX_CLIPS) {
        if filmcraft_project::graphic::layer_indices(&it.effects).is_empty() {
            continue;
        }
        let Some(size) = filmcraft_render::source_size(&s.project, it.item) else { continue };
        let mid = it.start + Tick(it.duration.0 / 2);
        let mt = it.source_time_at(mid);
        let m = filmcraft_render::motion_matrix(q, it, size, None, mt);
        for (_, sp) in filmcraft_render::graphic_clip::item_layer_specs(it, mt, size) {
            let LayerContent::Text(t) = &sp.content else { continue };
            if t.text.trim().is_empty() {
                continue;
            }
            let pts = filmcraft_render::graphic_clip::layer_quad(&sp).map(|p| m.apply(p));
            let bx = TextBox {
                x0: pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
                y0: pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
                x1: pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
                y1: pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
                lines: t.text.lines().count(),
            };
            if [bx.x0, bx.y0, bx.x1, bx.y1].iter().all(|v| v.is_finite()) {
                out.push(ClipText { clip: it.id, start: it.start, end: it.end(), text: t.text.clone(), bounds: bx });
            }
        }
    }
    out
}
