//! BashCutPro: the curated look pack (#47).
//!
//! A look is a set of Lumetri Color parameters with a one-line **purpose** ("calms reds so a face reads even:
//! talking head"), genre tags and one **intensity** knob: 1.0 is the look as designed, 0.0 is no change, and every
//! parameter moves from Lumetri's default towards the look's value by the knob (floats within their hard range).
//! Each look has a safe range for the knob; commands clamp to it.
//!
//! Applying adds one Lumetri Color instance to each clip, marked with a `bashcut_look` text parameter (Lumetri
//! ignores it) so applying again replaces the look instead of stacking it; one undo step, like upstream's
//! `lumetri.applyPreset`. Thumbnails are rendered by upstream's Lumetri on its procedural preview picture
//! (`filmcraft_render::lumetri_presets::preview_image`) or on the current program frame. `.cube` files are sampled
//! from the same grade on a lattice, so the pack can be published as LUTs. The pack's values are original work.
//!
//! Checks: [`damage`] (clipped highlights, crushed shadows, skin hue shift, a grey ramp that stays in order) runs in
//! `bashcut.looks.check`; golden values at the minimum, default and maximum intensity are compared in
//! `looks_tests.rs` against `looks_golden.json`, so a look cannot drift silently.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::OnceLock;

use filmcraft_color::{WorkingSpace, linear_to_srgb, srgb_to_linear};
use filmcraft_geom::Vec2;
use filmcraft_project::{ClipId, EffectInstance, Param, ParamKind, ParamValue, find_effect};
use filmcraft_render::Image;
use filmcraft_render::effects::{FxCtx, apply as apply_effect};
use filmcraft_time::Tick;
use serde_json::{Value, json};

use super::data;
use crate::commands::{CommandSpec, always, bad, bool_p, has_seq, str_p};
use crate::{Result, Session};

/// The text parameter that marks our Lumetri instance (`<look id>@<intensity>`).
pub const TAG: &str = "bashcut_look";
/// Largest intensity any look allows.
pub const MAX_AMOUNT: f64 = 1.5;
const MAX_CUBE: usize = 65;

/// One look of the pack.
#[derive(Clone, Debug)]
pub struct Look {
    pub id: &'static str,
    pub name: &'static str,
    /// What it is for, in one line.
    pub purpose: &'static str,
    pub tags: &'static [&'static str],
    /// Lumetri parameters at intensity 1.0 (the rest stay at their defaults).
    pub params: Vec<(&'static str, ParamValue)>,
    /// Safe intensity range; the default is 1.0.
    pub min: f64,
    pub max: f64,
}

fn fl(v: f64) -> ParamValue {
    ParamValue::Float(v)
}
fn rgb(r: f32, g: f32, b: f32) -> ParamValue {
    ParamValue::Color([r, g, b, 1.0])
}
/// A colour-wheel offset towards hue `deg` (0 = red, 120 = green, 240 = blue) of length `len`.
fn wheel(deg: f64, len: f64) -> ParamValue {
    let a = deg.to_radians();
    ParamValue::Vec2(Vec2::new(a.cos() * len, a.sin() * len))
}

