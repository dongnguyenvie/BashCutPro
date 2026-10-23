# Status

Update this file when you finish a step or the plan changes. Keep "Next" short and ordered.

_Last updated: 2026-10-23_

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
- Doubtful transcript text (#177): `bashcut.transcript.generate` (transcribe, then drop Whisper's repeated-text loops
  on footage with no speech) and `bashcut.transcript.clean` in `crates/engine/src/bashcut/transcript_clean.rs`; the
  batch runner cleans after every transcribe step. Live: whisper-base on 25 s of wind and birds wrote 106 words of
  "I'm going to go back to the next video." → 0 words and a no-speech warning; a 68-word TTS talk kept all 68.
- Bench vlog fixes (#185): `bashcut.profiles.apply`'s true-peak check re-exports up to 3 times with the loudness target
  0.25 LU lower each time (a lower ceiling made AAC overshoot worse), keeps the best attempt and warns if none is
  under; `bashcut.review.run` makes a `longStill` with nothing said over it a warning in vertical edits (the vlog's
  last 6 s were a near-still punch-in under the end card, reported as info).
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
- Review gate (#57): `bashcut.review.run` measures sound, picture, captions and edit, writes findings as Review markers
  (one undo step), scores each category and returns `verdict` (`needsFix` on blocking findings); kit review and critic
  start from it. Not measured yet: music over speech, caption safe area and contrast. Build + tests pending.
- Looks (#47): 26 curated looks (Lumetri params with purpose, tags, one knob and safe range) as `bashcut.looks.*`, the
  Looks section with engine thumbnails, golden + damage checks; no new hook. Build + tests + golden bless + live check
  pending (build pass).
- Batch resources (#61): `crates/bashcut-batch/src/governor.rs` sets concurrency from free RAM and cores, stops steps over
  the memory cap for a later retry, pauses on low disk, keeps the machine awake, reports peak memory and throughput.
  Build + tests pending (build pass).
- Batch section (#46): `bashcut_ui/{batch,batch_panel}.rs` builds, runs, watches and retries `bashcut-batch` jobs
  (drop zone through hook H12, steps from commands / plugin actions / recipes, output profiles, learned step times,
  Open to fix / Retry / Reveal, presets, recent jobs); "Run on many files…" from plugins and agent tool rows. Build +
  tests + live check pending; output thumbnails not yet.
- Library (#68): Library section with Mine (library plugin) and Online (resources plugin `search` / `download`, added in
  bashcutpro-plugins#14): licence before download, add to bin / place at playhead by the app's own commands, opt-in
  credits; native egui (plugin views #37 can wrap it). Build + tests + live check pending (build pass).
- User guide (#65): [guide/](guide/README.md), English and Vietnamese: getting started (install, first-run setup), the
  rail and palette, batch (presets, statuses, fixing a needs-fix item), output profiles, looks, library, the agent
  (Claude Code, Codex, terminal agents), more tools, troubleshooting. Screenshots pending (build pass).
- U4 (#37): `bashcut-view/1` plugin views (live JSON lines from a `view` action, buttons run actions with the form's
  values, `bashcut.plugins.view` / `click`), the Codex adapter (`codex exec --json`), pop-out windows remembered per
  monitor; no new hook. Build + tests + live check pending; translate-captions view port is a follow-up in plugins.
- Build pass, kit + plugins + bench (mac 02): kit catalogue refreshed from a build's `command_list` and skill examples
  fixed (#113), plugins' live paths run without paid keys (#117), first bench scorecard, Pro side only, 3 of 6 jobs
  on generated clips ([bench/2026-10-20.md](bench/2026-10-20.md)); engine follow-ups #127 (polish), #130 (word captions).
- Bench #58, full scorecard: all 6 jobs on real Creative Commons footage, Pro (bashcut-batch + agent) against the
  Swift BashCut with the same brief and rubric ([bench/2026-10-20-real-footage.md](bench/2026-10-20-real-footage.md)):
  Pro wins 1, ties 1, loses 4. Loss issues #185 (render freeze), #193/#196 (frozen-under-speech review noise),
  #195 (batch reviews only the active sequence); blind contact sheets wait for the owner's pick.
- Bench #58 re-score (round 2, after #198, #201, #202, #207, #210, #212 and kit #43-#54): all 6 Pro jobs re-run with
  `bashcut-batch --kit` ([bench/2026-10-20-rescore.md](bench/2026-10-20-rescore.md)): Pro wins 3, ties 3, loses
  none. Follow-ups #219 (tutorial needs ~150 agent turns) and #220 (owner: music licence filter in the kit).
- **UX round (2026-10-23)**: audit [ux-audit-2026-10.md](ux-audit-2026-10.md) (UX0), then UX1–UX6 (tokens, hints,
  plugin cards, run outcome in the card, Runs rows with error / Retry / Reveal), UX9–UX12 (rail, language menu,
  Batch step form, Batch start-from row), I18N1 (manifest vi titles and help), FX1/FX2 in the plugins repo, PL1
  polish (danger colour in the palette, '›' instead of the missing '▸' in run titles, localized palette plugin
  rows), and the upstream FilmCraft v0.10 sync (UP1). Still open: audit §9 "Not ticketed" P3s (R2, R4, R5, A4, A5,
  B4, L1, P6, X6); Batch step required params; Step storing plugin / action / params; choice labels in vi; a
  finished-run toast; control-channel self-start (needs an owner / security decision).

## Next

1. **Live tests**: every skill and plugin was checked statically only (kit `check.py`, plugin unit tests with
   fakes). Run each issue's "Done when" against a `filmcraft-cli` build: rough-cut on a talking-head clip,
   Vietnamese captions, grade + scopes, mix loudness, vieneu-tts line on the timeline, image-gen batch, hyperframes
   card with alpha. Confirm `scopes.read` scale, `render_frame` max side, the Lumetri intensity param name.
2. Next upstream sync on the next tag or in 1–2 weeks (`upstream.sh merge`, record in `UPSTREAM.md`). Then the
   in-app UI ([in-app-ui.md](in-app-ui.md)): U4 #37 done; engine analysis U3 #36.
   Batch and quality (owner goals): batch runner #45, Batch section #46 (done), templates #42, output profiles #43,
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
| 2026-10-20 | geekomT (Windows 11, AMD64) | 1c720a2a (dev build) | `smoke: 6 passed, 0 failed` with `-NoIsa`; ISA scan of release binaries pending (release checklist) |
| 2026-10-23 | geekomT (Windows 11, AMD64) | 4ec3fec9 + T6b (release, `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`) | `smoke: 8 passed, 0 failed` incl. the ISA scan of `filmcraft.exe` and `bashcutpro.exe`; also passes with another BashCutPro open (cli-razor on the headless engine, no bridge note) (T6b) |
