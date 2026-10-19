# Status

Update this file when you finish a step or the plan changes. Keep "Next" short and ordered.

_Last updated: 2026-10-20_

## Done

- Imported FilmCraft (upstream `105c09bd`, v0.8.0) and added `UPSTREAM.md`.
- All GitHub Actions switched to manual (`fc3cd05c`).
- Created the empty repos `bashcutpro-agent-kit` and `bashcutpro-plugins` (cloned beside this repo).
- Wrote this folder (`bashcut/`): overview, decisions, status.
- Rebrand (#25): builds show BashCutPro's name, icon, wordmark and links instead of the ArtCraft marks
  ([rebrand.md](rebrand.md), [brand/](brand/README.md)). Left: DMG window, Linux app id, web favicon.
- Command inventory (#3): 701 commands in [commands.json](commands.json) / [commands.md](commands.md), per-skill
  needs in [skill-needs.md](skill-needs.md), [migration.md](migration.md) corrected. `filmcraft-cli` builds with and
  without `--features whisper`.
- Kit (`bashcutpro-agent-kit`): scaffold + `setup` (#4), `check.py` validates every command id against
  `scripts/commands.json`; 30 skills: edit-workflow, rough-cut, captions-text, effects, color-grade, audio-mix,
  footage-survey, review, beat-cut, style-study, voiceover, stock-images, visual-plan, motion-graphics and 16 `vlog-*`
  recipes (#5–#7, #9–#12, #15–#22); sub-agents surveyor, critic, voice.
- Plugins (`bashcutpro-plugins`): translate-captions (#8), vieneu-tts (#13), resources (#14, credits opt-in),
  image-gen (#20), hyperframes (#21). Plugin format decision copied into [decisions.md](decisions.md).
- `bashcut-plugin-host` design note: [plugin-host.md](plugin-host.md) (#23), approved 2026-10-20.

## Next

1. **Live tests**: every skill and plugin was checked statically only (kit `check.py`, plugin unit tests with
   fakes). Run each issue's "Done when" against a `filmcraft-cli` build: rough-cut on a talking-head clip,
   Vietnamese captions, grade + scopes, mix loudness, vieneu-tts line on the timeline, image-gen batch, hyperframes
   card with alpha. Confirm `scopes.read` scale, `render_frame` max side, the Lumetri intensity param name.
2. Upstream-sync hygiene first: footprint check + hook ledger (#38), branding out of hot files (#39). Then the
   in-app UI ([in-app-ui.md](in-app-ui.md)): U0 #34 → U1 #31 → U2 #35 → U4 #37; engine analysis U3 #36 in parallel.
   Batch and quality (owner goals): batch runner #45 → Batch section #46, templates #42, output profiles #43,
   atomic edits #44, looks #47, voice polish #48, word captions #49, palette #52, agent reads #50, auto-connect #51,
   model manifest #53, smoke checks #54. Tracking #40.
3. Vlog library pack (text styles, stickers, transitions) once `.fcgt` conversions exist.

## Open questions

- Which features from BashCut matter most for BashCutPro's first release?
- Windows / Linux targets: which plugins must run there (VieNeu, Whisper MLX are macOS-only in BashCut;
  FilmCraft's own Whisper runs on candle).
