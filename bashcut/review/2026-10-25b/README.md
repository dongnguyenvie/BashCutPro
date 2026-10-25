# Agent Panel Review — 2026-10-25b

**Purpose:** Replace the TMUI stale shots (old 0.12.0 build, pre-redesign UI) with current builds.
**Date:** 2026-10-25  
**Build SHA:** `27ab8542e` (AGM2, origin/main)  
**Binary version:** `bashcutpro 0.12.1`  
**Platform:** macOS aarch64 (M4)  
**Pixel ratio:** 2× (Retina) — all images are logical × 2 in pixels

## Confirmation: new UI present

Shot `01-agent-empty-en-340.png` shows:
- **"Claude Code ▾"** model chip (combobox) in the header — new (AGM1/AGM2)
- **"New conversation ▾"** conversation picker in the header — new (AGU1)
- **Setup card** "Project not saved: conversations will not be kept" rendered as an info card — new (FX40)
- **Example prompt buttons** (Remove the silences / Add captions… / Plan a vlog / Hook and script / Finish for posting) — new (UX8, AGU3)
- **"Message Claude Code…"** placeholder — new (AGU2)
- **Round Send icon** (↑) in the lower right — new (AGU4)

The old UI had "You" / "Agent" labels, plain "• timeline.razor" rows, a text "Send"/"Stop" button, and an "Open in Terminal" row.

## Files

| File | State | Panel | Lang |
|---|---|---|---|
| `00-window-full-en.png` | Window reference | full window | EN |
| `01-agent-empty-en-340.png` | Empty state | Agent 340 px | EN |
| `02-agent-empty-en-240.png` | Empty state (narrow) | Agent 240 px | EN |
| `03-agent-empty-vi-240.png` | Empty state (narrow) | Agent 240 px | VI |
| `04-agent-empty-vi-340.png` | Empty state | Agent 340 px | VI |
| `05-terminal-working-en-340.png` | Terminal event injected (PreToolUse) | Agent 340 px | EN |
| `06-terminal-waiting-en-340.png` | Terminal event injected (Notification) | Agent 340 px | EN |
| `07-terminal-done-en-340.png` | Terminal event injected (Stop) | Agent 340 px | EN |
| `08-model-picker-open-en-340.png` | Model picker open | Agent 340 px | EN |
| `09-model-picker-open-en-240.png` | Model picker open (narrow) | Agent 240 px | EN |
| `10-model-picker-open-vi-340.png` | Model picker open | Agent 340 px | VI |
| `11-agent-empty-vi-240b.png` | Empty state (narrow) | Agent 240 px | VI |
| `12-rail-agent-closed-en.png` | Panel closed, rail visible | full window | EN |

### Data files
- `ui-elements-agent.json` — `ui.elements {prefix: "bashcut.agent"}` at 340 px, EN
- `ui-elements-rail.json` — `ui.elements {prefix: "bashcut.rail"}`
- `ui-state.json` — `bashcut.ui.state`

## How shots were taken

```sh
# Built from origin/main (27ab8542e)
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=.../BashCutPro/target CARGO_BUILD_JOBS=8 \
  cargo build --release -p filmcraft -p bashcutpro-cli

# Started with a fresh data dir and the control channel
FILMCRAFT_CONTROL_TOKEN=<64hex> \
  target/release/filmcraft --control 9877 \
  --data-dir /tmp/bcp-shots-data

# Drove via the control channel (Python script):
#   bashcut.ui.section {section: agent, open: true}
#   bashcut.ui.setWidth {width: 340|240}
#   bashcut.ui.setLanguage {language: en|vi}
#   bashcut.agent.terminalEvent {event: <json>}
#   ui.click {id: bashcut.agent.picker}
#   ui.screenshot {path: ...}
```

## Notes on states not captured

States that require a real running agent turn (could not be driven without claude installed):

- **Running / Thinking / Writing row** — requires a live `bashcut.agent.send` + real Claude Code process
- **Streaming turn** with tool-call rows — requires live agent
- **Tool-heavy: failed + stopped + expanded JSON + Show link** — requires live agent
- **Permission card** (bashcut.agent.permission.0.once) — requires a live agent with a pending permission
- **Error card** — requires a failed turn
- **Queued bubbles + jump pill** — requires multiple queued messages during a live turn
- **Turn summary with model chip** — requires a completed turn
- **Terminal live lock status row** — the `terminalEvent` commands were injected (shots 05–07) but the status row only renders when a session's `resume` argv is set, so no visible change appears in this headless context