fn build() -> Vec<Look> {
    let l = |id, name, purpose, tags, params, min, max| Look { id, name, purpose, tags, params, min, max };
    vec![
        // ---- people: talking head, interview, podcast
        l(
            "clean-skin",
            "Clean Skin",
            "Neutral; calms reds so a face reads even: talking head",
            &["talking-head", "interview", "vlog", "neutral"],
            vec![("temperature", fl(-3.0)), ("tint", fl(-4.0)), ("contrast", fl(6.0)), ("vibrance", fl(12.0)), ("creative_sat", fl(95.0))],
            0.25,
            1.5,
        ),
        l(
            "warm-presenter",
            "Warm Presenter",
            "A touch warmer and brighter so a presenter looks healthy under office light",
            &["talking-head", "tutorial", "vlog"],
            vec![("temperature", fl(12.0)), ("exposure", fl(0.1)), ("shadows", fl(10.0)), ("vibrance", fl(10.0))],
            0.25,
            1.5,
        ),
        l(
            "soft-interview",
            "Soft Interview",
            "Lower contrast and lifted shadows: flattering close-ups for interviews",
            &["interview", "podcast", "talking-head"],
            vec![("contrast", fl(-12.0)), ("shadows", fl(15.0)), ("highlights", fl(-10.0)), ("faded_film", fl(8.0))],
            0.25,
            1.5,
        ),
        l(
            "podcast-punch",
            "Podcast Punch",
            "Crisper contrast and a little colour so a dim studio holds up on phones",
            &["podcast", "talking-head", "shorts"],
            vec![("contrast", fl(18.0)), ("blacks", fl(-8.0)), ("vibrance", fl(18.0)), ("exposure", fl(0.05))],
            0.25,
            1.5,
        ),
        // ---- travel and outdoor
        l(
            "travel-vivid",
            "Travel Vivid",
            "Richer skies and foliage without pushing skin: travel and landscapes",
            &["travel", "outdoor", "landscape"],
            vec![("vibrance", fl(30.0)), ("creative_sat", fl(108.0)), ("highlights", fl(-15.0)), ("contrast", fl(10.0))],
            0.25,
            1.5,
        ),
        l(
            "golden-hour",
            "Golden Hour",
            "Warm low sun for sunsets and lazy afternoons",
            &["travel", "daily", "lifestyle"],
            vec![("temperature", fl(25.0)), ("tint", fl(5.0)), ("highlight_tint", rgb(0.62, 0.54, 0.44)), ("highlights", fl(-10.0)), ("vibrance", fl(12.0))],
            0.25,
            1.5,
        ),
        l(
            "teal-orange",
            "Teal & Orange",
            "Warm skin against cool shadows: cinematic travel and promos",
            &["travel", "promo", "cinematic"],
            vec![("shadow_tint", rgb(0.40, 0.52, 0.58)), ("highlight_tint", rgb(0.62, 0.53, 0.44)), ("contrast", fl(15.0)), ("creative_sat", fl(105.0))],
            0.25,
            1.5,
        ),
        l(
            "clear-sky",
            "Clear Sky",
            "Deeper blue skies and held highlights for bright daylight",
            &["travel", "outdoor", "drone"],
            vec![("highlights", fl(-25.0)), ("whites", fl(-10.0)), ("temperature", fl(-6.0)), ("vibrance", fl(20.0)), ("contrast", fl(8.0))],
            0.25,
            1.5,
        ),
        l(
            "city-night",
            "City Night",
            "Cool shadows, warm practical lights, clean blacks: night streets",
            &["travel", "night", "urban"],
            vec![
                ("temperature", fl(-12.0)),
                ("shadow_tint", rgb(0.42, 0.47, 0.58)),
                ("highlight_tint", rgb(0.60, 0.53, 0.45)),
                ("blacks", fl(-10.0)),
                ("contrast", fl(12.0)),
                ("exposure", fl(0.15)),
            ],
            0.25,
            1.25,
        ),
        // ---- food
        l(
            "fresh-food",
            "Fresh Food",
            "Warm, bright and saturated so food looks appetising",
            &["food", "product"],
            vec![("temperature", fl(10.0)), ("vibrance", fl(25.0)), ("exposure", fl(0.1)), ("contrast", fl(8.0)), ("shadows", fl(8.0))],
            0.25,
            1.5,
        ),
        l(
            "cafe-warm",
            "Café Warm",
            "Cozy brown tones for cafés, coffee and baking",
            &["food", "daily", "lifestyle"],
            vec![("temperature", fl(15.0)), ("faded_film", fl(10.0)), ("highlight_tint", rgb(0.60, 0.54, 0.46)), ("creative_sat", fl(95.0))],
            0.25,
            1.5,
        ),
        // ---- product and ads
        l(
            "product-true",
            "Product True",
            "Neutral balance and clean whites so colours match the real product",
            &["product-review", "product-ad", "ecommerce", "neutral"],
            vec![("whites", fl(8.0)), ("blacks", fl(-5.0)), ("contrast", fl(8.0)), ("vibrance", fl(5.0))],
            0.25,
            1.5,
        ),
        l(
            "product-punch",
            "Product Punch",
            "Bold contrast and colour for ads and thumbnails",
            &["product-ad", "promo", "shorts"],
            vec![("contrast", fl(25.0)), ("vibrance", fl(20.0)), ("creative_sat", fl(110.0)), ("blacks", fl(-10.0))],
            0.25,
            1.25,
        ),
        // ---- tutorials, explainers, news
        l(
            "screen-clean",
            "Screen Clean",
            "Leaves interface colours alone, a little clarity for screen and desk shots",
            &["tutorial", "explainer", "neutral"],
            vec![("contrast", fl(6.0)), ("whites", fl(6.0)), ("blacks", fl(-4.0))],
            0.25,
            1.5,
        ),
        l(
            "news-neutral",
            "News Neutral",
            "Faithful colour with safe whites for news and documentary",
            &["news", "explainer", "documentary", "neutral"],
            vec![("whites", fl(-6.0)), ("highlights", fl(-8.0)), ("contrast", fl(4.0))],
            0.25,
            1.5,
        ),
        l(
            "archive-warm",
            "Archive Warm",
            "Old-photo warmth that makes mixed archive pictures feel like one set",
            &["news", "documentary", "story"],
            vec![("temperature", fl(18.0)), ("faded_film", fl(20.0)), ("saturation", fl(75.0))],
            0.25,
            1.25,
        ),
        // ---- daily and lifestyle
        l(
            "soft-pastel",
            "Soft Pastel",
            "Lifted blacks and gentle colour for calm daily vlogs",
            &["daily", "lifestyle", "aesthetic"],
            vec![("faded_film", fl(25.0)), ("contrast", fl(-10.0)), ("creative_sat", fl(85.0)), ("highlight_tint", rgb(0.55, 0.50, 0.52))],
            0.25,
            1.25,
        ),
        l(
            "morning-light",
            "Morning Light",
            "Airy and slightly cool: routines and study-with-me",
            &["daily", "lifestyle", "study"],
            vec![("exposure", fl(0.2)), ("temperature", fl(-5.0)), ("shadows", fl(12.0)), ("contrast", fl(-6.0)), ("creative_sat", fl(92.0))],
            0.25,
            1.25,
        ),
        l(
            "film-matte",
            "Film Matte",
            "Faded matte print with warm mids for nostalgic stories",
            &["daily", "travel", "story", "cinematic"],
            vec![("faded_film", fl(35.0)), ("contrast", fl(-8.0)), ("wheel_midtones", wheel(35.0, 0.08)), ("creative_sat", fl(90.0))],
            0.25,
            1.25,
        ),
        // ---- cinematic, promo, sport
        l(
            "moody-green",
            "Moody Green",
            "Desaturated with green-cyan shadows for dramatic or rainy scenes",
            &["cinematic", "story", "promo"],
            vec![("shadow_tint", rgb(0.43, 0.52, 0.48)), ("creative_sat", fl(80.0)), ("contrast", fl(15.0)), ("exposure", fl(-0.15))],
            0.25,
            1.25,
        ),
        l(
            "silver-grit",
            "Silver Grit",
            "Low colour and hard contrast for gritty sport and action",
            &["sport", "action", "promo"],
            vec![("saturation", fl(60.0)), ("contrast", fl(35.0)), ("blacks", fl(-10.0)), ("highlights", fl(-10.0))],
            0.25,
            1.0,
        ),
        l(
            "punchy-shorts",
            "Punchy Shorts",
            "Bright, contrasty and colourful so a phone feed stops scrolling",
            &["shorts", "tiktok", "reels", "promo"],
            vec![("contrast", fl(20.0)), ("vibrance", fl(30.0)), ("exposure", fl(0.1)), ("blacks", fl(-6.0))],
            0.25,
            1.25,
        ),
        // ---- monochrome
        l(
            "mono-clean",
            "Clean Mono",
            "Neutral black and white with the full tonal range",
            &["mono", "story", "art"],
            vec![("saturation", fl(0.0)), ("contrast", fl(10.0))],
            0.5,
            1.0,
        ),
        l(
            "mono-warm",
            "Warm Mono",
            "Warm-toned black and white for portraits",
            &["mono", "portrait", "story"],
            vec![("saturation", fl(0.0)), ("wheel_midtones", wheel(35.0, 0.12)), ("contrast", fl(8.0))],
            0.5,
            1.0,
        ),
        // ---- fixes
        l(
            "fix-tungsten",
            "Fix Orange Indoor Light",
            "Cools down warm tungsten light so whites read white",
            &["fix", "indoor"],
            vec![("temperature", fl(-30.0)), ("tint", fl(4.0))],
            0.25,
            1.5,
        ),
        l(
            "fix-grey-day",
            "Fix Grey Day",
            "Warms and adds life to flat overcast footage",
            &["fix", "outdoor"],
            vec![("temperature", fl(12.0)), ("vibrance", fl(25.0)), ("contrast", fl(12.0)), ("blacks", fl(-6.0))],
            0.25,
            1.5,
        ),
    ]
}

