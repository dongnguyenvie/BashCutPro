//! BashCutPro: output profiles (#43).

use serde_json::json;

use std::sync::Arc;

use super::profiles::{Profile, builtins, file_true_peak, retry_limit};
use crate::Session;
use crate::media_test_util::tmp_dir;

#[test]
fn every_builtin_profile_compiles_to_a_valid_export_preset() {
    let mut s = Session::default();
    let none = tmp_dir("bashcut-profiles-none");
    let dir = none.to_string_lossy().into_owned();
    for pr in builtins() {
        pr.validate().unwrap();
        let r = s.execute("bashcut.profiles.compile", json!({"profile": pr.id, "dir": dir})).unwrap();
        assert_eq!(r["presets"][0], pr.preset_name());
        let got = s.execute("export.presets.get", json!({"name": pr.preset_name()})).unwrap();
        assert_eq!(got["frameSize"], json!([pr.width, pr.height]), "{got}");
        assert_eq!(got["format"], "h264", "{got}");
        let st = &got["settings"];
        assert_eq!(st["bitrateKbps"], json!(pr.video_kbps()), "{st}");
        assert_eq!(st["effects"]["loudness"]["enabled"], true, "{st}");
        assert_eq!(st["effects"]["loudness"]["target_lufs"], json!(pr.loudness_lufs), "{st}");
        assert_eq!(st["effects"]["loudness"]["true_peak_dbtp"], json!(pr.true_peak_dbtp - 0.5), "{st}: codec headroom");
        assert_eq!(st["burnCaptions"], json!(pr.captions == "burn"), "{st}");
        // the compiled preset resolves like any other (what filmcraft-cli export --preset does)
        assert!(crate::export_tools::settings_from_params(&s, &json!({"preset": pr.preset_name()}), "test").is_ok());
    }
    let all = s.execute("bashcut.profiles.list", json!({"dir": dir, "seconds": 60})).unwrap();
    assert_eq!(all["profiles"].as_array().unwrap().len(), builtins().len());
    assert!(all["profiles"][0]["estimatedBytes"].as_u64().unwrap() > 0);
}

#[test]
fn bitrate_follows_platform_guidance() {
    let p = |w, h, fps, codec: &str, bits| Profile { width: w, height: h, fps, codec: codec.into(), bit_depth: bits, ..Profile::default() };
    assert_eq!(p(1920, 1080, 30.0, "h264", 8).video_kbps(), 8_000);
    assert_eq!(p(1080, 1920, 30.0, "h264", 8).video_kbps(), 8_000, "vertical uses the short side");
    assert_eq!(p(1920, 1080, 60.0, "h264", 8).video_kbps(), 12_000);
    assert_eq!(p(1920, 1080, 30.0, "hevc", 8).video_kbps(), 5_600);
    assert_eq!(p(1920, 1080, 30.0, "h264", 10).video_kbps(), 10_000);
    assert_eq!(p(3840, 2160, 30.0, "h264", 8).video_kbps(), 40_000);
    assert_eq!(p(1280, 720, 30.0, "av1", 8).video_kbps(), 3_000);
    let one_min = p(1920, 1080, 30.0, "h264", 8).estimate_bytes(60.0);
    assert_eq!(one_min, (8_000 + 320) * 1000 / 8 * 60);
    assert_eq!(Profile::default().estimate_bytes(f64::NAN), 0);
    assert_eq!(Profile::default().estimate_bytes(-1.0), 0);
}

