# Agent Panel Review — 2026-10-25c (LIVE states)

**Purpose:** Capture the live Agent panel states that TMUI2 could not — running/thinking, streaming
text, tool calls, permission card, error card, queued bubbles, turn summary, terminal status, rail
dot. All shots use the fake stream-json agent (`LIVE_FAKE` from `crates/ui-egui/tests/bashcut_ui.rs`)
so no real Claude Code install is required.  
**Date:** 2026-10-25  
**Build SHA:** `c77bdb493` (TPKG, origin/main after FX42/FX43/TPKG merged)  
**Binary version:** `bashcutpro 0.12.1`  
**Platform:** macOS aarch64 (M4)  
**Pixel ratio:** 2× (Retina) — all cropped images are logical × 2 in pixels  
**Crop:** Agent panel only (not full window), except shots 47–48 (full window for rail context)

## How shots were taken

```sh
# 1. Written LIVE_FAKE to /tmp/tmui3/fake/live.py (from crates/ui-egui/tests/bashcut_ui.rs LIVE_FAKE)
# 2. Created wrapper: /tmp/tmui3/fake/claude (#!/bin/sh exec python3 live.py "$@")
# 3. Generated control token
export FILMCRAFT_CONTROL_TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(32))')"

# 4. Launched app with fake agent, scratch data dir, control port 9899
BASHCUT_CLAUDE=/tmp/tmui3/fake/claude \
BASHCUTPRO_DATA_DIR=/tmp/tmui3/data \
  target/release/filmcraft --control 9899 &

# 5. Saved demo project via control channel (removes "not saved" card)
#    engine.execute file.openDemoProject + file.saveAs /tmp/tmui3/data/film.fcproj

# 6. Drove each state via control channel (Python):
#    bashcut.agent.send {text: "think about it"}   -> Thinking hold
#    bashcut.agent.send {text: "stream it"}         -> Streaming text hold
#    bashcut.agent.send {text: "do the tools"}      -> tool calls + failed Bash
#    bashcut.agent.send {text: "toolhold now"} + stop -> Stopped tool
#    bashcut.agent.send {text: "please ask"}         -> Permission card
#    bashcut.agent.send {text: "please fail"}        -> Error card
#    bashcut.agent.send {text: "long hold..."} + queue -> Queued bubbles + jump pill
#    bashcut.agent.send {text: "cost please"}        -> Turn summary
#    ui.click {id: bashcut.agent.picker}             -> Model picker open
#    ui.click More menu + bashcut.agent.terminal     -> Opens terminal (sets TerminalWatch)
#    bashcut.agent.terminalEvent {event: ...}        -> Terminal status line states
#    bashcut.ui.section {section: agent, open: false} + turn -> Rail (section closed)

# 7. Cropped via PIL: panel rect from ui.elements bashcut.panel.agent (1222, 39, 340, 919 pt)
#    pixel crop = rect × 2 (Retina); full-window shots saved as-is for rail
```

## Files

| File | State | Panel width | Lang |
|---|---|---|---|
| `01-running-thinking-en-340.png` | Running: Thinking row | 340 px | EN |
| `02-running-thinking-en-240.png` | Running: Thinking row | 240 px | EN |
| `03-running-thinking-vi-340.png` | Running: Thinking row | 340 px | VI |
| `04-running-thinking-vi-240.png` | Running: Thinking row | 240 px | VI |
| `05-streaming-text-en-340.png` | Streaming "Hello world" | 340 px | EN |
| `06-streaming-text-en-240.png` | Streaming "Hello world" | 240 px | EN |
| `07-streaming-text-vi-340.png` | Streaming "Hello world" | 340 px | VI |
| `08-streaming-text-vi-240.png` | Streaming "Hello world" | 240 px | VI |
| `09-tool-heavy-en-340.png` | Tools: razor + ripple group + Bash Failed (expanded) + Show | 340 px | EN |
| `10-tool-heavy-en-240.png` | Tools: same, 240 px | 240 px | EN |
| `11-tool-heavy-vi-340.png` | Tools: same, VI | 340 px | VI |
| `12-tool-heavy-vi-240.png` | Tools: same, 240 px VI | 240 px | VI |
| `13-tool-stopped-en-340.png` | Tool stopped (toolhold + Stop) | 340 px | EN |
| `14-tool-stopped-en-240.png` | Tool stopped | 240 px | EN |
| `15-tool-stopped-vi-340.png` | Tool stopped | 340 px | VI |
| `16-permission-card-en-340.png` | Permission card pending (rm -rf build) | 340 px | EN |
| `17-permission-card-en-240.png` | Permission card pending | 240 px | EN |
| `18-permission-card-vi-340.png` | Permission card pending | 340 px | VI |
| `19-permission-card-vi-240.png` | Permission card pending | 240 px | VI |
| `20-permission-answered-en-340.png` | Permission answered (Allow once) | 340 px | EN |
| `21-error-card-en-340.png` | Error card (error_during_execution) | 340 px | EN |
| `22-error-card-en-240.png` | Error card | 240 px | EN |
| `23-error-card-vi-340.png` | Error card | 340 px | VI |
| `24-error-card-vi-240.png` | Error card | 240 px | VI |
| `25-queued-jump-en-340.png` | Queued bubbles (2 msgs) + New messages pill | 340 px | EN |
| `26-queued-jump-en-240.png` | Queued bubbles + pill | 240 px | EN |
| `27-queued-jump-vi-340.png` | Queued bubbles + pill | 340 px | VI |
| `28-queued-jump-vi-240.png` | Queued bubbles + pill | 240 px | VI |
| `29-turn-summary-en-340.png` | Turn summary: "9 steps · N s · $0.12" + model chip | 340 px | EN |
| `30-turn-summary-en-240.png` | Turn summary | 240 px | EN |
| `31-turn-summary-vi-340.png` | Turn summary VI ("9 bước · …") | 340 px | VI |
| `32-turn-summary-vi-240.png` | Turn summary VI | 240 px | VI |
| `33-model-picker-en-340.png` | Model picker open (Claude Code + Codex rows) | 340 px | EN |
| `34-model-picker-en-240.png` | Model picker open | 240 px | EN |
| `35-model-picker-vi-340.png` | Model picker open | 340 px | VI |
| `36-model-picker-vi-240.png` | Model picker open | 240 px | VI |
| `37-terminal-working-en-340.png` | Terminal: working… (UserPromptSubmit hook) | 340 px | EN |
| `38-terminal-working-vi-340.png` | Terminal: working… | 340 px | VI |
| `39-terminal-working-en-240.png` | Terminal: working… | 240 px | EN |
| `40-terminal-waiting-en-340.png` | Terminal: waiting for your permission (PermissionRequest) | 340 px | EN |
| `41-terminal-waiting-vi-340.png` | Terminal: waiting… | 340 px | VI |
| `42-terminal-done-en-340.png` | Terminal: done (Stop hook) | 340 px | EN |
| `43-terminal-done-vi-340.png` | Terminal: done | 340 px | VI |
| `47-rail-agent-closed-en.png` | Rail with Agent section closed (full window, no dot — see notes) | full | EN |
| `48-rail-agent-closed-vi.png` | Rail with Agent section closed (VI; rail icons are language-agnostic) | full | VI |