/// Every look of the pack, in listing order.
pub fn pack() -> &'static [Look] {
    static PACK: OnceLock<Vec<Look>> = OnceLock::new();
    PACK.get_or_init(build)
}

/// The look with this id (or name), ignoring case.
pub fn find(id: &str) -> Option<&'static Look> {
    let id = id.trim();
    pack().iter().find(|l| l.id.eq_ignore_ascii_case(id) || l.name.eq_ignore_ascii_case(id))
}

impl Look {
    /// `amount` within this look's safe range (NaN or missing = 1.0).
    pub fn clamp(&self, amount: Option<f64>) -> f64 {
        let a = amount.filter(|a| a.is_finite()).unwrap_or(1.0);
        a.clamp(self.min.min(1.0), self.max.clamp(1.0, MAX_AMOUNT))
    }

    /// A stable key of the look's values at `amount` (thumbnail cache, golden file).
    pub fn fingerprint(&self, amount: f64) -> String {
        let mut h = DefaultHasher::new();
        self.id.hash(&mut h);
        serde_json::to_string(&self.params.iter().map(|(k, v)| (k, v)).collect::<Vec<_>>()).unwrap_or_default().hash(&mut h);
        ((amount * 1000.0).round() as i64).hash(&mut h);
        format!("{:016x}", h.finish())
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "purpose": self.purpose,
            "tags": self.tags,
            "intensity": {"min": self.min, "default": 1.0, "max": self.max},
            "fingerprint": self.fingerprint(1.0),
        })
    }
}

