# Agent Panel Review — 2026-10-26 (FX47 model list + review cleanup)

**Purpose:** Verify FX47 fixes: updated model list (5-5 ids, "Sonnet" not "Sonnet (default)"), Show
link visible under tool rows. Also re-captures the three shots that FX45's review (2026-10-25d)
had wrong (left edge cropped off, duplicate blobs for some files).  
**Date:** 2026-10-26  
**Build SHA:** (FX47 fix/bc-fx47-models, built from f3516bbe origin/main + FX47 changes)  
**Binary version:** `bashcutpro 0.12.1`  
**Platform:** macOS aarch64 (M4)  
**Pixel ratio:** 2× (Retina) — all cropped images are logical × 2 in pixels  
**Crop:** Agent panel only (full panel width from left edge)

## Fixes verified

| # | Issue | Verified |
|---|---|---|
| F1 | Model picker listed stale 4-5 ids | New: `claude-sonnet-5-5`, `claude-opus-5-5`, `claude-haiku-5-5`, `claude-fable-5-1` |
| F2 | "Sonnet (default)" misleading next to "Default" entry | Now "Sonnet" only; Default is its own entry |
| F3 | FX45 shots had left edge cropped (labels mid-word) | Re-captured with full panel width |
| F4 | Show link under timeline.razor / timeline.ripple | Visible in tools-done shots (code unchanged, crop was the issue) |
| F5 | Duplicate blobs in 2026-10-25d (fx45-model-picker-*, fx45-tools-done-*, fx45-turn-summary-en-340) | Deleted from 2026-10-25d |

## How shots were taken

```sh
# Binary built from fix/bc-fx47-models worktree (bcp-fx47), shared target
export FILMCRAFT_CONTROL_TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(32))')"
export FAKE_LOG=/tmp/fx47/fake/fake.log
export BASHCUT_CLAUDE=/tmp/fx47/fake/claude  # LIVE_FAKE wrapper
export BASHCUTPRO_DATA_DIR=/tmp/fx47/data
# LIVE_FAKE extracted from crates/ui-egui/tests/bashcut_ui.rs LIVE_FAKE const

target/release/filmcraft --control 9901 &

# Drove via control channel (Python capture.py):
#   file.openDemoProject + file.saveAs → saved project (removes "not saved" card)
#   bashcut.ui.section {section: agent, open: true}
#   bashcut.agent.send {text: "do the tools"} → tool rows + Show links
#   bashcut.agent.send {text: "cost please"} → turn summary (9 steps · 1 s · $0.12 · Fake)
#   ui.click {id: bashcut.agent.model} → model picker open

# Cropped via PIL from full-window screenshots:
#   Panel rect at 1x: 340px=[1222, 39, 340, 919], 240px=[1322, 39, 240, 919]
#   Pixel crop = rect × 2 (Retina)
```

## Files

| File | State | Width | Lang |
|---|---|---|---|
| `fx47-01-tools-done-en-340.png` | Tools done: Show links visible, Default model row | 340 px | EN |
| `fx47-01-tools-done-en-240.png` | Tools done | 240 px | EN |
| `fx47-01-tools-done-vi-340.png` | Tools done (VI): Xem links, Mặc định, Thất bại | 340 px | VI |
| `fx47-01-tools-done-vi-240.png` | Tools done | 240 px | VI |
| `fx47-02-model-picker-en-340.png` | Model picker: Default/Sonnet/Opus/Haiku/5-5 ids | 340 px | EN |
| `fx47-02-model-picker-en-240.png` | Model picker | 240 px | EN |
| `fx47-02-model-picker-vi-340.png` | Model picker (VI): Mặc định selected | 340 px | VI |
| `fx47-02-model-picker-vi-240.png` | Model picker | 240 px | VI |
| `fx47-03-turn-summary-en-340.png` | Turn summary: "9 steps · 1 s · $0.12 · Fake" | 340 px | EN |
| `fx47-03-turn-summary-en-240.png` | Turn summary | 240 px | EN |
| `fx47-03-turn-summary-vi-340.png` | Turn summary (VI): "9 bước · 1 s · $0.12 · Fake" | 340 px | VI |
| `fx47-03-turn-summary-vi-240.png` | Turn summary | 240 px | VI |
