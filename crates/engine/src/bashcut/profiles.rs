//! BashCutPro: output profiles, platform-correct exports by default (#43).
//!
//! A profile is what a platform wants (frame size and rate, codec, bitrate from its upload guidance, loudness and true
//! peak, captions burned or as a sidecar, safe-area insets for review). Built-ins cover TikTok, Reels, Shorts,
//! YouTube 16:9, YouTube 4K and a Facebook square; files in `<data dir>/bashcut/profiles/*.json` add or replace them
//! by `id`.
//!
//! A profile **compiles to an ordinary FilmCraft export preset** through `export.presets.save` (a user preset, named
//! `BashCut <name>`, persisted in the data dir by upstream's own library), so `filmcraft-cli export --preset`, the
//! export queue and Quick Export all use it. Nothing in upstream's export code or preset tables changes.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::data;
use crate::commands::{CommandSpec, always, bad, f64_p, str_p, u64_p};
use crate::{EngineError, Result, Session};

pub const SCHEMA: &str = "bashcut-profile/1";
const CATEGORY: &str = "BashCutPro";
const MAX_FILES: usize = 256;

/// Safe-area insets as fractions of the frame (0.1 = 10 % of the width or height).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SafeArea {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// `h264` | `hevc` | `av1`.
    pub codec: String,
    /// 8 or 10; only scales the bitrate guidance.
    pub bit_depth: u8,
    /// Video bitrate override; None = from the platform guidance ([`Profile::video_kbps`]).
    pub bitrate_kbps: Option<u32>,
    pub audio_kbps: u32,
    pub sample_rate: u32,
    /// Integrated loudness target (LUFS) for export loudness normalization.
    pub loudness_lufs: f64,
    pub true_peak_dbtp: f64,
    /// `burn` | `srt` | `vtt` | `none`.
    pub captions: String,
    pub safe_area: SafeArea,
    pub notes: String,
    #[serde(skip_deserializing)]
    pub builtin: bool,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            id: String::new(),
            name: String::new(),
            platform: String::new(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            codec: "h264".into(),
            bit_depth: 8,
            bitrate_kbps: None,
            audio_kbps: 320,
            sample_rate: 48_000,
            loudness_lufs: -14.0,
            true_peak_dbtp: -1.0,
            captions: "srt".into(),
            safe_area: SafeArea::default(),
            notes: String::new(),
            builtin: false,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn builtin(id: &str, name: &str, platform: &str, (width, height): (u32, u32), lufs: f64, captions: &str, safe_area: SafeArea, notes: &str) -> Profile {
    Profile {
        id: id.into(),
        name: name.into(),
        platform: platform.into(),
        width,
        height,
        loudness_lufs: lufs,
        captions: captions.into(),
        safe_area,
        notes: notes.into(),
        builtin: true,
        ..Profile::default()
    }
}

/// The built-in profiles. Safe areas leave room for each app's buttons and caption bar.
pub fn builtins() -> Vec<Profile> {
    let vertical = |top, bottom, right| SafeArea { top, bottom, left: 0.06, right };
    vec![
        builtin("tiktok", "TikTok", "TikTok", (1080, 1920), -14.0, "burn", vertical(0.08, 0.20, 0.12), "9:16, 1080×1920 30 fps"),
        builtin("reels", "Reels", "Instagram", (1080, 1920), -14.0, "burn", vertical(0.14, 0.20, 0.10), "9:16, 1080×1920 30 fps"),
        builtin("shorts", "Shorts", "YouTube", (1080, 1920), -14.0, "burn", vertical(0.10, 0.20, 0.12), "9:16, 1080×1920 30 fps"),
        builtin(
            "youtube",
            "YouTube 16:9",
            "YouTube",
            (1920, 1080),
            -14.0,
            "srt",
            SafeArea { top: 0.05, bottom: 0.05, left: 0.05, right: 0.05 },
            "16:9 1080p, captions as an SRT sidecar",
        ),
        builtin(
            "youtube-4k",
            "YouTube 4K",
            "YouTube",
            (3840, 2160),
            -14.0,
            "srt",
            SafeArea { top: 0.05, bottom: 0.05, left: 0.05, right: 0.05 },
            "16:9 2160p, captions as an SRT sidecar",
        ),
        builtin(
            "facebook-square",
            "Facebook Square",
            "Facebook",
            (1080, 1080),
            -16.0,
            "burn",
            SafeArea { top: 0.05, bottom: 0.15, left: 0.05, right: 0.05 },
            "1:1, 1080×1080 30 fps",
        ),
    ]
}

impl Profile {
    /// Check a profile read from a file or params; every number is treated as hostile.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !data::plain_name(&self.id) {
            return Err(format!("profile id `{}` must be a plain name", self.id));
        }
        if !(16..=8192).contains(&self.width) || !(16..=8192).contains(&self.height) {
            return Err(format!("{}: frame size {}×{} is out of range", self.id, self.width, self.height));
        }
        if !self.fps.is_finite() || !(1.0..=240.0).contains(&self.fps) {
            return Err(format!("{}: fps must be 1–240", self.id));
        }
        if !matches!(self.codec.as_str(), "h264" | "hevc" | "av1") {
            return Err(format!("{}: codec must be h264, hevc or av1", self.id));
        }
        if !matches!(self.bit_depth, 8 | 10) {
            return Err(format!("{}: bitDepth must be 8 or 10", self.id));
        }
        if self.bitrate_kbps.is_some_and(|b| !(100..=400_000).contains(&b)) {
            return Err(format!("{}: bitrateKbps must be 100–400000", self.id));
        }
        if !(32..=512).contains(&self.audio_kbps) || !(8_000..=192_000).contains(&self.sample_rate) {
            return Err(format!("{}: audio bitrate or sample rate out of range", self.id));
        }
        if !self.loudness_lufs.is_finite() || !(-40.0..=-5.0).contains(&self.loudness_lufs) {
            return Err(format!("{}: loudnessLufs must be -40 to -5", self.id));
        }
        if !self.true_peak_dbtp.is_finite() || !(-9.0..=0.0).contains(&self.true_peak_dbtp) {
            return Err(format!("{}: truePeakDbtp must be -9 to 0", self.id));
        }
        if !matches!(self.captions.as_str(), "burn" | "srt" | "vtt" | "none") {
            return Err(format!("{}: captions must be burn, srt, vtt or none", self.id));
        }
        let a = self.safe_area;
        if [a.top, a.bottom, a.left, a.right].iter().any(|v| !v.is_finite() || !(0.0..=0.45).contains(v)) {
            return Err(format!("{}: safe-area insets must be 0–0.45", self.id));
        }
        Ok(())
    }

    /// The export preset this profile compiles to.
    pub fn preset_name(&self) -> String {
        format!("BashCut {}", self.name)
    }

    /// Video bitrate from platform upload guidance: 8 Mbps at 1080p30 (5 at 720p, 16 at 1440p, 40 at 2160p),
    /// ×1.5 above 30 fps, ×0.7 for HEVC, ×0.6 for AV1, ×1.25 at 10-bit; rounded to 100 kbps.
    pub fn video_kbps(&self) -> u32 {
        if let Some(b) = self.bitrate_kbps {
            return b;
        }
        let short = self.width.min(self.height);
        let mut kbps: f64 = match short {
            0..=480 => 2_500.0,
            481..=720 => 5_000.0,
            721..=1080 => 8_000.0,
            1081..=1440 => 16_000.0,
            1441..=2160 => 40_000.0,
            _ => 80_000.0,
        };
        if self.fps > 30.5 {
            kbps *= 1.5;
        }
        kbps *= match self.codec.as_str() {
            "hevc" => 0.7,
            "av1" => 0.6,
            _ => 1.0,
        };
        if self.bit_depth >= 10 {
            kbps *= 1.25;
        }
        ((kbps / 100.0).round() * 100.0) as u32
    }

    /// Estimated file size in bytes for `seconds` of output (video + audio bitrate; container overhead ignored).
    pub fn estimate_bytes(&self, seconds: f64) -> u64 {
        if !seconds.is_finite() || seconds <= 0.0 {
            return 0;
        }
        let kbps = f64::from(self.video_kbps()) + f64::from(self.audio_kbps);
        (kbps * 1000.0 / 8.0 * seconds.min(1.0e7)) as u64
    }

    /// Safe-area insets in pixels `{top, bottom, left, right}` of the output frame.
    pub fn safe_area_px(&self) -> Value {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let a = self.safe_area;
        json!({
            "top": (a.top * h).round(), "bottom": (a.bottom * h).round(),
            "left": (a.left * w).round(), "right": (a.right * w).round(),
        })
    }

    /// `export.presets.save` params for this profile.
    pub fn preset_params(&self) -> Value {
        let sidecar = match self.captions.as_str() {
            "srt" | "vtt" => self.captions.as_str(),
            _ => "none",
        };
        json!({
            "name": self.preset_name(),
            "category": CATEGORY,
            "description": format!("{} ({}): {}", self.name, self.platform, self.notes),
            "overwrite": true,
            "format": self.codec,
            "width": self.width,
            "height": self.height,
            "fps": self.fps,
            "bitrateKbps": self.video_kbps(),
            "audioCodec": "aac",
            "burnCaptions": self.captions == "burn",
            "captionSidecar": sidecar,
            "loudnessLufs": self.loudness_lufs,
            "settings": {
                "audio": {"bitrateKbps": self.audio_kbps, "sampleRate": self.sample_rate},
                "effects": {"loudness": {"true_peak_dbtp": self.true_peak_dbtp}},
            },
        })
    }

    pub fn to_json(&self, seconds: Option<f64>) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        v["builtin"] = json!(self.builtin);
        v["videoKbps"] = json!(self.video_kbps());
        v["preset"] = json!(self.preset_name());
        v["safeAreaPx"] = self.safe_area_px();
        v["bytesPerMinute"] = json!(self.estimate_bytes(60.0));
        if let Some(secs) = seconds {
            v["estimatedBytes"] = json!(self.estimate_bytes(secs));
        }
        v
    }
}