fn lumetri() -> EffectInstance {
    match find_effect("lumetri") {
        Some(def) => def.instance(),
        None => EffectInstance {
            effect: "lumetri".into(),
            enabled: true,
            params: Default::default(),
            masks: Vec::new(),
            post_fader: false,
            essential: false,
            layer: None,
        },
    }
}

/// `d` moved towards `v` by `a` (floats kept in the parameter's hard range).
fn mix(key: &str, d: &ParamValue, v: &ParamValue, a: f64) -> ParamValue {
    let af = a as f32;
    match (d, v) {
        (ParamValue::Float(x), ParamValue::Float(y)) => {
            let m = x + (y - x) * a;
            match find_effect("lumetri").and_then(|e| e.param(key)).map(|p| &p.kind) {
                Some(ParamKind::Float { min, max, .. }) if min <= max => ParamValue::Float(m.clamp(*min, *max)),
                _ => ParamValue::Float(m),
            }
        }
        (ParamValue::Color(x), ParamValue::Color(y)) => ParamValue::Color(std::array::from_fn(|i| (x[i] + (y[i] - x[i]) * af).clamp(0.0, 1.0))),
        (ParamValue::Vec2(x), ParamValue::Vec2(y)) => {
            ParamValue::Vec2(Vec2::new((x.x + (y.x - x.x) * a).clamp(-1.0, 1.0), (x.y + (y.y - x.y) * a).clamp(-1.0, 1.0)))
        }
        // tone curves: towards the identity line
        (_, ParamValue::Curve(pts)) => ParamValue::Curve(pts.iter().map(|[px, py]| [*px, (px + (py - px) * af).clamp(0.0, 1.0)]).collect()),
        _ if a > 0.0 => v.clone(),
        _ => d.clone(),
    }
}

/// The Lumetri Color instance of `look` at `amount` (not clamped to the safe range; see [`Look::clamp`]).
pub fn instance(look: &Look, amount: f64) -> EffectInstance {
    let a = if amount.is_finite() { amount.clamp(0.0, MAX_AMOUNT) } else { 1.0 };
    let mut e = lumetri();
    for (k, v) in &look.params {
        if let Some(p) = e.params.get_mut(*k) {
            p.value = mix(k, &p.value, v, a);
        }
    }
    e.params.insert(TAG.to_string(), Param::new(ParamValue::Text(format!("{}@{a:.2}", look.id))));
    e
}

/// Whether `e` is a look we applied.
pub fn is_look(e: &EffectInstance) -> bool {
    e.effect == "lumetri" && e.params.contains_key(TAG)
}

/// The look id and intensity of one of our instances.
pub fn look_of(e: &EffectInstance) -> Option<(String, f64)> {
    let ParamValue::Text(t) = &e.params.get(TAG)?.value else { return None };
    let (id, a) = t.split_once('@')?;
    Some((id.to_string(), a.parse().ok()?))
}

