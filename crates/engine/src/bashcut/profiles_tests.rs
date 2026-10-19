//! BashCutPro: output profiles (#43).

use serde_json::json;

use super::profiles::{Profile, builtins};
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
        assert_eq!(st["effects"]["loudness"]["true_peak_dbtp"], json!(pr.true_peak_dbtp), "{st}");
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
