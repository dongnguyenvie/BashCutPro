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

## Next

The full migrate / skip list and its reasons: [migration.md](migration.md).

1. **Scaffold `bashcutpro-agent-kit`**: README, skill layout, MCP setup for Claude Code / Codex, and
   the first skill (`edit-workflow`) rewritten against FilmCraft commands.
2. **First out-of-process plugin** in `bashcutpro-plugins` (candidates: caption translation, VieNeu
   Vietnamese TTS) to prove the loop: read state over MCP → work outside → import back. Decide the
   manifest (`bashcut-plugin.json`) and registry format from what it needs.
3. **Later:** `crates/bashcut-plugin-host` for in-app panels and menus, only once plugins need UI.

## Open questions

- Which features from BashCut matter most for BashCutPro's first release?
- Windows / Linux targets: which plugins must run there (VieNeu, Whisper MLX are macOS-only in BashCut;
  FilmCraft's own Whisper runs on candle).
