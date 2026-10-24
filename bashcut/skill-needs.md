# What each BashCut skill needs from FilmCraft

For every skill in `bashcut-agent-kit/skills`, the BashCut tools it calls (read from its `SKILL.md` and the
files next to it) and how BashCutPro covers each need, checked against [commands.json](https://github.com/dongnguyenvie/bashcutpro-agent-kit/blob/main/scripts/commands.json)
(`command_list`, 2026-10-20, default release build). Ids and params: [commands.md](commands.md).

Classes:

- **A, available**: one FilmCraft command or MCP tool does it.
- **B, compose**: several commands in one `command_batch`, or commands plus a little work by the agent or a
  kit script (computing gaps from word times, writing JSON into project notes, tiling frames).
- **P, plugin**: work done out of process (a `bashcutpro-plugins` plugin or a kit script), the result brought
  back with existing commands.
- **E, engine**: needs a new command in a `bashcut`-prefixed crate (or upstream), because nothing reads or does
  it today.
- **drop**: BashCut process scaffolding the rewritten skill does without (text in the skill instead).

Whisper note: speech needs `--features whisper` (gap G19). A default build still has every other
`transcript.*` command once words are imported with `transcript.set`, so "words" below is **A with the
whisper build, else P** (an outside transcriber, then `transcript.set`).

## Shared needs (many skills)

| BashCut tool | FilmCraft | Class |
|---|---|---|
| `context get`, `project get`, `timeline get`, `timeline sheet` | MCP `doc_inspect` / `sequence_inspect`, `project.items`, `state.inspect` | A |
| `project data` / `project set-data` (brief, plan as JSON) | `project.setNotes {text, merge}` holds free text; a JSON block there, or a sidecar file beside the `.fcproj` | B |
| `plan check` + `plan apply` (one plan → many edits) | `command_batch` of `timeline.*`, `captions.*`, `effects.*` steps, `stop_on_error`; validation is the agent's | B |
| `--base-rev`, `timeline undo`, `timeline changes` | `edit.undo` / `edit.redo`, `history.list`; no revision guard | A (no revision guard) |
| `ui frame`, `ui frames`, `media frame` | MCP `render_frame {seconds}` (timeline), `file.exportFrame {target: "source", item, time}` (a media file) | A |
| `transcript words`, `media transcript`, `media transcribe` | `transcript.generate` / `transcript.set`, `transcript.inspect`, `transcript.search` | A / P (Whisper note) |
| `speech rate` | words per second from `transcript.inspect` | B |
| `jobs wait`, `jobs status` | `wait: true` on long commands, `jobs.list`, `jobs.cancel` | A |
| `media import`, `media place` | `file.import` / MCP `media_import`, `timeline.place {sourceIn, duration}` | A |
| `layers add` | `sequence.addTracks` | A |
| `platforms get` | a table in the skill | drop |
| `run append` / `run log` / `run checklist`, `checkpoint request` / `status`, `workflow gates` | the agent's own log file and its conversation with the user | drop |
| `schema get`, `skills get` | `command_list`, `filmcraft-cli describe <id>` | A |
| `library list / place / apply / add / save-selection` | `presets.*`, `graphics.template.*`, `lut.*`, `lumetri.applyPreset`, `essentialSound.applyPreset`, `export.presets.*`: one store per kind, no shared library | B |
| `review run`, `review verify`, `review packet` | measuring + judging loop; see `review` | E / P |
| `audio measure`, `audio mix-measure` (LUFS, true peak, speech windows) | nothing read-only; `essentialSound.autoMatch` reports `measuredLufs` while applying gain; or export a wav and measure outside | E (P meanwhile) |

## Per skill

### edit-workflow

| Need | FilmCraft | Class |
|---|---|---|
| create / open / save the project, sequence by format | `file.newProject`, `file.open`, `file.save`, `file.newSequence {width, height, fps}` | A |
| brief and plan as data, stage | `project.setNotes` (JSON block) or sidecar | B |
| one plan applied at once | `command_batch` | B |
| captions group, captions find | `transcript.createCaptions {maxChars, lines, gapFrames}`, `transcript.search` | A (Whisper note) |
| draft and final export, status | `file.exportMedia {loudnessLufs, burnCaptions, wait}`, `export.quick`, `export.queue.*`, CLI `filmcraft-cli export` | A |
| cover frame | `file.exportFrame` | A |
| YouTube chapters | `markers.addChapter`, read markers from `doc_inspect`, write the text | B |
| variants of one edit | `file.saveCopy`, or a second sequence (`file.newSequence {fromItems}`) | B |
| script check (beats vs words heard) | compare the plan's beats with `transcript.inspect` | B |
| gates, checkpoints, run log, review packet | | drop / see `review` |

### rough-cut

| Need | FilmCraft | Class |
|---|---|---|
| transcript per file | `transcript.generate` / `transcript.set` | A / P |
| `media resolve-range --quote` (quote → word-exact range) | `transcript.search {query}` then `transcript.select {from, to}` (word ids); `midSentence` flags from the words around | A + B |
| speech gaps (`media speech-map`) | gaps between word `end` and next `start` in `transcript.inspect`; no noise-floor separation | B |
| selects with reasons, kept / rejected | subclips (`clip.makeSubclip`) or range markers (`markers.addRange {name, comment}`) + `metadata.set` | B |
| place selects in order | `transcript.extract` / `transcript.lift`, or `timeline.place {items}` back to back | A |
| `--dry-run` `cutsInsideWord` | compare planned cut times with word spans before the batch | B |
| one project per short (`project derive`, `variants`) | `file.saveCopy` + `file.open`, or one sequence per short | B |
| `captions find` | `transcript.search` | A |

### captions-text

| Need | FilmCraft | Class |
|---|---|---|
| captions from speech, grouped | `transcript.createCaptions {maxChars, lines, minSeconds, maxSeconds, gapFrames}` | A (Whisper note) |
| SRT import / export | `captions.import`, `captions.export` | A |
| edit text, times | `captions.setText`, `captions.setTimes`, `captions.split`, `captions.merge` | A |
| caption look | `captions.setStyle` (per track: font, size, colour, plate, outline, anchor, margin) | A |
| word-by-word highlight (karaoke) | no per-word style in captions; one `graphics.newText` per word in a batch is possible but heavy | E |
| titles, place / chapter cards, keyword stickers | `graphics.newText`, `graphics.setCharStyle`, `graphics.template.apply` with `.fcgt` versions of the library looks | A once the `.fcgt` files exist (P to make them) |
| fonts: list, fetch open fonts | `fonts.list`; `fetch_fonts.py` installs into the OS font folder | A + P |
| `review layout` (safe area, contrast, overlap) | `render_frame` + a measuring script | P (E later) |
| `media subjects` (keep text off faces) | none | E / P |

### color-grade

| Need | FilmCraft | Class |
|---|---|---|
| measure a frame (`color measure`) | `scopes.read {scopes, time}` (waveform, parade, histogram, vectorscopes as numbers) | A |
| per-clip stats, shot match | `scopes.read` at sampled times per clip; `lumetri.applyMatch` | B / A |
| import a `.cube`, apply as look | `lut.import`, `lumetri.setLook {clip, lut or path}`; strength via `effects.setParam` on Lumetri Color | A |
| one adjustment layer over the video | `file.newAdjustmentLayer` + `timeline.place` + `lumetri.setLook` | B |
| generated look (`grade.py`) | `render_frame` → `grade.py` → `lut.import` | P (script runs unchanged) |
| keep a look | `lumetri` presets / `presets.save`, `lut.export` | A |

### audio-mix

| Need | FilmCraft | Class |
|---|---|---|
| clip level, volume keyframes | `clip.audioGain {mode, db}`, `clipMixer.set {keyframe, time}` | A |
| fades, joins | `sequence.applyAudioTransition` (3 audio transitions) | A |
| fixed music bed / ducking | `clip.audioGain`; `essentialSound.setType` + `essentialSound.set {ducking.*}` + `essentialSound.generateDucking` | A |
| loudness per role, `musicInGaps`, `speechWindows` | none read-only (see shared needs) | E (P meanwhile) |
| final loudness per output | `file.exportMedia {loudnessLufs}` | A |
| sync a second camera by sound | `clip.synchronize {method: "audio"}` | A |
| music and SFX search (`fetch_audio.py`) | script runs unchanged, then `file.import` | P |
| retime music to length | `clip.remix {seconds}` | A |
| SFX at cuts, sync offsets | cut times from `sequence_inspect`, place with a batch; offsets need a peak finder | B / E |

### footage-survey

| Need | FilmCraft | Class |
|---|---|---|
| inventory (fps, size, length, date) | `project.items`, `media.status`, CLI `filmcraft-cli probe` | A |
| shot cuts inside a file | `clip.sceneEditDetection {applyCuts: false, generateMarkers: true}` | A |
| contact sheets | `file.exportFrame` per time + a tiling script | B / P |
| shot descriptions stored | the agent looks, writes `metadata.set {Description, Scene, Shot}` | B |
| exposure / sound / motion per file (`media analyze`) | `scopes.read` samples cover exposure; sound activity, motion: none | B / E |
| one `media digest` read | `doc_inspect` + transcript + metadata, assembled by the agent | B |
| silent audio, broken files | `media.status`, `clip.audioPeak` | A |
| sync camera and screen recording | `clip.synchronize {method: "audio"}` | A |
| `media strip` (frames + level over time) | none | P |
| suspect words (low confidence) | `transcript.set` words carry no confidence | E (or the outside transcriber keeps it) |

### review

| Need | FilmCraft | Class |
|---|---|---|
| look at frames, phone size | `render_frame {seconds, max_side}` | A |
| cut list, shots, gaps | `sequence_inspect` | A |
| black / frozen picture, long still | none | E / P |
| cuts against words (`review sync`, cut inside a word) | `sequence_inspect` vs `transcript.inspect` | B |
| text layout, contrast | none | P / E |
| loudness, true peak, voice vs music | none read-only | E (P meanwhile) |
| what changed since last round (`timeline changes`) | `history.list` | A |
| critic sub-agent | knowledge (port `CRITIC.md`) | drop (no tool) |
| `review verify`, `accept`, `compare`, `packet` | | drop / P later |

### beat-cut

| Need | FilmCraft | Class |
|---|---|---|
| beat grid, tempo, downbeats, energy | none (`clip.remix` analyses internally only) | P now (beats → `markers.add`), E later |
| word edges, speech rate | `transcript.inspect` | B (Whisper note) |
| cut on the grid | `command_batch` of `timeline.razor` / `timeline.trim` / `timeline.place` | B |
| punch-in from headroom | `effects.setParam {effect: "motion", param: "Scale"}`; headroom from media size vs sequence size | A + B |
| rhythm stats, beat sync check | from `sequence_inspect` cut times | B |

### effects

| Need | FilmCraft | Class |
|---|---|---|
| transitions (BashCut's 39 kinds) | `sequence.applyVideoTransition {effect, frames, edge}`: 112 video transitions; needs a name map | A (map: #17) |
| Ken Burns, punch-in, drift (`clip motion`, `clip keyframe`) | `effects.setParam {effect: "motion", keyframe: true}`, `effects.setInterpolation` | A |
| speed, reverse, ramps | `clip.speedDuration {speed, reverse}`; ramps with the `Time Remapping` effect keyframes | A |
| freeze | `clip.frameHold`, `clip.insertFrameHoldSegment` | A |
| reframe to vertical | `Auto Reframe` effect, or `Motion` scale / position | A |
| blurred backdrop for vertical | duplicate on a lower track + `Gaussian Blur` + scale | B |
| text / sticker In and Out | keyframes on opacity / position, or `.fcgt` templates with built-in animation | B |
| covering burned-in text or a logo | `masks.add` + `masks.track` + `Gaussian Blur` / `Mosaic` | A |
| subjects / faces for reframing | none (`Auto Reframe` does its own) | E / P |

### motion-graphics

| Need | FilmCraft | Class |
|---|---|---|
| simple cards, lower thirds, counters | `graphics.*`, `.fcgt` templates (`graphics.template.apply {values}`) | A once templates exist |
| HTML / GSAP renders (hyperframes) | render outside, import alpha video or image sequence (`file.importImageSequence`) | P |
| timing to a word | `transcript.search` | A (Whisper note) |

### voiceover

| Need | FilmCraft | Class |
|---|---|---|
| synthesize and place a line | `tts.create {text, voice, pace, time, track}`, `tts.render` | A (English formant voices only) |
| natural English voice | Kokoro needs `neural-voices`, exposed by `apps/filmcraft` only, not by `filmcraft-cli` | E (a feature flag in the CLI) |
| Vietnamese voice, cloned voice | VieNeu outside → wav → `file.import` + `timeline.place` | P |
| check each take (re-transcribe, similarity) | `transcript.generate` on the take, compare text | B (Whisper note) |
| fit to a window | `tts.create {pace}`, `clip.speedDuration` | A |
| narration windows (gaps in live speech) | from `transcript.inspect` | B |
| record a voiceover | `audio.voiceover.*` | A |

### stock-images

| Need | FilmCraft | Class |
|---|---|---|
| fetch stock, emoji, logos, illustrations | its scripts run unchanged | P |
| place an image with motion | `file.import` + `timeline.place` + `effects.setParam {effect: "motion"}` | A |
| AI image generation, disclosure | image-gen plugin | P |
| credits | `project.setNotes` or an end card | B |

### visual-plan

Needs image generation (P), frame looks (`render_frame`, A), placing (`timeline.place`, A) and a coverage
check (`sequence_inspect` against the plan, B). Blocked on the image-gen plugin.

### style-study

| Need | FilmCraft | Class |
|---|---|---|
| import a reference video, find its cuts | `file.import`, `clip.sceneEditDetection` | A |
| colour numbers | `scopes.read` | A |
| shot frames, descriptions | `file.exportFrame`, agent looks | B |
| speech rate, words | `transcript.*` | A / P (Whisper note) |
| on-screen text (`media ocr`) | none | P / E |
| loudness | none read-only | E |

### library

`library add / apply / save-selection / export-pack / import-pack` map to separate stores: effect presets
(`presets.save {keyframes}` / `apply` / `export` / `import`), graphics templates (`graphics.template.export` /
`install` / `apply`), LUTs (`lut.import` / `export`), Lumetri and Essential Sound presets, export presets.
No shared library, search, favourites or generation. Class: B for presets, E for a unified library.

### self-learn

BashCut's `knowledge.*` store (lessons, facts, prefs, memos) has no counterpart; per-project facts can live in
`project.setNotes`. Class: drop (or an agent-side memory file).

### setup

Becomes MCP registration (`filmcraft-cli mcp` headless, `--bridge` for the live app) in Claude Code / Codex
config, and building with `--features whisper`. BashCut's `plugins *` and `voice speak` checks: drop.

## New engine commands worth having

Ranked by how many skills wait on them. None exists today; each would go in a `bashcut`-prefixed crate.

1. **Loudness meter** (read-only): integrated / short-term LUFS, true peak, per clip, per track, per range of
   the mix. Needed by audio-mix, review, voiceover, edit-workflow, style-study.
2. **Beat / tempo / energy analysis** of an audio item, returning beats and downbeats (or writing markers).
   Needed by beat-cut, effects, review, style-study.
3. **Picture checks**: black frames, frozen frames, long stills over a range. Needed by review.
4. **Word-by-word caption styling** (highlight the spoken word). Needed by captions-text and most vlog recipes.
5. **Frame analysis**: subjects / faces boxes, text contrast in a region, OCR. Needed by captions-text, review,
   effects, style-study; a plugin can do it from `render_frame` PNGs first.
6. **`neural-voices` feature in `filmcraft-cli`**, so headless agents get Kokoro (a one-line upstream-file
   change, so check upstream first).
