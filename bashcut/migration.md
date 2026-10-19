# Migration plan: BashCut → BashCutPro

What to bring over from `bashcut-agent-kit` and `bashcut-plugins`, what to skip, and why. Based on
FilmCraft's engine command ids (~1,000, read from `crates/engine/src`, 2026-10-20). Re-check with
`command_list` once `filmcraft-cli` is built.

Verdicts: **migrate** (now), **later** (worth it, blocked on something), **skip** (FilmCraft already
does it, or it needs something we decided not to build yet).

## FilmCraft already has

| Area | Commands |
|---|---|
| Transcript and text-based editing | `transcript.generate / inspect / search / select / lift / extract / removeFillers / removePauses / createCaptions` (Whisper on candle; feature `whisper`, off in release builds: gap G19) |
| Captions | `captions.add / import / export / list / setText / merge / move …`, SRT / VTT / SCC / TTML, burn-in |
| Text and graphics | `graphics.*` (102 ids), `.fcgt` templates (`graphics.template.install / apply / set`) |
| Colour | `lumetri.*`, `lut.import / list`, `lumetri.applyMatch`, `scopes.read` |
| Sound | `essentialSound.autoMatch` (loudness), `essentialSound.generateDucking`, mixer, `clip.audioGain`, `clip.remix` |
| Voice | `tts.*` (formant voices, Kokoro English with `neural-voices`; no Vietnamese) |
| Cutting aids | `clip.sceneEditDetection`, `clip.synchronize` (audio sync), multicam, markers, `clip.speedDuration`, `clip.frameHold` |
| Effects | 93 video effects, 84 transitions, masks, keyframes (`effects.*`) |
| Export | `export.*`, presets, queue |

## Missing in FilmCraft

Beat detection as a command (remix analyses beats internally only), a word-level speech map / media
digest, contact sheets, shot descriptions, a library of reusable items, an agent knowledge store, edit
plans (`plan.cut`), review packets, image generation, Vietnamese TTS, caption translation, plugin hosting.

## Skills (`bashcut-agent-kit/skills`)

| Skill | Verdict | Why |
|---|---|---|
| `edit-workflow` | **migrate** (lean) | The spine of every edit. Rewrite small: stages and gates as text; no `plan.cut`, `run checklist` or review packets, which FilmCraft lacks; use `command_batch` instead of one-plan apply |
| `rough-cut` | **migrate** | FilmCraft's text-based editing (`transcript.search / select / lift / extract`) covers cut-by-quote directly; highest value per hour |
| `captions-text` | **migrate** | `captions.*`, `transcript.createCaptions`, graphics text and templates cover it; keep the Vietnamese font and readability rules |
| `color-grade` | **migrate** | Lumetri, LUT import, `scopes.read`, `applyMatch`; `grade.py` (measures frames, writes `.cube`) runs unchanged |
| `audio-mix` | **migrate** | Essential Sound (loudness auto-match, ducking), mixer, gain; `fetch_audio.py` runs unchanged |
| `footage-survey` | **migrate** | `transcript.generate`, `clip.sceneEditDetection`, `render_frame`; contact sheets built by a script from rendered frames |
| `review` | **migrate** (lean) | `render_frame`, `scopes.read`, loudness meters; the critic brief is knowledge; drop review packets / `review verify` |
| `stock-images` | **migrate** | Its fetch scripts are standalone (official APIs) and land files for `media_import` |
| `effects` | **migrate** after the command inventory | Needs a name map from BashCut's 39 transitions and native effects to FilmCraft's 93 / 84 ids |
| `beat-cut` | **later** | Word-edge rhythm works now; music beats need a beat-detection plugin or an engine command |
| `voiceover` | **later** | English works with `tts.*`; Vietnamese waits for the VieNeu plugin |
| `motion-graphics` | **later** | Needs a `graphics.render` plugin (hyperframes); simple cards can be `.fcgt` templates meanwhile |
| `visual-plan` | **later** | Depends on image generation (plugin) |
| `style-study` | **later** | Useful, but builds on survey, beat and colour measurement landing first |
| `setup` | **migrate** (rewrite) | Becomes "register `filmcraft-cli mcp` in Claude Code / Codex"; part of the kit scaffold |
| `library` | **skip** | BashCut's library has no FilmCraft counterpart; presets and templates exist but differ. Revisit as a "presets" skill |
| `self-learn` | **skip** | Built on BashCut's `knowledge.*` store; nothing to write to |

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
| `whisper-captions` | **skip** | FilmCraft has Whisper built in (`transcript.generate`); keep its vocabulary / proper-name tips in `captions-text` |
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
