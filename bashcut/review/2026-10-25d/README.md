# Agent Panel Review — 2026-10-25d (FX45 after-fix verification)

**Purpose:** Verify the four bugs fixed in FX45 against the same LIVE_FAKE states as TMUI3 (2026-10-25c).  
**Date:** 2026-10-26  
**Build SHA:** `bba65384` (FX45, fix/bc-fx45-live-ui on top of origin/main `6e4f38d2`)  
**Binary version:** `bashcutpro 0.12.1`  
**Platform:** macOS aarch64 (M4)  
**Pixel ratio:** 2× (Retina) — all cropped images are logical × 2 in pixels  
**Crop:** Agent panel only

## Bugs fixed

| # | Bug | Fix |
|---|---|---|
| L1 | Model row / header chip not visible at 340 px or 240 px | `setup.rs`: `model_row()` moved out of the header `horizontal()` — always on its own row below the agent+conversation row; no longer squeezed to zero by the conversation picker. |
| L2 | Assistant name row (AGU2 Sparkle + agent name) missing when tools precede first assistant block | `transcript.rs`: `run_start` now walks back to the agent-run's start and checks whether an earlier assistant block exists in the run, so `User → Tool(s) → Assistant` correctly shows the name label. |
| L3 | "Codex (not installed)" stays English in vi | Already fixed in main (`{agent} (not installed)` → `{agent} (chưa cài)` vi.tsv row existed since FX44-era); shots 35/36 in TMUI3 showed it correctly. No FX45 change needed. |
| L4 | vi turn summary "giây" may clip at 240 px | `vi.tsv`: `{n} s` → `{n} s` (same as EN; "s" universally understood, shorter); `{m} min {s} s` → `{m} phút {s} s`. |

## How shots were taken

```sh
# Binary built from fix/bc-fx45-live-ui (bba65384)
export FILMCRAFT_CONTROL_TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(32))')"
export FAKE_LOG=/tmp/fx45/fake/fake.log
export BASHCUT_CLAUDE=/tmp/fx45/fake/claude  # LIVE_FAKE wrapper
export BASHCUTPRO_DATA_DIR=/tmp/fx45/data

target/release/filmcraft --control 9899 &

# Drove via control channel (Python):
#   file.openDemoProject + file.saveAs → saved project (removes "not saved" card)
#   bashcut.ui.section {section: agent, open: true}
#   bashcut.agent.send {text: "do the tools"} → tools + assistant name + turn summary chip
#   ui.click {id: bashcut.agent.model} → model picker open
#   bashcut.agent.send {text: "cost please"} → turn summary with vi "s"
#   bashcut.agent.send {text: "stream it"} (hold) → header chip during a running turn
```

## Files

| File | State | Width | Lang |
|---|---|---|---|
| `fx45-01-tools-done-en-340.png` | Post-tools turn: name row + chip in turn summary | 340 px | EN |
| `fx45-01-tools-done-en-240.png` | Post-tools turn | 240 px | EN |
| `fx45-01-tools-done-vi-340.png` | Post-tools turn | 340 px | VI |
| `fx45-01-tools-done-vi-240.png` | Post-tools turn | 240 px | VI |
| `fx45-02-model-picker-en-340.png` | Model picker open (showing model list) | 340 px | EN |
| `fx45-02-model-picker-en-240.png` | Model picker open | 240 px | EN |
| `fx45-02-model-picker-vi-340.png` | Model picker open | 340 px | VI |
| `fx45-02-model-picker-vi-240.png` | Model picker open | 240 px | VI |
| `fx45-03-turn-summary-en-340.png` | Turn summary: "9 steps · 1 s · $0.12 · Fake" | 340 px | EN |
| `fx45-03-turn-summary-en-240.png` | Turn summary | 240 px | EN |
| `fx45-03-turn-summary-vi-340.png` | Turn summary: "9 bước · 1 s · $0.12 · Fake" (s not giây) | 340 px | VI |
| `fx45-03-turn-summary-vi-240.png` | Turn summary (narrow) | 240 px | VI |
| `fx45-04-header-chip-en-340.png` | Mid-turn: model picker row visible, "Working · Writing…" | 340 px | EN |
| `fx45-04-header-chip-en-240.png` | Mid-turn | 240 px | EN |
| `fx45-04-header-chip-vi-340.png` | Mid-turn | 340 px | VI |
| `fx45-04-header-chip-vi-240.png` | Mid-turn | 240 px | VI |