These states need the agent installed (claude or codex) and a real project. A follow-up manual capture session or a test ticket should cover them.

## Problem list (judged on these shots only)

Problems are rated **P1** (blocked / broken), **P2** (real friction), **P3** (polish). Each is observed from the screenshots.

### Header area

| # | Sev | Finding | Shot | Notes |
|---|---|---|---|---|
| N1 | P2 | **Model chip (340 px):** The "Claude Code ▾" combo and "New conversation ▾" combo are placed side-by-side; at 340 px they fit. At 240 px the conversation picker drops to a second row (`narrow < 300 px` triggers it in `header`). The two-row layout at 240 px puts the conversation picker at full width which is fine, but the model chip and icon buttons remain on the top row — the icons crowd the right edge. | 02, 09 | The ± row works but see N3 for the icon size. |
| N2 | P3 | **"New conversation"** label in the conversation picker truncates at 240 px to just "New conversa…" (see shot 02, 03). The picker takes `ui.available_width()`, so it is as wide as the row, but the placeholder text is longer than the truncated label shown (it says "Cuộc trò chuyện mới" in VI which is shorter). | 02, 03 | Truncation is expected; the full text is in the tooltip; acceptable. |
| N3 | P3 | **Header icon density.** The three icons (hamburger ≡, new +, pop-out ⤢) in the top-right are 20×18 px each with minimal gaps. On Retina they render crisply, but the hit targets are borderline small (20 px). | 01 | Minor; matches upstream icon sizes. |

### Setup strip (info cards)

| # | Sev | Finding | Shot | Notes |
|---|---|---|---|---|
| N4 | P2 | **Setup card is always shown when no project is saved.** The card "Project not saved: conversations will not be kept" + "Save…" button takes ~55 px of vertical space at the top of every session on a new project. It cannot be dismissed. On a project the user cannot save (read-only, demo), this is a permanent nag. | 01, 02, 03, 04 | Design decision in UX7; no dismiss button specified. Consider a "Don't show again" link. |
| N5 | P3 | **Setup card text at 240 px wraps.** "Project not saved: conversations will not be kept" wraps to 2 lines and "Save…" button floats to the right of line 2 (shot 02). The card area is tight. | 02, 03 | Expected wrapping; readable. |

### Empty state

| # | Sev | Finding | Shot | Notes |
|---|---|---|---|---|
| N6 | P2 | **Example prompt buttons wrap differently EN vs VI.** At 340 px EN: two buttons fit on the first row ("Remove the silences" / "Add captions to the main sequence" each on its own row due to width). At 340 px VI: "Thêm phụ đề cho sequence chính" wraps inside its button border; the button height grows to 2 lines. At 240 px VI: "Cắt các đoạn im lặng" fits on one line but "Thêm phụ đề cho sequence chính" wraps. | 03, 04, 11 | Vietnamese example prompt text is longer than English; some buttons go 2-line in VI at 240 px. This should be reviewed with the VI translator for shorter phrasing. |
| N7 | P3 | **Chat icon in the empty state** is a filled speech-bubble icon (the same as the rail icon). It's slightly large (≈24 px) and sits alone above the title. No contrast issue. | 01 | Minor; consistent with the rail. |
| N8 | P3 | **Blurb text** "Claude Code edits this project with you. It stops at each stage for your approval." is shown both under the panel title AND inside the empty state area. It repeats the information from the header blurb. | 01 | Matches code: `blurb` is in `side_panel.rs` header and the `empty` state. The ticket UX0-R5 noted this; not yet fixed. |

### Model picker

| # | Sev | Finding | Shot | Notes |
|---|---|---|---|---|
| N9 | P2 | **"Codex (not installed)"** appears in the picker as a greyed row that cannot be clicked. This is correct behaviour (UX7) but the label uses the English suffix "(not installed)" even in Vietnamese mode. | 10 | The label comes from `setup.rs` `f("{agent} (not installed)", ...)`. The string `"(not installed)"` is not yet in `vi.tsv`. New ticket needed. |
| N10 | P3 | **Model picker width**: at 340 px the picker is sized 90–150 px (clamped by the code). The dropdown shows "Claude Code" selected in blue and "Codex (not installed)" dimmed below. Layout is clean. | 08, 09 | Good. |
| N11 | P3 | **Picker closes on Escape** (tested by the capture script). Clicking outside also closes it. No issue. | 08 | OK |

### Input area