#[test]
fn user_profiles_add_or_replace_and_bad_files_are_skipped() {
    let dir = tmp_dir("bashcut-profiles-user");
    std::fs::write(dir.join("tiktok.json"), br#"{"id":"tiktok","name":"TikTok 60","width":1080,"height":1920,"fps":60}"#).unwrap();
    std::fs::write(dir.join("clip.json"), br#"{"name":"Client","width":1280,"height":720,"captions":"none"}"#).unwrap();
    std::fs::write(dir.join("huge.json"), br#"{"id":"huge","width":4000000000,"height":1}"#).unwrap();
    std::fs::write(dir.join("nan.json"), br#"{"id":"nan","fps":-1}"#).unwrap();
    std::fs::write(dir.join("evil.json"), br#"{"id":"../evil"}"#).unwrap();
    std::fs::write(dir.join("broken.json"), b"{").unwrap();
    let d = dir.to_string_lossy().into_owned();
    let mut s = Session::default();
    let r = s.execute("bashcut.profiles.get", json!({"profile": "tiktok", "dir": d})).unwrap();
    assert_eq!(r["fps"], 60.0, "{r}");
    assert_eq!(r["videoKbps"], 12_000);
    let r = s.execute("bashcut.profiles.get", json!({"profile": "clip", "dir": d})).unwrap();
    assert_eq!(r["name"], "Client");
    let list = s.execute("bashcut.profiles.list", json!({"dir": d})).unwrap();
    assert_eq!(list["profiles"].as_array().unwrap().len(), builtins().len() + 1, "{list}");
    for id in ["huge", "nan", "evil", "broken", "../evil"] {
        assert!(s.execute("bashcut.profiles.get", json!({"profile": id, "dir": d})).is_err(), "{id}");
    }
}

#[test]
fn profile_commands_refuse_hostile_params() {
    let mut s = Session::default();
    let d = tmp_dir("bashcut-profiles-hostile").to_string_lossy().into_owned();
    for (id, p) in [
        ("bashcut.profiles.get", json!({})),
        ("bashcut.profiles.get", json!({"profile": 7})),
        ("bashcut.profiles.apply", json!({"profile": "nope", "dir": d})),
        ("bashcut.profiles.apply", json!({"profile": "tiktok", "path": "out.mp4", "dir": d})),
        ("bashcut.profiles.compile", json!({"profile": "nope", "dir": d})),
    ] {
        assert!(s.execute(id, p.clone()).is_err(), "{id} {p}");
    }
    // no path: compile only, with the safe area for review
    let r = s.execute("bashcut.profiles.apply", json!({"profile": "reels", "dir": d, "seconds": 1e300})).unwrap();
    assert_eq!(r["preset"], "BashCut Reels");
    assert!(r["safeAreaPx"]["bottom"].as_f64().unwrap() > 0.0);
}

#[test]
fn user_files_come_from_the_sessions_data_dir() {
    // `filmcraft-cli --data-dir` (and the app) point the session's libraries at one data dir; our files follow it
    let data = tmp_dir("bashcut-profiles-data-dir");
    let folder = data.join("bashcut").join("profiles");
    std::fs::create_dir_all(&folder).unwrap();
    let mine = Profile { id: "mine".into(), name: "Mine".into(), ..Profile::default() };
    std::fs::write(folder.join("mine.json"), serde_json::to_vec(&mine).unwrap()).unwrap();
    let mut s = Session::default();
    s.export_presets.set_dir(&data);
    let all = s.execute("bashcut.profiles.list", json!({})).unwrap();
    assert!(all["profiles"].as_array().unwrap().iter().any(|p| p["id"] == "mine"), "{all}");
    let t = s.execute("bashcut.template.list", json!({})).unwrap();
    assert_eq!(t["dir"].as_str().map(std::path::PathBuf::from), Some(data.join("bashcut").join("templates")), "{t}");
}

#[test]
fn an_srt_profile_writes_no_empty_caption_file() {
    // after bashcut.captions.wordStyle the caption track is empty (the words are graphic clips): no empty .srt (#173)
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.execute("captions.newTrack", json!({})).unwrap();
    // half a second keeps the exports quick (In/Out is the default range)
    s.execute("markers.markIn", json!({"seconds": 0.0})).unwrap();
    s.execute("markers.markOut", json!({"seconds": 0.5})).unwrap();
    let d = tmp_dir("bashcut-profiles-sidecar");
    let out = d.join("empty.mp4");
    let r =
        s.execute("bashcut.profiles.apply", json!({"profile": "youtube", "path": out.to_string_lossy(), "dir": d.to_string_lossy(), "wait": true})).unwrap();
    assert!(r["captionSidecarSkipped"].as_str().is_some_and(|n| n.contains("captions.export")), "{r}");
    assert!(out.exists());
    assert!(!d.join("empty.srt").exists(), "no empty caption file");

    // with a caption the sidecar is written as before
    s.execute("captions.add", json!({"text": "Hello there", "seconds": 0.0, "durationSeconds": 1.0})).unwrap();
    let out = d.join("captioned.mp4");
    let r =
        s.execute("bashcut.profiles.apply", json!({"profile": "youtube", "path": out.to_string_lossy(), "dir": d.to_string_lossy(), "wait": true})).unwrap();
    assert!(r.get("captionSidecarSkipped").is_none(), "{r}");
    let srt = std::fs::read_to_string(d.join("captioned.srt")).unwrap();
    assert!(srt.contains("Hello there"), "{srt}");
}

/// Stereo 48 kHz WAV of `mono` imported as a project item.
fn import_wav(s: &mut Session, name: &str, mono: &[f32]) -> u64 {
    let inter: Vec<f32> = mono.iter().flat_map(|v| [*v, *v]).collect();
    let bytes: Arc<[u8]> = crate::previews::write_wav_f32(&inter, 48_000).into();
    crate::commands::import_bytes(s, &format!("/{name}.wav"), bytes, None).unwrap().0
}

#[test]
fn a_profile_export_lands_under_its_true_peak_limit() {
    // sparse square-wave bursts, driven into the limiter by the loudness normalisation; the encoded file is measured
    // after the export (#184)
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "peaks", "video": 1, "audio": 1, "width": 320, "height": 180})).unwrap();
    let square: Vec<f32> = (0..96_000).map(|i| if i % 9_600 < 960 { if (i / 24) % 2 == 0 { 0.5 } else { -0.5 } } else { 0.0 }).collect();
    let item = import_wav(&mut s, "square", &square);
    s.execute("timeline.place", json!({"item": item, "audioTrack": "A1", "seconds": 0.0, "video": false})).unwrap();
    let d = tmp_dir("bashcut-profiles-peak");
    let out = d.join("square.mp4");
    let r = s.execute("bashcut.profiles.apply", json!({"profile": "tiktok", "path": out.to_string_lossy(), "dir": d.to_string_lossy(), "wait": true})).unwrap();
    let check = &r["peakCheck"];
    let limit = check["limitDbtp"].as_f64().unwrap_or_else(|| panic!("no peak check: {r}"));
    let measured = file_true_peak(&s, &out).unwrap();
    assert!(measured <= limit, "{measured} dBTP over the {limit} dBTP limit: {check}");
    assert_eq!(check["truePeakDbtp"].as_f64(), Some(measured), "{check}");
    assert_eq!(check["reexported"], false, "{check}");
}

#[test]
fn an_overshoot_lowers_the_limit_by_the_overshoot_plus_a_margin() {
    // the bench's vlog: -0.41 dBTP after AAC for a -1 dBTP limit (#184)
    let l = retry_limit(-1.0, -0.41).unwrap();
    assert!((l - (-1.0 - 0.59 - 0.3)).abs() < 1e-9, "{l}");
    assert_eq!(retry_limit(-1.0, -1.0), None);
    assert_eq!(retry_limit(-1.0, -3.0), None);
    assert_eq!(retry_limit(-1.0, f64::NAN), None);
    assert_eq!(retry_limit(-1.0, f64::INFINITY), None);
    assert_eq!(retry_limit(-1.0, 40.0), Some(-9.0), "never under the profile floor");
}

#[test]
fn file_true_peak_reads_a_file_and_refuses_hostile_ones() {
    let s = Session::default();
    let d = tmp_dir("bashcut-profiles-file-peak");
    let wav = d.join("half.wav");
    let sine: Vec<f32> = (0..48_000).map(|i| 0.5 * (i as f32 * 0.05).sin()).flat_map(|v| [v, v]).collect();
    std::fs::write(&wav, crate::previews::write_wav_f32(&sine, 48_000)).unwrap();
    let tp = file_true_peak(&s, &wav).unwrap();
    assert!((tp + 6.02).abs() < 0.3, "{tp}");
    assert_eq!(file_true_peak(&s, &d.join("missing.mp4")), None);
    let junk = d.join("junk.mp4");
    std::fs::write(&junk, b"not media at all").unwrap();
    assert_eq!(file_true_peak(&s, &junk), None);
}
