# Status

Update this file when you finish a step or the plan changes. Keep "Next" short and ordered.

_Last updated: 2026-10-20_

## Done

- Imported FilmCraft (upstream `105c09bd`, v0.8.0) and added `UPSTREAM.md`.
- All GitHub Actions switched to manual (`fc3cd05c`).
- Created the empty repos `bashcutpro-agent-kit` and `bashcutpro-plugins` (cloned beside this repo).
- Wrote this folder (`bashcut/`): overview, decisions, status.

## Next

The full migrate / skip list and its reasons: [migration.md](migration.md).

1. **Command inventory.** Build `filmcraft-cli`, run `command_list` (MCP) and save the catalogue. Map
   each `bashcut-agent-kit` skill to the FilmCraft commands it needs: available now, composable with
   `command_batch`, needs a plugin, or needs a new command.
2. **Scaffold `bashcutpro-agent-kit`**: README, skill layout, MCP setup for Claude Code / Codex, and
   the first skill (`edit-workflow`) rewritten against FilmCraft commands.
3. **First out-of-process plugin** in `bashcutpro-plugins` (candidates: caption translation, VieNeu
   Vietnamese TTS) to prove the loop: read state over MCP → work outside → import back. Decide the
   manifest (`bashcut-plugin.json`) and registry format from what it needs.
4. **Later:** `crates/bashcut-plugin-host` for in-app panels and menus, only once plugins need UI.

## Open questions

- Which features from BashCut matter most for BashCutPro's first release?
- Windows / Linux targets: which plugins must run there (VieNeu, Whisper MLX are macOS-only in BashCut;
  FilmCraft's own Whisper runs on candle).
