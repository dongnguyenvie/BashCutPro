# Quality bench: Pro vs BashCut, 2026-10-20

Rubric: [bench/rubric.md](https://github.com/dongnguyenvie/bashcutpro-agent-kit/blob/main/bench/rubric.md) in the kit.
Numbers and small contact sheets only; no footage. **All 6 jobs on real Creative Commons footage, both sides, with
agent steps** (the second run; the [first](2026-10-20.md) was Pro only on generated clips). Read [How it was
run](#how-it-was-run) before the numbers: each side is judged partly by its own reviewer.

**Result: Pro wins 1, ties 1, loses 4.** Every loss has a `quality` issue (see [Where Pro loses](#where-pro-loses)).

## Summary

| Job | Pro | BashCut | Winner |
|---|---|---|---|
| Talking head to a vertical short | 87 | 75 | pro |
| Daily vlog to 60 s | 82 | 100 | bashcut |
| Podcast to three clips | 69 | 78 | bashcut |
| B-roll montage to music | 100 | 100 | tie |
| Tutorial with captions | 55 | 66 | bashcut |
| Vietnamese subtitles | 83 | 97 | bashcut |

A job is won by at least 3 points of the total (0–100); closer is a tie.

## Owner's blind side-by-side

**Pending: the pick is the owner's.** One contact sheet per job in [`2026-10-20-real-footage/`](2026-10-20-real-footage/),
made by `score.py blind` (sides shuffled at random per job): side A's strips on top, a white gap, side B's strips
below; 8 evenly spaced frames per output (the podcast sheet has three outputs per side). Pick A or B per job from the
sheet (or the full outputs) **before** opening [`blind-key.json`](2026-10-20-real-footage/blind-key.json), then fill
in the table.

| Job | Sheet | Pick | A was |
|---|---|---|---|
| Talking head to a vertical short | [sheet](2026-10-20-real-footage/talking-head-short.jpg) | pending | – |
| Daily vlog to 60 s | [sheet](2026-10-20-real-footage/vlog-60s.jpg) | pending | – |
| Podcast to three clips | [sheet](2026-10-20-real-footage/podcast-3-clips.jpg) | pending | – |
| B-roll montage to music | [sheet](2026-10-20-real-footage/broll-montage.jpg) | pending | – |
| Tutorial with captions | [sheet](2026-10-20-real-footage/tutorial-captions.jpg) | pending | – |
| Vietnamese subtitles | [sheet](2026-10-20-real-footage/vietnamese-subtitles.jpg) | pending | – |

## Per job

### Talking head to a vertical short

| Category | Pro | BashCut |
|---|---|---|
| sound | 100 (-14.0 LUFS, -1.3 dBTP) | 100 (-14.4 LUFS, -1.9 dBTP) |
| captions | 100 (43 cues, 100% at or under 20 cps, 0% over 25) | 60 (48 cues, 75% at or under 20 cps, 15% over 25) |
| edit | 39 (39% of 23 cuts on a word edge) | 15 (15% of 27 cuts on a word edge) |
| picture | 100 (0 black frames, 0.0 s frozen) | 100 (0 black frames, 0.0 s frozen) |
| review | 97 (1 minor) | 100 (9 polish) |
| length | 100 (59.7 s) | 100 (48.0 s) |
| render time (not scored) | 67 s | 8 s |

### Daily vlog to 60 s

| Category | Pro | BashCut |
|---|---|---|
| sound | 70 (-14.0 LUFS, -0.4 dBTP) | 100 (-14.0 LUFS, -1.9 dBTP) |
| captions | 100 (12 cues, 100% at or under 20 cps, 0% over 25) | 100 (11 cues, 100% at or under 20 cps, 0% over 25) |
| edit | – (not measured) | – (not measured) |
| picture | 45 (0 black frames, 2.7 s frozen) | 100 (0 black frames, 0.0 s frozen) |
| review | 97 (1 minor, 1 polish) | 100 (1 polish) |
| length | 100 (60.0 s) | 100 (59.5 s) |
| render time (not scored) | 81 s | 12 s |

### Podcast to three clips

| Category | Pro | BashCut |
|---|---|---|
| sound | 100 (-14.1 LUFS, -1.4 dBTP; -14.0 LUFS, -1.4 dBTP; -14.0 LUFS, -1.4 dBTP) | 100 (-14.3 LUFS, -1.9 dBTP; -14.1 LUFS, -1.9 dBTP; -14.3 LUFS, -1.9 dBTP) |
| captions | 55 (16 cues, 56% at or under 20 cps, 12% over 25; 17 cues, 88% at or under 20 cps, 0% over 25; 19 cues, 47% at or under 20 cps, 16% over 25) | 32 (43 cues, 35% at or under 20 cps, 33% over 25; 37 cues, 76% at or under 20 cps, 8% over 25; 52 cues, 50% at or under 20 cps, 23% over 25) |
| edit | 33 (50% of 2 cuts on a word edge; 50% of 2 cuts on a word edge; 0% of 2 cuts on a word edge) | 56 (50% of 12 cuts on a word edge; 60% of 10 cuts on a word edge; 58% of 12 cuts on a word edge) |
| picture | 100 (0 black frames, 0.0 s frozen; 0 black frames, 0.0 s frozen; 0 black frames, 0.0 s frozen) | 100 (0 black frames, 0.0 s frozen; 0 black frames, 0.0 s frozen; 0 black frames, 0.0 s frozen) |
| review | 45 (2 blocker, 5 minor; 2 minor; 3 blocker, 6 minor) | 100 (3 polish; 3 polish; 3 polish) |
| length | 100 (35.7 s; 40.9 s; 37.4 s) | 100 (33.0 s; 34.5 s; 41.4 s) |
| render time (not scored) | – | – |

### B-roll montage to music

| Category | Pro | BashCut |
|---|---|---|
| sound | 100 (-14.0 LUFS, -4.6 dBTP) | 100 (-14.0 LUFS, -4.5 dBTP) |
| captions | – (no captions in this job) | – (no captions in this job) |
| edit | – (not measured) | – (not measured) |
| picture | 100 (0 black frames, 0.0 s frozen) | 100 (0 black frames, 0.0 s frozen) |
| review | 100 (no findings) | 100 (1 polish) |
| length | 100 (29.9 s) | 100 (30.0 s) |
| render time (not scored) | 75 s | 5 s |

### Tutorial with captions

| Category | Pro | BashCut |
|---|---|---|
| sound | 100 (-14.1 LUFS, -1.4 dBTP) | 100 (-14.7 LUFS, -2.0 dBTP) |
| captions | 74 (46 cues, 74% at or under 20 cps, 0% over 25) | 70 (99 cues, 75% at or under 20 cps, 5% over 25) |
| edit | 50 (50% of 4 cuts on a word edge) | 34 (34% of 58 cuts on a word edge) |
| picture | 0 (1 black frames, 29.1 s frozen) | 0 (0 black frames, 13.4 s frozen) |
| review | 0 (64 minor) | 100 (32 polish) |
| length | 100 (191.7 s) | 100 (170.7 s) |
| render time (not scored) | 137 s | 26 s |

### Vietnamese subtitles

| Category | Pro | BashCut |
|---|---|---|
| sound | 100 (-14.0 LUFS, -1.2 dBTP) | 100 (-14.1 LUFS, -1.9 dBTP) |
| captions | 100 (29 cues, 100% at or under 20 cps, 0% over 25) | 100 (42 cues, 100% at or under 20 cps, 0% over 25) |
| edit | 100 (no cuts) | 100 (no cuts) |
| picture | 80 (2 black frames, 0.0 s frozen) | 80 (2 black frames, 0.0 s frozen) |
| review | 19 (27 minor, 5 polish) | 100 (2 polish) |
| length | – (120.0 s (no target)) | – (120.0 s (no target)) |
| render time (not scored) | 105 s | 19 s |


## Where Pro loses

Each loss has an issue with the label `quality`:

- **Daily vlog to 60 s** (82 vs 100): [#185](https://github.com/dongnguyenvie/BashCutPro/issues/185). Two causes:
  - The true peak went over the limit (−0.4 dBTP). The AAC encoder overshoots the limiter; [#184](https://github.com/dongnguyenvie/BashCutPro/issues/184) is fixed by [#186](https://github.com/dongnguyenvie/BashCutPro/pull/186), which measures the exported file and re-exports with a lower limit when it is over. The same project then measured −1.1 dBTP.
  - A 2.7 s frozen stretch at the end, in our render; the source is not frozen there.
- **Podcast to three clips** (69 vs 78): [#195](https://github.com/dongnguyenvie/BashCutPro/issues/195). bashcut-batch's review gate after an agent step only reviewed the active sequence (clip 2), so clips 1 and 3 went out with 5 blocking caption-speed findings that the fix rounds would have caught. Each clip also has only 2 cuts.
- **Tutorial with captions** (55 vs 66): [#193](https://github.com/dongnguyenvie/BashCutPro/issues/193). 53 of 64 review findings say "the picture does not move" for a still screen under narration, which is normal for a screen recording. The cut also stays at 191.6 s of 192 s.
- **Vietnamese subtitles** (83 vs 97): [#196](https://github.com/dongnguyenvie/BashCutPro/issues/196). Same cause as #193: 25 frozen warnings on a locked-off interview. Everything else ties.

The one win, **talking head** (87 vs 75), is on captions (100 vs 60: BashCut's captions read faster than 20
characters a second in a quarter of the cues) and edit (39% vs 15% of cuts on a word edge).

## How it was run

- **Machine and builds.** Both sides ran one job at a time on the same Mac (Apple silicon).
  - Pro: debug builds of `main` at the start of the run (dev profile).
  - BashCut: the Swift app from `dongnguyenvie/BashCut` `main`, built locally.
  - #178 and #184 were fixed during or after the run (#179, #186), so **every Pro run used the binaries from before those fixes**. Current `main` would score Pro somewhat higher:
    - The vlog's true peak would be about −1.1 dBTP.
    - Pro's review of the saved projects gives fewer frozen warnings: tutorial 36 instead of 53, Vietnamese 18 instead of 25. That is still enough for the review category to lose (#193).
- **Agent.** Both sides used the real `claude` CLI with `claude-opus-5-5`, the same brief per job (kit `bench/jobs.json`) and the same tools allowed.
  - Pro: through `bashcut-batch` with an agent step: the BashCutPro kit as a plugin, Pro's MCP, `fixRounds: 2`, review gate `bashcut.review.run`. The agent exported itself where the job asks for several outputs.
  - BashCut: through its own CLI and MCP with the BashCut kit as a plugin, then `bashcut review run` and at most 2 fix rounds on errors, as the batch does for Pro. Then the export with the job's preset and `--normalize-audio`.
  - BashCut's export refuses a draft that has not passed its own audit. That refusal went back to the agent as a fix round (talking head, b-roll).
- **Scoring** with the kit's [`score.py`](https://github.com/dongnguyenvie/bashcutpro-agent-kit/blob/main/bench/score.py) (weights: sound 20, captions 20, edit 20, picture 15, review 15, length 10). Every measurement is taken on the exported files, the same way for both sides, except where noted:
  - **Sound:** ffmpeg `ebur128` (external oracle) on the output.
  - **Picture:** ffmpeg `blackdetect` / `freezedetect` (defaults) on the output.
  - **Captions:** the SRT sidecar when the export wrote one. Otherwise an SRT built from the text clips of the busiest text track (burned-in captions); this applies to both sides.
  - **Edit:** cut points from each side's timeline, checked against word edges from a neutral reference: voice-activity energy spans of the source audio, not either side's transcript.
  - **Review:** **each side's own reviewer** (Pro `bashcut.review.run`, BashCut `bashcut review run`), mapped through the rubric's severities. BashCut's reviewer reports mostly polish-level findings and Pro's reports warnings, so this category leans toward BashCut. It explains most of the Pro losses above, and #193 makes Pro's reviewer quieter on still pictures under speech.
  - **Length:** the job's target range.
- **Podcast on BashCut.** BashCut has one timeline per project, so its agent built and exported the three clips one after another. Each clip's timeline was recovered by undoing back to the revision its export recorded (caption counts match the export history).
  - The BashCut app went down once during that job, most likely because the disk filled up. It was relaunched, and the agent carried on in the same session; its 31 minutes include about 10 minutes of waiting.

### Agent usage (not scored)

| Job | Pro | BashCut |
|---|---|---|
| Talking head | 2 rounds, 132 turns, $10.61, 18 min | 2 rounds, 73 turns, $7.91, 8 min |
| Daily vlog | 1 round, 172 turns, $4.00, 9 min | 1 round, 83 turns, $3.32, 11 min |
| Podcast | 1 round, 75 turns, $1.37, 5 min | 1 round, 115 turns, $8.95, 31 min |
| B-roll montage | 1 round, 69 turns, $1.79, 3 min | 2 rounds, 100 turns, $7.66, 13 min |
| Tutorial | 2 rounds, 137 turns, $8.70, 10 min | 1 round, 54 turns, $2.40, 8 min |
| Vietnamese | 1 round, 41 turns, $1.28, 4 min | 1 round, 43 turns, $1.48, 5 min |

Render time (in the per-job tables) is not scored either. Pro's numbers come from a debug build. BashCut's minutes for
the podcast include the app outage.

## Footage and licences

Only the minutes each job needed were used. Footage was kept outside the repo and never committed. The contact sheets
are frames of these works and of the two edits made from them.

| Job | Source | Author | Licence |
|---|---|---|---|
| Talking head | [InXjOKOhzKc](https://youtu.be/InXjOKOhzKc), 0–180 s | Kizzume | CC BY (YouTube Creative Commons) |
| Daily vlog | [0pW_Gx2CAc0](https://youtu.be/0pW_Gx2CAc0), 0–240 s | Marl Gabileo | CC BY |
| Podcast | [DmWH0tYnxHk](https://youtu.be/DmWH0tYnxHk), 300–780 s | vlogwrap | CC BY |
| B-roll montage | [SBrXXKPq0CE](https://youtu.be/SBrXXKPq0CE) | Free Raw Footage | CC BY |
| B-roll montage | [Gx07PCvirXo](https://youtu.be/Gx07PCvirXo) | ZU FILM & PHOTO | CC BY |
| B-roll montage | [99HTI9almbM](https://youtu.be/99HTI9almbM) | mynbalaofficial | CC BY |
| B-roll montage | [xu_4JtZ5pWw](https://youtu.be/xu_4JtZ5pWw) | Sponge Can | CC BY |
| B-roll montage (music) | "Cyborg Ninja", [incompetech.com](https://incompetech.com) | Kevin MacLeod | CC BY 4.0 |
| Tutorial | [Qf3JNIO8zVY](https://youtu.be/Qf3JNIO8zVY), 192 s | MacMingle | CC BY |
| Vietnamese subtitles | [kDY8n-qxUek](https://youtu.be/kDY8n-qxUek), 0–120 s | Burrup and Away | CC BY |

Music the agents added on their own (daily vlog):
- **Pro:** "A new world" by zero-project, [Jamendo track 2185505](https://www.jamendo.com/track/2185505) via Openverse, CC BY.
- **BashCut:** "Calm Acoustic Guitar" from its music library ([Jamendo track 1996840](https://www.jamendo.com/track/1996840)), **CC BY-NC-ND**. That is fine for a bench but not for a published or commercial vlog. BashCut's library should flag the NC-ND licence before an agent uses it.
