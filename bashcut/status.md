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
- Upstream-sync tooling (#38): `bashcut/scripts/upstream.sh` (`footprint`, `merge` with rerere, `setup`) and the
  Hooks / Carried edits ledger in `UPSTREAM.md`; `footprint --pr` is the check before every PR.
- First upstream sync with the tooling (#63): already up to date, recorded in `UPSTREAM.md` "Sync records".
- Branding out of hot upstream files (#39): `dialogs.rs`, `main.rs`, `header.rs` carry one hook each into
  `bashcut_brand`; cold `brand.rs`, `links.rs`, `build.rs` and packaging stay as listed carried edits.
- Batch runner (#45): `crates/bashcut-batch` + `apps/bashcut-batch` ([batch.md](batch.md)); tested with a fake
  `filmcraft-cli`, live run pending.
- Agent auto-connect (#51): the window publishes `automation.json`, `bashcut-mcp` bridges or runs headless with a
  project lock and a write guard ([agent-connect.md](agent-connect.md)); runtime "Let agents edit" switch left for #35.
- `bashcut-plugin-host` design note: [plugin-host.md](plugin-host.md) (#23), approved 2026-10-20.
- In-app UI U0 (#34): `crates/ui-egui/src/bashcut_ui/` rail + side panel, state, strings (vi), hooks H5–H9, H11
  (ledger in `UPSTREAM.md`). Build + tests pending (build pass).
- Engine measurements (#36): `crates/bashcut-analysis` (loudness, beats, picture checks) and `bashcut.audio.loudness`,
  `bashcut.audio.beats`, `bashcut.review.pictureChecks` in `crates/engine/src/bashcut/`; kit skills use them. Build + tests
  pending (build pass).
- U1 (#31): `crates/bashcut-plugin-host` (manifests, registry, git installs into `<data dir>/bashcut-plugins/`,
  settings + OS key store, runner with progress, `bashcut-result/1`, hand-off, cancel, timeout, capped output) and
  the Plugins and Runs sections with `bashcut.plugins.*` commands; hook H4. Build + tests + live check pending.
- Atomic edits (#44): `bashcut.batch` runs many commands as one undo step, all or nothing (no hook); plugins' `mcp.py`
  and kit skills use it with `command_batch` as fallback. Build + tests pending (build pass).
- Command palette (#52): `bashcut_ui/palette.rs`, Cmd/Ctrl+Shift+P, over commands, plugin actions, looks, profiles,
  templates and kit recipes, ranked by recent use; no new hook. Build + tests + live check pending.
- Agent reads (#50): `bashcut.timeline.summary` (seconds, non-default fields only), `changed` + `createdId` from our edit
  commands, typed error codes and units notes applied to every `bashcut.*` command in `bashcut/errors.rs`; kit reads
  the summary. Build + tests pending (build pass).
- U2 (#35): `crates/bashcut-agent` (Claude Code stream-json turns with resume, transcript, conversations in
  `<project>.fcproj.bashcut/conversations/`, gates in `plan.json`) and the Agent section with `bashcut.agent.*`
  commands; agent plugins (`kind: "agent"`); Runs ▸ Send to agent sends hand-offs. Build + tests + live check
  pending. Follow-up: the kit's edit-workflow skill must write gates to `<project>.fcproj.bashcut/plan.json`.

## Next

1. **Live tests**: every skill and plugin was checked statically only (kit `check.py`, plugin unit tests with
   fakes). Run each issue's "Done when" against a `filmcraft-cli` build: rough-cut on a talking-head clip,
   Vietnamese captions, grade + scopes, mix loudness, vieneu-tts line on the timeline, image-gen batch, hyperframes
   card with alpha. Confirm `scopes.read` scale, `render_frame` max side, the Lumetri intensity param name.
2. Next upstream sync on the next tag or in 1–2 weeks (`upstream.sh merge`, record in `UPSTREAM.md`). Then the
   in-app UI ([in-app-ui.md](in-app-ui.md)): U4 #37; engine analysis U3 #36 in parallel.
   Batch and quality (owner goals): batch runner #45 → Batch section #46, templates #42, output profiles #43,
   atomic edits #44, looks #47, voice polish #48, word captions #49, palette #52, agent reads #50, auto-connect #51,
   model manifest #53, smoke checks #54. Tracking #40.
3. Vlog library pack (text styles, stickers, transitions) once `.fcgt` conversions exist.

## Open questions

- Which features from BashCut matter most for BashCutPro's first release?
- Windows / Linux targets: which plugins must run there (VieNeu, Whisper MLX are macOS-only in BashCut;
  FilmCraft's own Whisper runs on candle).

## Smoke checks

One row per run of `bashcut/scripts/smoke.sh` / `smoke.ps1` ([smoke.md](smoke.md)): date, machine, commit, the summary line.

| Date | Machine | Commit | Result |
|---|---|---|---|
