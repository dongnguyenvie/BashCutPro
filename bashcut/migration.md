# Migration plan: BashCut → BashCutPro

What to bring over from `bashcut-agent-kit` and `bashcut-plugins`, what to skip, and why. Checked against
FilmCraft's live command catalogue (**701 commands**, MCP `command_list` on a release `filmcraft-cli`,
2026-10-20): [commands.json](https://github.com/dongnguyenvie/bashcutpro-agent-kit/blob/main/scripts/commands.json), summary in [commands.md](commands.md). What each skill needs, need
by need: [skill-needs.md](skill-needs.md). (The first version of this file guessed ~1,000 ids from source.)

Verdicts: **migrate** (now), **later** (worth it, blocked on something), **skip** (FilmCraft already
does it, or it needs something we decided not to build yet).

## FilmCraft already has

| Area | Commands |
|---|---|
| Transcript and text-based editing | `transcript.generate / inspect / search / select / lift / extract / removeFillers / removePauses / createCaptions` (Whisper on candle; feature `whisper`, off in default builds: gap G19). **`transcript.set` imports words made elsewhere**, and every other `transcript.*` command then works without Whisper |
| Captions | `captions.add / import / export / list / setText / setTimes / merge / split / setStyle …`, SRT / VTT / SCC / MCC / STL / TTML, burn-in at export; style is per caption track, no word-by-word highlight |
| Text and graphics | `graphics.*` (82 ids), `.fcgt` templates (`graphics.template.install / apply / set / export`), `fonts.list` |
| Colour | `lumetri.*` (7: `setLook`, `setInputLut`, `applyPreset`, `applyMatch`…), `lut.import / list / export`, `scopes.read` (scopes as numbers) |
| Sound | `essentialSound.autoMatch` (applies loudness; no read-only meter), `essentialSound.generateDucking`, mixer, `clipMixer.set` (volume keyframes), `clip.audioGain`, `clip.remix`, export `loudnessLufs` |
| Voice | `tts.voices / create / render / preview` (formant voices, English only; Kokoro needs `neural-voices`, which `filmcraft-cli` does not expose, so headless MCP has formant voices only; no Vietnamese) |
| Cutting aids | `clip.sceneEditDetection`, `clip.synchronize` (audio sync), multicam, markers, `clip.speedDuration` (incl. reverse), `clip.frameHold`, `clip.makeSubclip` |
| Effects | 130 video effects (incl. `Time Remapping`, `Auto Reframe`, `Warp Stabilizer`), 112 video transitions, 58 audio effects, 3 audio transitions (`effects.list`); masks with tracking (`masks.track`), keyframes (`effects.*`) |
| Export | `file.exportMedia`, `export.quick`, `export.presets.*`, `export.queue.*`, `file.exportFrame`, OTIO / FCPXML / EDL / AAF |
| Frames | MCP `render_frame` (timeline), `file.exportFrame {target: "source"}` (a media file) |

## Missing in FilmCraft

Beat detection as a command (remix analyses beats internally only), a read-only loudness meter (LUFS, true
peak), black / frozen frame checks, word-by-word caption styling, subjects / OCR / text-contrast analysis,
word confidence, a speech map (gaps can be computed from word times), contact sheets (frames exist, tiling does
not), a library of reusable items (separate preset stores exist), an agent knowledge store, edit plans
(`plan.cut`; `command_batch` covers applying), review packets, image generation, Vietnamese TTS, caption
translation, plugin hosting. Ranked list of engine commands worth adding: [skill-needs.md](skill-needs.md).

## Build

- `cargo build --release -p filmcraft-cli`: builds (rustc stable 1.99, 2m 47s cold on an M-series Mac).
- `--features whisper`: builds too (4m 00s cold), same 701 commands; `transcript.models` then reports
  `"available": true`, and `transcript.generate` asks for a model first (`transcript.downloadModel`, e.g.
  `whisper-base`, 74 M parameters). Transcription itself was not run (no model downloaded).
- Agents should use the whisper build; with the default build, transcribe outside and `transcript.set`.

## Skills (`bashcut-agent-kit/skills`)

| Skill | Verdict | Why |
|---|---|---|
| `edit-workflow` | **migrate** (lean) | The spine of every edit. Rewrite small: stages and gates as text; no `plan.cut`, `run checklist` or review packets, which FilmCraft lacks; use `command_batch` instead of one-plan apply; brief and plan as a JSON block in `project.setNotes` (or a sidecar file) |
| `rough-cut` | **migrate** | FilmCraft's text-based editing (`transcript.search / select / lift / extract`) covers cut-by-quote directly; highest value per hour. Needs words: the whisper build, or an outside transcript through `transcript.set`. Selects become subclips or range markers |
| `captions-text` | **migrate** | `captions.*`, `transcript.createCaptions`, graphics text and templates cover it; keep the Vietnamese font and readability rules. Gaps: word-by-word highlight (engine), layout / contrast checks (plugin), the library looks as `.fcgt` |
| `color-grade` | **migrate** | Lumetri, LUT import, `scopes.read`, `applyMatch`; `grade.py` (measures frames, writes `.cube`) runs unchanged |
| `audio-mix` | **migrate** | Essential Sound (loudness auto-match, ducking), mixer, gain, volume keyframes; `fetch_audio.py` runs unchanged. Its ranges are measured, and nothing measures loudness read-only: until a meter command exists, export a wav and measure outside |
| `footage-survey` | **migrate** | `transcript.generate`, `clip.sceneEditDetection`, `render_frame`; contact sheets built by a script from rendered frames |
| `review` | **migrate** (lean) | `render_frame`, `scopes.read`, `sequence_inspect`, `history.list`; the critic brief is knowledge; drop review packets / `review verify`. No loudness meter, black / frozen frame or contrast check yet: look at frames, measure outside |
| `stock-images` | **migrate** | Its fetch scripts are standalone (official APIs) and land files for `media_import` |
| `effects` | **migrate** | Mostly available: motion keyframes, `Time Remapping` ramps, reverse, freeze, `Auto Reframe`, masks with tracking. Needs a name map from BashCut's 39 transitions to FilmCraft's 112 |
| `beat-cut` | **later** | Word-edge rhythm works now; music beats need a beat-detection plugin or an engine command |
| `voiceover` | **later** | English works with `tts.*` (formant voices only over headless MCP; Kokoro is desktop-app-only); Vietnamese waits for the VieNeu plugin |
| `motion-graphics` | **later** | Needs a `graphics.render` plugin (hyperframes); simple cards can be `.fcgt` templates meanwhile |
| `visual-plan` | **later** | Depends on image generation (plugin) |
| `style-study` | **later** | Useful, but builds on survey, beat and colour measurement landing first |
| `setup` | **migrate** (rewrite) | Becomes "register `filmcraft-cli mcp` in Claude Code / Codex"; part of the kit scaffold |
| `library` | **migrate later** (was skip; reversed 2026-10-20: library plugin #67, online browser #68) | No unified library, but more is there than first read: effect presets with keyframes (`presets.save / apply / export / import`), `.fcgt` templates (`graphics.template.export / install`), LUTs, Lumetri / Essential Sound / export presets. Revisit as a "presets" skill |
| `self-learn` | **migrate later** (was skip; reversed 2026-10-20: kit-owned files, #66) | Built on BashCut's `knowledge.*` store; nothing to write to |

Sub-agents: port `surveyor` and `critic` with their skills; `voice` and `graphics` with their plugins.
Hooks (`correction`, `precompact`, `record-skill`): **skip**, they feed `self-learn`.

## Plugins (`bashcut-plugins/plugins`)

| Plugin | Verdict | Why |
|---|---|---|
| `translate-captions` | **migrate first** | No FilmCraft equivalent, high value (Vietsub), and a clean out-of-process loop: `captions.list` → translate → `captions.setText`; covering burned-in subtitles uses FilmCraft masks + blur. Source is Swift, so rewrite; it also settles the plugin manifest and registry format |
| `vieneu-tts` | **migrate** | Vietnamese voice, which FilmCraft lacks; the provider is already Python; output wav → `media_import` |
| `resources` | **migrate** | Python fetch scripts for music, SFX, stock, fonts; no app coupling |
| `vlog` | **migrate** as skills | Mostly recipes (knowledge); its library pack (text styles, stickers, transitions) waits for `.fcgt` conversions |
| `image-gen` | **later** | Worth it, but Swift; rewrite after the plugin format is settled by `translate-captions` |
| `hyperframes` | **later** | HTML / GSAP → alpha video works out of process (import as image sequence or alpha video), but it is the largest port |
| `whisper-captions` | **skip** (re-check) | FilmCraft has Whisper built in (`transcript.generate`) when built with `--features whisper`; keep its vocabulary / proper-name tips in `captions-text`. If we ship a default build, an outside transcriber feeding `transcript.set` replaces it |
| `silence-markers` | **skip** | `transcript.removePauses` and markers already do it |
| `director` | **skip** | An in-app chat dock; needs `bashcut-plugin-host`. Agents use Claude Code / Codex over MCP instead |
| `antigravity` | **skip** | An in-app agent terminal; same reason |

## Order of work

Tracked in [#24](https://github.com/dongnguyenvie/BashCutPro/issues/24); one issue per item.

1. **Foundation**: command catalogue (#3), scaffold `bashcutpro-agent-kit` + `setup` skill (#4).
2. **Edit core skills**: `edit-workflow` lean (#5), `rough-cut` (#6), `captions-text` (#7).
3. **First plugin**: `translate-captions`, which also defines `bashcut-plugin.json` and the registry (#8).
4. **Next**: `color-grade` (#9), `audio-mix` (#10), `footage-survey` + `surveyor` (#11), `review` + `critic`
   (#12), `vieneu-tts` (#13), `resources` (#14), `stock-images` (#15), vlog recipes (#16), `effects` map (#17).
5. **Later**: `beat-cut` (#18), `voiceover` vi (#19), `image-gen` (#20), `hyperframes` / `motion-graphics`
   (#21), `style-study` (#22), `bashcut-plugin-host` (#23).

Steps 1 and 3 can run in parallel (different repos). Step 2 needs step 1's catalogue.