/// Grade `img` (linear, premultiplied) with `look` at `amount`, as Lumetri renders it in a Rec. 709 sequence.
pub fn grade(img: &mut Image, look: &Look, amount: f64) {
    let cx = FxCtx {
        t: Tick::ZERO,
        px_scale: img.w as f32 / 1920.0,
        seconds: 0.0,
        timecode: "",
        clip_name: "",
        project: None,
        env: None,
        frame_rate: 24.0,
        working: WorkingSpace::Rec709,
    };
    apply_effect(img, &instance(look, amount), &cx);
}

/// A thumbnail: `base` (or upstream's procedural preview picture at `w`×`h`) graded by the look.
pub fn thumbnail(look: &Look, amount: f64, base: Option<&Image>, w: usize, h: usize) -> Image {
    let mut img = match base {
        Some(b) => b.clone(),
        None => filmcraft_render::lumetri_presets::preview_image(w.clamp(8, 1920), h.clamp(8, 1080)),
    };
    grade(&mut img, look, amount);
    img
}

/// The program frame at the playhead, about `w` pixels wide (for previews on the current frame).
pub fn program_frame(s: &Session, w: usize) -> Option<Image> {
    let full = s.active_sequence().map(|q| q.settings.width as f32).filter(|x| *x > 0.0).unwrap_or(1920.0);
    let scale = (w.clamp(16, 1920) as f32 / full).clamp(0.01, 1.0);
    s.render_program(scale).filter(|i| i.w > 0 && i.h > 0)
}

// ---------------------------------------------------------------- reference patches and checks

/// Reference colours (display sRGB): a grey ramp, three skin tones, then saturated colours.
pub const GREYS: [f32; 12] = [0.02, 0.05, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95];
pub const SKIN: [[f32; 3]; 3] = [[0.92, 0.75, 0.65], [0.78, 0.57, 0.44], [0.45, 0.30, 0.22]];
pub const COLOURS: [[f32; 3]; 6] = [[0.80, 0.15, 0.15], [0.20, 0.65, 0.25], [0.15, 0.30, 0.80], [0.90, 0.85, 0.20], [0.45, 0.65, 0.90], [0.30, 0.45, 0.20]];

/// Every reference colour, in order: greys, skin, colours.
pub fn patches() -> Vec<[f32; 3]> {
    GREYS.iter().map(|g| [*g; 3]).chain(SKIN).chain(COLOURS).collect()
}

/// The reference colours graded by `look` at `amount`, in display sRGB.
pub fn graded_patches(look: &Look, amount: f64) -> Vec<[f32; 3]> {
    let src = patches();
    let mut img = Image::new(src.len(), 1);
    for (i, c) in src.iter().enumerate() {
        if let Some(px) = img.px.get_mut(i * 4..i * 4 + 4) {
            px.copy_from_slice(&[srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), 1.0]);
        }
    }
    grade(&mut img, look, amount);
    img.px.as_chunks::<4>().0.iter().map(|[r, g, b, _]| [enc(*r), enc(*g), enc(*b)]).collect()
}

