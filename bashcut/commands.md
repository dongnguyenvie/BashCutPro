# BashCutPro command catalogue

The single source is the agent kit's **`scripts/commands.json`** ([bashcutpro-agent-kit](https://github.com/dongnguyenvie/bashcutpro-agent-kit)),
generated from a build's own `command_list` (`bashcutpro-cli mcp --demo`, v0.10.0, upstream `dd525c93`, whisper,
Windows, 2026-10-23). BashCutPro keeps no copy of it. After an upstream merge or a new `bashcut.*` command, refresh the
kit (`python3 scripts/refresh_commands.py --cli <abs>/target/debug/bashcutpro-cli`).
Each row has id, label, menu, shortcut, params and enabled; `frontend` lists window-only ids a headless build lacks.

`enabled` is the state in the demo project with nothing selected: 242 of 730 are `false` because they need a
selection, a transcript, a caption track or a saved path, not because they are missing. `bashcutpro-cli
describe <id>` prints one command; `bashcutpro-cli exec --demo <id> key=value` runs one headless. The `bashcut.*`
ids are described in [bashcut-commands.md](bashcut-commands.md).

## Totals

**730 engine commands** (701 FilmCraft + 29 `bashcut.*`) in 43 namespaces, plus **17 MCP tools**: `command_list`, `command_run`,
`command_batch`, `doc_inspect`, `project_inspect`, `sequence_inspect`, `media_import`, `render_frame`,
`render_preview`, and in `--bridge` mode `ui_elements`, `ui_inspect`, `ui_click`, `ui_drag`, `ui_key`,
`ui_type`, `ui_control`, `ui_screenshot`.

| Namespace | Count | Namespace | Count | Namespace | Count |
|---|---|---|---|---|---|
| `graphics` | 82 | `captions` | 22 | `lut` | 4 |
| `file` | 57 | `mixer` | 20 | `audio` | 5 |
| `clip` | 57 | `export` | 19 | `prefs` | 4 |
| `timeline` | 56 | `media` | 17 | `labels` | 4 |
| `sequence` | 45 | `effects` | 16 | `clipMixer` | 4 |
| `project` | 40 | `shortcuts` | 16 | `jobs`, `mediaCache`, `events`, `metadata` | 2 each |
| `edit` | 39 | `playhead` | 14 | `command`, `state`, `history`, `perf` | 1 each |
| `multicam` | 35 | `transcript` | 14 | `color`, `fonts`, `scopes`, `help` | 1 each |
| `markers` | 32 | `masks` | 12 | `bashcut` | 29 |
| `trim` | 24 | `mediaBrowser` | 10 | | |
| | | `essentialSound` | 9 | | |
| | | `source`, `lumetri`, `presets`, `tts` | 7 each | | |

Effects are data, not commands: `effects.list` returns **130 video effects, 112 video transitions, 58 audio
effects, 3 audio transitions** (applied with `effects.apply`, `sequence.applyVideoTransition`,
`sequence.applyAudioTransition`). They include `Motion`, `Time Remapping` (speed ramps), `Auto Reframe`,
`Lumetri Color`, `Warp Stabilizer`, `Gaussian Blur`, `Crop`, `RGB Split`, `Light Leaks`, `Camera Shake`.

## Ids that matter for editing agents

Time params take ticks (254 016 000 000/s) or `seconds`, `frame`, `timecode` where listed.

