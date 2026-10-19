# BashCutPro: fork overview

Read this first if you are an agent (or a person) picking up BashCutPro work without the earlier
conversation. Everything here is about **our fork**; the rest of the repository is upstream FilmCraft
and its own docs (`CLAUDE.md`, `AGENTS.md`, `docs/`) still apply in full.

| File | What it holds |
|---|---|
| [README.md](README.md) | this overview: what the fork is, the repos, the direction |
| [extending-upstream.md](extending-upstream.md) | **read before touching upstream files**: add, never modify; extension points; what upstream changes most |
| [decisions.md](decisions.md) | decisions already made, with the reason; don't re-open them without the owner |
| [migration.md](migration.md) | what to port from BashCut's kit and plugins, what to skip, and the order |
| [status.md](status.md) | where the work stands and the next steps; update it when you finish something |
| [rebrand.md](rebrand.md) | how builds show BashCutPro instead of FilmCraft / ArtCraft, and every upstream file that touches |
| [scripts/](scripts/upstream.py) | upstream-sync tooling: `upstream.sh footprint` (the check before every PR), `merge` with rerere, `setup` |
| [agent-connect.md](agent-connect.md) | agents reach the open window (`automation.json`, `bashcut-mcp`), project engine lock, allowed write roots (#51) |
| [batch.md](batch.md) | `bashcut-batch`: headless parallel pipelines, one project per input, resumable job book (#45) |
| [brand/](brand/README.md) | BashCutPro's app icon, mark and wordmarks, and the scripts that render them |
| [commands.md](commands.md), [commands.json](commands.json) | FilmCraft's live command catalogue (`command_list`) and the ids that matter for agents |
| [skill-needs.md](skill-needs.md) | what each BashCut skill needs, need by need: available, compose, plugin, or new engine command |
| [plugin-host.md](plugin-host.md) | `bashcut-plugin-host`: running plugins from the app (approved) |
| [in-app-ui.md](in-app-ui.md) | in-app UI (rail, side panel, agent, plugins, runs), where its code lives, and every upstream hook line (accepted) |

## What BashCutPro is

- A fork of **FilmCraft** (pure-Rust, Premiere-parity NLE, MIT / Apache-2.0):
  `https://github.com/storytold/filmcraft`, remote `upstream` (fetch only). See [`../UPSTREAM.md`](../UPSTREAM.md)
  for the sync procedure and log.
- The goal: bring what BashCut (our macOS Swift editor, repo `dongnguyenvie/BashCut`, local
  `../bash-cut`) does for AI agents (editing skills, plugins for TTS / captions / image generation, the
  edit workflow) to a cross-platform engine, **while keeping the fork mergeable with upstream**.

## Repositories

| Repo | Local path (from this repo) | Role |
|---|---|---|
| `dongnguyenvie/BashCutPro` | `.` | the app (FilmCraft fork) plus a thin, additive layer of our own |
| `dongnguyenvie/bashcutpro-agent-kit` | `../bashcutpro-agent-kit` | agent skills for BashCutPro, written for FilmCraft's command model |
| `dongnguyenvie/bashcutpro-plugins` | `../bashcutpro-plugins` | plugins (out-of-process) and their registry |
| `dongnguyenvie/BashCut`, `bashcut-agent-kit`, `bashcut-plugins` | `../bash-cut`, `../bashcut-agent-kit`, `../bashcut-plugins` | the Swift product; **reference only**: port the editing knowledge, not the command calls |

## Direction

1. **Upstream first.** Keep changes to upstream files as small as possible; put our code in new files
   and new crates. Every edit to an upstream file is a future merge conflict.
2. **Rewrite, don't map.** BashCut has ~300 dedicated tools (`plan apply`, `selects place`…); FilmCraft
   has a generic command model (`command_list`, `command_run {id, params}`, `command_batch`,
   `doc_inspect`, `render_frame`; see `docs/agents.md`). Skills in `bashcutpro-agent-kit` carry over
   BashCut's editing craft (thresholds, cut-by-quote, rhythm, review rules, vlog recipes) and are
   rewritten against FilmCraft command ids.
3. **Plugins live outside the app.** FilmCraft has no plugin system (gap G11 in `docs/gaps.md` is about
   VST3 / AU / OpenFX; Preferences ▸ Plugins is a stub). A plugin is a separate process that:
   - reads state through MCP (`filmcraft-cli mcp`, headless or `--bridge`) or the control channel
     (`filmcraft --control <port>`, `docs/control-protocol.md`);
   - does its work outside (AI image generation, Vietnamese TTS, caption translation, producing
     `.fcgt` graphics templates, `.cube` LUTs, wav / srt files…);
   - brings the result in with existing commands (`media_import`, `graphics.template.apply`, caption
     import, effect / export presets, `command_batch`).
4. **In-app UI only when needed**, through one additive crate: `crates/bashcut-plugin-host` (see
   [decisions.md](decisions.md) for the names). It would run plugin processes and show their panel
   and menu; the core edit is limited to registering it.
5. **Use what FilmCraft already has** before building: Whisper speech-to-text (`speech`, `transcript.*`),
   text to speech (`tts.*`), music remix (`clip.remix`), Essential Sound, scene detection, Lumetri /
   LUTs, graphics templates, caption formats, multicam sync.

## Extension points (no core change)

| Surface | Use |
|---|---|
| MCP: `filmcraft-cli mcp` | every engine command, project / sequence inspection, frame render; `--bridge` drives the open app |
| Control channel: `filmcraft --control <port>` | JSON lines: `engine.execute`, UI state, screenshots, clicks |
| CLI | `filmcraft-cli export --preset …` |
| `.fcproj` | UTF-8 JSON with a schema version (`docs/project-files.md`) |
| Data files | effect presets JSON, export presets, `.fcgt` templates (`graphics.template.install`), `.cube` / `.3dl` LUTs, SRT / VTT / SCC / TTML captions, label presets |
| Web build | `window.filmcraft` (`docs/web.md`) |

## Rules for our own changes

- Follow upstream's rules (`AGENTS.md`: never crash, clean-room, pure Rust, layering, quality gates).
- Our crates and ids carry a `bashcut` prefix so they never collide with upstream names.
- Docs, UI text and samples in English.
- GitHub Actions run manually only (`workflow_dispatch`); keep it that way when merging upstream.
- Commit identity `dongnguyenvie@gmail.com`; no AI attribution lines in commits or PRs.