fn enc(v: f32) -> f32 {
    if v.is_finite() { linear_to_srgb(v.max(0.0)) } else { 0.0 }
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Hue (degrees) and saturation (0..1, HSV) of a display colour.
fn hue_sat(c: [f32; 3]) -> (f32, f32) {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let d = max - min;
    let sat = if max > 0.0 { d / max } else { 0.0 };
    if d <= f32::EPSILON {
        return (0.0, sat);
    }
    let h = if max == c[0] {
        60.0 * ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if max == c[1] {
        60.0 * ((c[2] - c[0]) / d + 2.0)
    } else {
        60.0 * ((c[0] - c[1]) / d + 4.0)
    };
    (h, sat)
}

/// Thresholds of the damage check.
pub const CLIP_LEVEL: f32 = 0.995;
pub const CRUSH_LEVEL: f32 = 0.02;
pub const MAX_SKIN_SHIFT: f32 = 15.0;

/// What a look does to the reference colours at `amount`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Damage {
    /// The 0.9 grey reaches white in some channel.
    pub clipped_highlights: bool,
    /// The 0.1 grey falls to black.
    pub crushed_shadows: bool,
    /// The grey ramp's lightness goes down somewhere.
    pub ramp_inverted: bool,
    /// Largest hue change of a skin tone (degrees; 0 for monochrome results).
    pub skin_hue_shift: f32,
    pub ok: bool,
}

pub fn damage(look: &Look, amount: f64) -> Damage {
    let out = graded_patches(look, amount);
    let grey = |i: usize| out.get(i).copied().unwrap_or([0.0; 3]);
    let at = |level: f32| GREYS.iter().position(|g| (*g - level).abs() < 1e-6).map(grey).unwrap_or([0.0; 3]);
    let clipped_highlights = at(0.9).iter().any(|c| *c >= CLIP_LEVEL);
    let crushed_shadows = luma(at(0.1)) <= CRUSH_LEVEL;
    let ramp: Vec<f32> = (0..GREYS.len()).map(|i| luma(grey(i))).collect();
    let ramp_inverted = ramp.windows(2).any(|w| matches!(w, [a, b] if *b + 0.002 < *a));
    let mut skin_hue_shift = 0.0f32;
    for (i, src) in SKIN.iter().enumerate() {
        let Some(o) = out.get(GREYS.len() + i) else { continue };
        let (h0, _) = hue_sat(*src);
        let (h1, s1) = hue_sat(*o);
        if s1 < 0.08 {
            continue; // monochrome: no hue to keep
        }
        let d = (h1 - h0).abs() % 360.0;
        skin_hue_shift = skin_hue_shift.max(d.min(360.0 - d));
    }
    let ok = !clipped_highlights && !crushed_shadows && !ramp_inverted && skin_hue_shift <= MAX_SKIN_SHIFT;
    Damage { clipped_highlights, crushed_shadows, ramp_inverted, skin_hue_shift, ok }
}

// ---------------------------------------------------------------- .cube

/// The look as a 3D LUT (`.cube` text, display sRGB in and out), sampled on a `size`³ lattice.
pub fn cube(look: &Look, amount: f64, size: usize) -> String {
    let n = size.clamp(2, MAX_CUBE);
    let mut img = Image::new(n * n * n, 1);
    let step = (n - 1) as f32;
    let mut i = 0;
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                if let Some(px) = img.px.get_mut(i * 4..i * 4 + 4) {
                    px.copy_from_slice(&[srgb_to_linear(r as f32 / step), srgb_to_linear(g as f32 / step), srgb_to_linear(b as f32 / step), 1.0]);
                }
                i += 1;
            }
        }
    }
    grade(&mut img, look, amount);
    let mut out = format!("TITLE \"BashCutPro {}\"\n# {}\nLUT_3D_SIZE {n}\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n", look.name, look.purpose);
    for [r, g, b, _] in img.px.as_chunks::<4>().0 {
        out.push_str(&format!("{:.6} {:.6} {:.6}\n", enc(*r).min(1.0), enc(*g).min(1.0), enc(*b).min(1.0)));
    }
    out
}

// ---------------------------------------------------------------- commands

fn look_p(p: &Value, cmd: &str) -> Result<&'static Look> {
    let id = str_p(p, "look").ok_or_else(|| bad(cmd, "need `look` (an id from bashcut.looks.list)"))?;
    find(id).ok_or_else(|| bad(cmd, format!("no look `{id}`")))
}

fn amount_p(p: &Value, look: &Look) -> f64 {
    look.clamp(p.get("intensity").and_then(Value::as_f64))
}

fn tags_p(p: &Value) -> Vec<String> {
    match p.get("tags") {
        Some(Value::String(t)) => vec![t.to_lowercase()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_lowercase).collect(),
        _ => Vec::new(),
    }
}

fn list(_: &mut Session, p: &Value) -> Result<Value> {
    let tags = tags_p(p);
    let q = str_p(p, "query").map(str::to_lowercase).unwrap_or_default();
    let looks: Vec<Value> = pack()
        .iter()
        .filter(|l| tags.is_empty() || l.tags.iter().any(|t| tags.iter().any(|w| w == t)))
        .filter(|l| {
            q.is_empty() || l.name.to_lowercase().contains(&q) || l.purpose.to_lowercase().contains(&q) || l.tags.iter().any(|t| t.contains(q.as_str()))
        })
        .map(Look::to_json)
        .collect();
    let mut tags_all: Vec<&str> = pack().iter().flat_map(|l| l.tags.iter().copied()).collect();
    tags_all.sort_unstable();
    tags_all.dedup();
    Ok(json!({"looks": looks, "tags": tags_all}))
}

fn get(_: &mut Session, p: &Value) -> Result<Value> {
    let look = look_p(p, "bashcut.looks.get")?;
    let amount = amount_p(p, look);
    let mut v = look.to_json();
    v["params"] = json!(
        instance(look, amount)
            .params
            .iter()
            .filter(|(k, _)| look.params.iter().any(|(lk, _)| lk == k))
            .map(|(k, p)| (k.clone(), json!(p.value)))
            .collect::<serde_json::Map<_, _>>()
    );
    v["damage"] = json!(damage(look, amount));
    Ok(v)
}