/// Built-ins plus the user's files (a file with a built-in's id replaces it). Bad files are skipped with a warning.
pub fn all(s: &Session, p: &Value) -> Vec<Profile> {
    let mut v = builtins();
    let Some(dir) = data::folder(s, p, "profiles") else { return v };
    let Ok(rd) = std::fs::read_dir(&dir) else { return v };
    let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|x| x == "json")).take(MAX_FILES).collect();
    files.sort();
    for f in files {
        let parsed = data::read_json(&f, 256 << 10).and_then(|j| serde_json::from_value::<Profile>(j).map_err(|e| format!("{}: {e}", f.display())));
        let mut pr = match parsed {
            Ok(pr) => pr,
            Err(e) => {
                log::warn!("output profile skipped: {e}");
                continue;
            }
        };
        if pr.id.is_empty() {
            pr.id = f.file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
        }
        if pr.name.is_empty() {
            pr.name = pr.id.clone();
        }
        if let Err(e) = pr.validate() {
            log::warn!("output profile {} skipped: {e}", f.display());
            continue;
        }
        match v.iter_mut().find(|x| x.id == pr.id) {
            Some(slot) => *slot = pr,
            None => v.push(pr),
        }
    }
    v
}

/// A profile by id or name (case-insensitive).
pub fn find(s: &Session, p: &Value, key: &str) -> Option<Profile> {
    let k = key.trim().to_ascii_lowercase();
    all(s, p).into_iter().find(|x| x.id.to_ascii_lowercase() == k || x.name.to_ascii_lowercase() == k)
}

