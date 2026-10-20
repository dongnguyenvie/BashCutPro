# Output profiles (#43)

Platform-correct exports without tuning. Code: `crates/engine/src/bashcut/profiles.rs`, tests in `profiles_tests.rs`.

- **Profile** (`bashcut-profile/1`, camelCase JSON): `id`, `name`, `platform`, `width`, `height`, `fps`, `codec`
  (`h264` | `hevc` | `av1`), `bitDepth`, `bitrateKbps?`, `audioKbps`, `sampleRate`, `loudnessLufs`, `truePeakDbtp`,
  `captions` (`burn` | `srt` | `vtt` | `none`), `safeArea {top,bottom,left,right}` (fractions of the frame), `notes`.
  An `srt` / `vtt` sidecar is skipped when the sequence's first caption track has no captions (after
  `bashcut.captions.wordStyle` they are graphic clips): `apply` returns `captionSidecarSkipped` instead of writing an
  empty file (#173).
- **Built-ins:** `tiktok`, `reels`, `shorts` (1080×1920), `youtube` (1080p), `youtube-4k`, `facebook-square`. User files
  `<data dir>/bashcut/profiles/*.json` add profiles or replace a built-in with the same `id`; invalid files are skipped.
- **Bitrate guidance** (when `bitrateKbps` is not set): by the short side, 2.5 / 5 / 8 / 16 / 40 / 80 Mbps at
  ≤480 / 720 / 1080 / 1440 / 2160 / above; ×1.5 above 30 fps; ×0.7 HEVC, ×0.6 AV1; ×1.25 at 10-bit.
- **Compile:** each profile becomes the user export preset `BashCut <name>` (category `BashCutPro`) through
  `export.presets.save`, so upstream's library persists it in `<data dir>/export-presets.json` and
  `filmcraft-cli export out.mp4 --preset "BashCut TikTok"`, the export queue and Quick Export use it. No upstream edit.
  The preset's loudness normalisation uses `loudnessLufs` and a true-peak ceiling 0.5 dB under `truePeakDbtp`: AAC
  encoding overshoots the limiter by a few tenths of a dB (a TikTok export measured -0.75 dBTP with a -1 ceiling, #127).

| Command | Params | Result |
|---|---|---|
| `bashcut.profiles.list` | `seconds?` | every profile with `videoKbps`, `preset`, `safeAreaPx`, `bytesPerMinute`, `estimatedBytes` (active sequence or `seconds`) |
| `bashcut.profiles.get` | `profile` | one profile |
| `bashcut.profiles.compile` | `profile?` | writes the export preset(s) |
| `bashcut.profiles.apply` | `profile`, `path?`, `queue?`, `wait?`, `sequence?`, `range?` | compiles, then `file.exportMedia` (or `export.queue.add` with `queue`); without `path` it only compiles |

Every command takes `dir` to read profiles from another folder (tests, batch runs).

Not done here: a live loudness check of a demo export per vertical profile (manual, build pass).