| # | Sev | Finding | Shot | Notes |
|---|---|---|---|---|
| N12 | P2 | **Send button is disabled (greyed)** when there is no text in the input and no running agent (correct). The context label "Main Edit · 0:04.3" shows the current sequence and its duration — this is useful. The Send button shows a round ↑ icon. | 01 | The disabled tooltip ("Write a message first") is not visible in a static screenshot; should be verified live. |
| N13 | P3 | **Input field height.** At 340 px, the input area occupies the bottom ~60 px of the panel. At 240 px the same. No clipping visible. | 01, 02 | OK |

### Vietnamese text

| # | Sev | Finding | Shot | Notes |
|---|---|---|---|---|
| N14 | P2 | **VI blurb overflow at 240 px.** The panel-level blurb "Trò chuyện với trợ lý dựng phim (Claude Code, Codex) về dự án này." wraps to 3 lines at 240 px (shots 03, 11), taking more space than the EN 2-line wrap. The title "Trợ lý" is shorter than "Agent". | 03, 11 | The blurb is defined in `SECTIONS` and is not translated to a shorter VI form specifically for the narrow panel. Could use a shorter VI blurb. |
| N15 | P3 | **"Cuộc trò chuyện mới"** (New conversation) fits at 240 px correctly. Model picker label "Claude Code" is untranslated (it's a product name, correct). | 03 | OK |
| N16 | P3 | **"Nhắn Claude Code…"** input placeholder (VI for "Message Claude Code…") is correctly localized. | 03, 04, 11 | Good. |
| N17 | P3 | **"(not installed)"** suffix in the model picker is English even in VI mode (N9). Same text: `f("{agent} (not installed)", ...)`. | 10 | Missing VI row in `vi.tsv`. |

### Rail

| # | Sev | Finding | Shot | Notes |
|---|---|---|---|---|
| N18 | P3 | **Rail icons** at the right edge (shot 00, 12) show the 6 sections (Agent, Batch, Looks, Library, Plugins, Runs) as small monochrome icons. Without the Agent panel open (shot 12), the rail is the only BashCutPro UI visible. The Agent icon (chat bubble) has no badge/dot in this state — correct, no pending events. | 00, 12 | Rail dot badges are not triggered in this session (no running jobs). |
| N19 | P3 | **No dot on rail** for Agent when a turn ends while the section is not visible — not captured (requires live agent). The code for this is in `rail.rs::agent_badge`. Not a visual bug in the current shots. | — | Out of scope for static shots. |

### Alignment and spacing

| # | Sev | Finding | Shot | Notes |
|---|---|---|---|---|
| N20 | P3 | **Setup card left edge aligns with the panel's content padding** (8 px from the panel edge). Example buttons also align at the same padding. Consistent. | 01 | OK |
| N21 | P3 | **Example prompt button grid:** At 340 px, EN, the layout is: row 1: "Remove the silences" (single); row 2: "Add captions to the main sequence" (single, wide); row 3: "Plan a vlog" + "Hook and script" (two); row 4: "Finish for posting" (single). The mixed single/double layout causes an uneven grid feel. Not a bug but a polish opportunity. | 01 | The layout uses `ui.horizontal_wrapped` which fills left-to-right; the visual rhythm is not grid-like. |
| N22 | P3 | **Empty state icon color**: The speech-bubble icon matches the text color (dim white). No contrast issue on the dark theme. | 01 | OK |

## Summary for the coordinator

The redesigned Agent panel (0.12.1, AGU1–12, AGT1–3, FX40, PLK5/PLK7, AGM1/AGM2) is a clear improvement over the 0.12.0 build. The key new features visible:

- Model chip + conversation picker in the header (AGM1/AGM2, AGU1)
- Info cards for setup issues (FX40)
- Example prompt buttons in the empty state (UX8/AGU3)
- Vietnamese localization mostly correct — one missing string N9/N17 ("not installed")
- Round Send icon (↑), context label below the input

Critical issues (P2): none that would block shipping this UI.

Actionable polish:
- N9/N17: add `(chưa cài đặt)` to `vi.tsv` for the picker's not-installed suffix
- N6: shorten the Vietnamese example prompt "Thêm phụ đề cho sequence chính" for the 240 px narrow panel
- N14: consider a shorter VI blurb for narrow widths

States that need a live-agent follow-up capture: running turn, tool calls, permission card, error card, queued bubbles, jump pill, turn summary. Recommend a separate TMUI3 ticket once claude is installed in the test environment.