fn profile_p(s: &Session, p: &Value, cmd: &str) -> Result<Profile> {
    let key = str_p(p, "profile").or_else(|| str_p(p, "id")).ok_or_else(|| bad(cmd, "need `profile` (see bashcut.profiles.list)"))?;
    find(s, p, key).ok_or_else(|| bad(cmd, format!("no output profile `{key}` (see bashcut.profiles.list)")))
}

/// Seconds to estimate for: `seconds`, else the active sequence's duration.
fn seconds_p(s: &Session, p: &Value) -> Option<f64> {
    if let Some(x) = f64_p(p, "seconds") {
        return Some(x);
    }
    s.state.active_sequence.and_then(|id| s.project.sequence(id)).map(|q| q.duration().seconds())
}

/// Compile a profile into its export preset (`export.presets.save`); returns the preset name.
pub fn compile(s: &mut Session, pr: &Profile) -> Result<String> {
    pr.validate().map_err(|e| bad("bashcut.profiles.compile", e))?;
    s.execute("export.presets.save", pr.preset_params())?;
    Ok(pr.preset_name())
}

pub fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "bashcut.profiles.list",
            label: "List Output Profiles",
            menu: &[],
            shortcut: None,
            params: r#"{"seconds":f64? (estimate; default the active sequence),"dir":str?}"#,
            enabled: always,
            run: |s, p| {
                let secs = seconds_p(s, p);
                Ok(json!({"profiles": all(s, p).iter().map(|x| x.to_json(secs)).collect::<Vec<_>>()}))
            },
            journal: false,
        },
        CommandSpec {
            id: "bashcut.profiles.get",
            label: "Get Output Profile",
            menu: &[],
            shortcut: None,
            params: r#"{"profile":str,"seconds":f64?,"dir":str?}"#,
            enabled: always,
            run: |s, p| {
                let pr = profile_p(s, p, "bashcut.profiles.get")?;
                Ok(pr.to_json(seconds_p(s, p)))
            },
            journal: false,
        },
        CommandSpec {
            id: "bashcut.profiles.compile",
            label: "Compile Output Profiles to Export Presets",
            menu: &[],
            shortcut: None,
            params: r#"{"profile":str? (default every profile),"dir":str?}"#,
            enabled: always,
            run: |s, p| {
                let list = match str_p(p, "profile") {
                    Some(_) => vec![profile_p(s, p, "bashcut.profiles.compile")?],
                    None => all(s, p),
                };
                let mut names = Vec::with_capacity(list.len());
                for pr in &list {
                    names.push(compile(s, pr)?);
                }
                Ok(json!({"presets": names}))
            },
            journal: true,
        },
        CommandSpec {
            id: "bashcut.profiles.apply",
            label: "Export with Output Profile",
            menu: &[],
            shortcut: None,
            params: r#"{"profile":str,"path":str?,"queue":bool=false,"wait":bool=false,"sequence":id?,"range":str?,"hooks":bool=true,"dir":str?}  (no path: compile only, for filmcraft-cli export --preset)"#,
            enabled: always,
            run: apply,
            journal: true,
        },
    ]
}