## Notes on specific states

### Terminal locked composer
The "locked composer" state (message box hidden because terminal holds the session) is visually
identical to the "terminal working" state (shots 37/38/39): both show the status line and hide the
message box. Shots 37–39 serve as the locked-composer reference. The locked state persists through
all non-Done phases (Working, Waiting) and is released by Stop (Done phase, shots 42–43) or Take back.

### Rail dot
The rail dot ("Agent: news") only appears when: (a) the Agent section is hidden, (b) the window is
NOT OS-focused, and (c) a turn ends. The control channel cannot unfocus the window from outside, so
the dot cannot be triggered from an automated script without Computer Use. Shots 47–48 show the rail
with the Agent section closed (no dot — the "cleared" baseline). The dot state requires a manual
capture or a headless test (see `attention_dot_and_notification` in `crates/ui-egui/tests/bashcut_ui.rs`).

### Rail shots EN/VI identical
The rail section icons are the same in both languages (no text labels). Shots 47 and 48 differ only
in the language setting but are byte-identical; this is expected and not a duplicate error.

### Streaming text
Shot taken while `stream it` is held (FAKE_GO not created) and "Hello world" appears in the
transcript as a growing provisional line. The agent process is held until release.

## Problem list (these states only)

| # | Sev | Finding | Shot | Notes |
|---|---|---|---|---|
| L1 | P2 | **Turn summary not scrolled into view at 240 px.** At 240 px after `cost please`, the turn-summary line ("9 steps · N s · $0.12") may be below the visible area if the transcript fills the panel. The shots show it scrolled into view. In real use the user may need to scroll to see it. | 29–32 | Expected at 240 px; no auto-scroll to turn end after idle. |
| L2 | P2 | **Terminal locked state visual = Working state.** When the session is held by the terminal (all non-Done phases), the composer is locked and the status line shows the phase. The "locked composer" and "terminal working" shots are identical because both show the same phase. There is no distinct "locked" visual beyond the phase line + hidden message box. | 37–39 | Designed this way; Take back gives the box back. |
| L3 | P3 | **Model picker VI: "(not installed)" suffix in EN.** Codex row shows "Codex (not installed)" with the English suffix in VI mode (N9/N17 from TMUI2 — same issue). | 35–36 | Missing VI row in `vi.tsv`; noted in TMUI2 N9/N17. |
| L4 | P3 | **Rail dot cannot be captured automatically.** The OS-focus constraint means automated scripts cannot produce the dot state. Headless test `attention_dot_and_notification` covers this behavior. | 47–48 | Requires Computer Use or a manual session for a screenshot. |
| L5 | P3 | **Turn summary VI: "giây" label may be long at 240 px.** "9 bước · N giây · $0.12" wraps inside the summary row at narrow widths; the row does not grow to two lines but the giây text may be partially clipped. | 31–32 | Check at 240 px VI; may need shorter label. |
| L6 | P3 | **Queued jump pill overlaps with queued bubbles at 240 px.** At 240 px the "New messages" pill sits above the transcript and the queued message bubbles are below. The layout is correct but visually dense. | 26, 28 | Expected; both fit within the panel. |
