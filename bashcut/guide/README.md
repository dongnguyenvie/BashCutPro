# BashCutPro user guide

A short guide to what BashCutPro adds on top of the FilmCraft editor: the right-hand rail with its sections, batch
runs, platform-correct exports, looks, the online library and the editing agent. It is written for people who edit
videos, not for developers.

| | English | Tiếng Việt |
|---|---|---|
| Install and first steps | [Getting started](en/getting-started.md) | [Bắt đầu](vi/getting-started.md) |
| The rail and the command palette | [The rail](en/rail.md) | [Thanh bên](vi/rail.md) |
| Many files at once | [Batch](en/batch.md) | [Hàng loạt](vi/batch.md) |
| Exports for TikTok, YouTube… | [Output profiles](en/export.md) | [Hồ sơ xuất](vi/export.md) |
| Colour in one click | [Looks](en/looks.md) | [Màu phim](vi/looks.md) |
| Music, sound effects, stock | [Library](en/library.md) | [Thư viện](vi/library.md) |
| Claude Code and Codex | [The agent](en/agent.md) | [Trợ lý](vi/agent.md) |
| Voice, captions, templates, review | [More tools](en/more-tools.md) | [Công cụ khác](vi/more-tools.md) |
| When something goes wrong | [Troubleshooting](en/troubleshooting.md) | [Khắc phục sự cố](vi/troubleshooting.md) |

Everything else (timeline, monitors, effects, export) is FilmCraft's own editor and works as its documentation
describes.

**Status (2026-10-20).** The guide describes what is merged on `main` and was walked step by step in the first build
pass. Keys: **Cmd** on a Mac is **Ctrl** on Windows and Linux.

**Pictures** (`img/`) are taken from the app itself at 1280x800 in the dark theme, on the built-in demo project:
`filmcraft --control 9876`, open the section with `engine.execute {"id": "bashcut.ui.show.<section>"}`, then
`ui.screenshot`; crop to the panel and scale to 480 px wide (the whole window to 960). Each picture has a
`.attribution` sidecar and a row in `ATTRIBUTION.md`; retake it when its part of the screen changes.