/// The video clips among `clips` that carry one of our looks, with that look.
fn applied(s: &Session, clips: &[ClipId]) -> Vec<Value> {
    let Some(q) = s.active_sequence() else { return Vec::new() };
    q.video_tracks
        .iter()
        .flat_map(|t| t.items.iter())
        .filter(|i| clips.is_empty() || clips.contains(&i.id))
        .filter_map(|i| i.effects.iter().find(|e| is_look(e)).and_then(look_of).map(|(id, a)| json!({"clip": i.id.0, "look": id, "intensity": a})))
        .collect()
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "bashcut.looks.apply";
    let look = look_p(p, CMD)?;
    let amount = amount_p(p, look);
    let clips = crate::commands::clips_p(s, p);
    if clips.is_empty() {
        return Err(bad(CMD, "select clips first"));
    }
    let inst = instance(look, amount);
    let n = s.edit_sequence(&format!("Apply Look {}", look.name), |q, _, _| {
        let mut n = 0;
        for t in q.video_tracks.iter_mut() {
            for it in t.items.iter_mut().filter(|i| clips.contains(&i.id)) {
                match it.effects.iter().position(is_look) {
                    Some(i) => {
                        if let Some(e) = it.effects.get_mut(i) {
                            *e = inst.clone();
                        }
                    }
                    None => {
                        let pos = it.effects.iter().position(|e| e.def().is_some_and(|d| d.intrinsic)).unwrap_or(it.effects.len());
                        it.effects.insert(pos, inst.clone());
                    }
                }
                n += 1;
            }
        }
        Ok(n)
    })?;
    if n == 0 {
        return Err(bad(CMD, "no video clips among the clips"));
    }
    Ok(json!({"look": look.id, "name": look.name, "intensity": amount, "clips": n}))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "bashcut.looks.remove";
    let clips = crate::commands::clips_p(s, p);
    if clips.is_empty() {
        return Err(bad(CMD, "select clips first"));
    }
    if applied(s, &clips).is_empty() {
        return Ok(json!({"clips": 0}));
    }
    let n = s.edit_sequence("Remove Look", |q, _, _| {
        let mut n = 0;
        for t in q.video_tracks.iter_mut() {
            for it in t.items.iter_mut().filter(|i| clips.contains(&i.id)) {
                let before = it.effects.len();
                it.effects.retain(|e| !is_look(e));
                n += usize::from(it.effects.len() != before);
            }
        }
        Ok(n)
    })?;
    Ok(json!({"clips": n}))
}

fn thumb(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "bashcut.looks.thumbnail";
    let look = look_p(p, CMD)?;
    let amount = amount_p(p, look);
    let w = p.get("width").and_then(Value::as_u64).unwrap_or(160).clamp(16, 1920) as usize;
    let h = (w * 9 / 16).max(9);
    let on_frame = bool_p(p, "frame").unwrap_or(false);
    let fp = look.fingerprint(amount);
    // the picture thumbnail is cached in the data dir by fingerprint; a frame preview never is
    let cached = if on_frame { None } else { data::folder(s, p, "looks").map(|d| d.join("thumbs").join(format!("{}-{fp}-{w}.png", look.id))) };
    let path: Option<PathBuf> = str_p(p, "path").filter(|x| !x.is_empty()).map(PathBuf::from).or(cached.clone());
    let mut out = json!({"look": look.id, "intensity": amount, "fingerprint": fp, "frame": on_frame});
    if path.is_some() && path == cached && cached.as_ref().is_some_and(|c| c.is_file()) {
        out["path"] = json!(cached.map(|c| c.to_string_lossy().into_owned()));
        out["cached"] = json!(true);
        return Ok(out);
    }
    let img = if on_frame {
        let base = program_frame(s, w).ok_or_else(|| bad(CMD, "no program frame to preview on"))?;
        thumbnail(look, amount, Some(&base), w, h)
    } else {
        thumbnail(look, amount, None, w, h)
    };
    out["width"] = json!(img.w);
    out["height"] = json!(img.h);
    if let Some(path) = path {
        let png = filmcraft_export::encode_png(img.over_black_rgba8(), img.w as u32, img.h as u32).map_err(|e| bad(CMD, e.to_string()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| bad(CMD, format!("{}: {e}", dir.display())))?;
        }
        std::fs::write(&path, png).map_err(|e| bad(CMD, format!("{}: {e}", path.display())))?;
        out["path"] = json!(path.to_string_lossy());
    }
    Ok(out)
}