| Job | Ids |
|---|---|
| Read state | MCP `doc_inspect`, `project_inspect`, `sequence_inspect`; `project.inspect`, `project.items`, `sequence.inspect`, `state.inspect`, `history.list` (edit journal), `events.list`, `jobs.list`, `media.status` |
| Project | `file.newProject`, `file.open`, `file.save`, `file.saveAs`, `file.saveCopy`, `file.newSequence`, `project.notes` / `project.setNotes` (free text, `merge`), `metadata.get` / `metadata.set` (Description, Scene, Shot, Log Note, Comment… per item or clip) |
| Media | `file.import` (or MCP `media_import`), `file.importImageSequence`, `clip.makeSubclip`, `media.search`, `media.relink`, `media.createProxies`; CLI `filmcraft-cli probe <file>` |
| Place and cut | `timeline.place` (`sourceIn`, `duration`, `insert`), `timeline.razor`, `timeline.trim` (regular / ripple), `timeline.roll`, `timeline.slip`, `timeline.slide`, `timeline.move`, `edit.rippleDelete`, `sequence.lift`, `sequence.extract`, `sequence.closeGap`, `sequence.addEdit`, `sequence.addTracks`, `source.insert` / `source.overwrite` |
| Text-based editing | `transcript.generate` (or `sequence.transcribe`), `transcript.set` (import words), `transcript.inspect`, `transcript.search`, `transcript.select`, `transcript.lift`, `transcript.extract`, `transcript.removePauses`, `transcript.removeFillers`, `transcript.createCaptions`, `transcript.renameSpeaker`; `generate` needs Whisper (not in the default build), the rest work on an imported transcript (see below) |
| Captions | `captions.newTrack`, `captions.import` (SRT, VTT, SCC, MCC, STL, TTML / DFXP), `captions.export`, `captions.list`, `captions.add`, `captions.setText`, `captions.setTimes`, `captions.split`, `captions.merge`, `captions.setStyle` (track-level: font, size, colour, plate, outline, anchor, margin); burn-in through `file.exportMedia` `burnCaptions` |
| Titles and graphics | `graphics.newText`, `graphics.setText`, `graphics.setCharStyle`, `graphics.set`, `graphics.newRectangle` / `newEllipse` / `newShape`, `graphics.template.install` / `apply` / `set` / `list` / `export` / `thumbnail` (`.fcgt`; `.mogrt` refused), `fonts.list`, `graphics.fonts.used`, `file.replaceFonts` |
| Motion, speed, effects | `effects.apply`, `effects.setParam` (`keyframe: true` to animate; `"effect": "motion"` for scale / position), `effects.addKeyframe`, `effects.setInterpolation`, `effects.list`, `clip.speedDuration` (speed, `reverse`), `clip.frameHold`, `clip.insertFrameHoldSegment`, `masks.add`, `masks.track`, `presets.save` / `apply` / `export` / `import`, `file.newAdjustmentLayer` |
| Transitions | `sequence.applyVideoTransition` (`effect`, `frames`, `edge`), `sequence.applyAudioTransition`, `sequence.setTransition`, `sequence.removeTransition`, `trim.applyDefaultTransition` |
| Colour | `lumetri.setLook` / `setInputLut` (a `.cube` path or `lib:` id), `lumetri.applyPreset`, `lumetri.applyMatch`, `lumetri.setSection`, `lut.import` / `list` / `export`, `scopes.read` (waveform, parade, histogram, vectorscopes, as numbers) |
| Sound | `clip.audioGain` (set, adjust, normalize), `clipMixer.set` (level / pan, keyframes), `mixer.setStrip` / `setKeyframe` (track level), `essentialSound.setType` / `set` / `applyPreset` / `autoMatch` / `generateDucking`, `clip.audioPeak`, `sequence.normalizeMixTrack`, `clip.remix` (retime music), `clip.synchronize` (`method: "audio"`) |
| Voice | `tts.voices`, `tts.create` (places a clip), `tts.render`, `tts.preview`; `audio.voiceover.*` records the microphone |
| Shots and markers | `clip.sceneEditDetection` (cuts, subclips or markers), `markers.add` / `addRange` / `addChapter`, `markers.exportCsv`, `multicam.*` |
| Frames | MCP `render_frame` / `render_preview`; `file.exportFrame` (`target: "source"` with an `item` renders a media file, not only the timeline); CLI `filmcraft-cli render --seconds S --out f.png` |
| Export | `file.exportMedia` (format, size, `loudnessLufs`, `burnCaptions`, `captionSidecar`, range), `export.quick`, `export.presets.*`, `export.queue.*`; CLI `filmcraft-cli export`; interchange: `file.exportOtio`, `file.exportFcpxml`, `file.exportEdl`, `file.exportAaf` |
| Undo | `edit.undo`, `edit.redo`; every `command_run` is one undo step, `command_batch` runs steps in order (`stop_on_error`) |

## What the default build cannot do

- **Speech** (`transcript.*`, `sequence.transcribe`): `transcript.models` reports `"available": false` in a
  default build; Whisper needs `--features whisper` (gap G19), which also enables model download. With
  `--features whisper` the build compiles, lists the same commands and reports `"available": true`; a model is
  then downloaded with `transcript.downloadModel`. Without it,
  **`transcript.set`** imports word timings made elsewhere (`{"item", "transcript": {"language", "speakers",
  "words": [{"text", "start", "end", "speaker"}]}}`, ticks), and every other `transcript.*` command then works
  on them. Words carry no confidence.
- **Natural voices**: `tts.*` uses the built-in formant voices (English only). Kokoro needs the
  `neural-voices` feature, which only the desktop app (`apps/filmcraft`) exposes; `filmcraft-cli` has no such
  feature, so headless MCP cannot use it.
- **No read-only measurement** of loudness (integrated LUFS, true peak), music beats or energy, black or frozen
  frames, text contrast, or subjects in the frame. `essentialSound.autoMatch` reports `measuredLufs`, but only
  as it applies gain.