/// Compile the profile, then export with its preset (`file.exportMedia`, or `export.queue.add` with `queue`).
fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "bashcut.profiles.apply";
    let pr = profile_p(s, p, CMD)?;
    let preset = compile(s, &pr)?;
    let mut out = pr.to_json(seconds_p(s, p));
    out["preset"] = json!(preset);
    let Some(path) = str_p(p, "path").filter(|x| !x.is_empty()) else { return Ok(out) };
    if s.state.active_sequence.is_none() && u64_p(p, "sequence").is_none() {
        return Err(EngineError::NoSequence);
    }
    let mut q = json!({"preset": preset, "path": path, "wait": p.get("wait").and_then(Value::as_bool).unwrap_or(false)});
    if let Some(v) = p.get("range") {
        q["range"] = v.clone();
    }
    // plugin hooks declared for `export.before` (a failed required hook stops the export)
    out["hooks"] = super::hooks::fire(s, p, "export.before", json!({"profile": pr.id, "preset": preset, "path": path}), CMD)?;
    let queue = p.get("queue").and_then(Value::as_bool).unwrap_or(false);
    if let Some(id) = u64_p(p, "sequence") {
        q[if queue { "sequences" } else { "sequence" }] = if queue { json!([id]) } else { json!(id) };
    }
    out["export"] = if queue {
        q["start"] = json!(true);
        s.execute("export.queue.add", q)?
    } else {
        s.execute("file.exportMedia", q)?
    };
    Ok(out)
}