fn export(_: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "bashcut.looks.export";
    let size = p.get("size").and_then(Value::as_u64).unwrap_or(33).clamp(2, MAX_CUBE as u64) as usize;
    let write = |path: &std::path::Path, text: &str| -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| bad(CMD, format!("{}: {e}", dir.display())))?;
        }
        std::fs::write(path, text).map_err(|e| bad(CMD, format!("{}: {e}", path.display())))
    };
    if let Some(id) = str_p(p, "look") {
        let look = find(id).ok_or_else(|| bad(CMD, format!("no look `{id}`")))?;
        let path = str_p(p, "path").filter(|x| !x.is_empty()).ok_or_else(|| bad(CMD, "need `path` (a .cube file) with `look`"))?;
        let amount = amount_p(p, look);
        write(std::path::Path::new(path), &cube(look, amount, size))?;
        return Ok(json!({"look": look.id, "intensity": amount, "path": path, "size": size}));
    }
    let dir = str_p(p, "dir").filter(|x| !x.is_empty()).ok_or_else(|| bad(CMD, "need `look` + `path`, or `dir` for the whole pack"))?;
    let dir = PathBuf::from(dir);
    let mut index = Vec::new();
    for look in pack() {
        let file = format!("{}.cube", look.id);
        write(&dir.join(&file), &cube(look, 1.0, size))?;
        let mut v = look.to_json();
        v["cube"] = json!(file);
        index.push(v);
    }
    let doc = json!({"schema": "bashcut-looks/1", "size": size, "looks": index});
    data::write_json(&dir.join("looks.json"), &doc).map_err(|e| bad(CMD, e))?;
    Ok(json!({"dir": dir.to_string_lossy(), "looks": pack().len(), "size": size}))
}

fn check(_: &mut Session, p: &Value) -> Result<Value> {
    let looks: Vec<&Look> = match str_p(p, "look") {
        Some(id) => vec![find(id).ok_or_else(|| bad("bashcut.looks.check", format!("no look `{id}`")))?],
        None => pack().iter().collect(),
    };
    let rows: Vec<Value> = looks.iter().map(|l| json!({"look": l.id, "min": damage(l, l.min), "default": damage(l, 1.0), "max": damage(l, l.max)})).collect();
    let ok = rows.iter().all(|r| r["default"]["ok"] == json!(true));
    Ok(json!({"ok": ok, "looks": rows}))
}

/// A read-only look command, always enabled.
macro_rules! query {
    ($id:expr, $label:expr, $params:expr, $run:expr $(,)?) => {
        spec! { id: $id, label: $label, params: $params, enabled: always, run: $run, journal: false }
    };
}

pub fn commands() -> Vec<CommandSpec> {
    vec![
        query!("bashcut.looks.list", "List Looks", r#"{"tags":[str]|str? (any of; e.g. "talking-head","travel","food"),"query":str?}"#, list),
        query!("bashcut.looks.get", "Get Look", r#"{"look":str,"intensity":0..1.5=1?}"#, get),
        spec! {
            id: "bashcut.looks.apply",
            label: "Apply Look",
            params: r#"{"look":str,"intensity":0..1.5=1? (clamped to the look's safe range),"clips":[id]? (default the selection)}"#,
            enabled: has_seq,
            run: apply,
            journal: true,
        },
        spec! {
            id: "bashcut.looks.remove",
            label: "Remove Look",
            params: r#"{"clips":[id]? (default the selection)}"#,
            enabled: has_seq,
            run: remove,
            journal: true,
        },
        query!(
            "bashcut.looks.thumbnail",
            "Look Thumbnail",
            r#"{"look":str,"intensity":0..1.5=1?,"width":n=160,"frame":bool=false (on the program frame at the playhead),"path":str? (PNG; default a cache file in the data dir)}"#,
            thumb,
        ),
        query!(
            "bashcut.looks.export",
            "Export Looks as LUTs",
            r#"{"look":str?,"path":str? (.cube, with look),"dir":str? (the whole pack + looks.json),"intensity":0..1.5=1?,"size":2..65=33}"#,
            export,
        ),
        query!(
            "bashcut.looks.check",
            "Check Looks",
            r#"{"look":str? (default every look)}  (clipped highlights, crushed shadows, skin hue shift at min / default / max)"#,
            check,
        ),
    ]
}
