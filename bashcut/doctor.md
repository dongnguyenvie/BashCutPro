# Doctor, update checks and plugin hooks (#73)

So a long unattended run does not fail on setup. Rules: [extending-upstream.md](extending-upstream.md) (no core change).

## `bashcut-doctor` (`apps/bashcut-doctor`, L6)

`bashcut-doctor [--json] [--updates] [--no-hash] [--cli p] [--data-dir d] [--kit d] [--plugins d] [--registry p|url]`

| Check (`id`) | Fails / warns when | Fix it prints |
|---|---|---|
| `cli`, `cli.commands` | the engine (`--cli`, else the shared `bashcut_dirs::engine` lookup: `BASHCUTPRO_CLI`, `BASHCUT_FILMCRAFT_CLI`, `FILMCRAFT_CLI`, then `bashcutpro-cli` / `bcpro` / `filmcraft-cli` next to the doctor, then on `PATH`, else `filmcraft-cli`; RC2) does not start; the build lacks `bashcut.profiles.*`, `bashcut.template.instantiate`, `bashcut.audio.polish` | install / rebuild (kit `bcp:setup`) |
| `speech` | `transcript.models` says the CLI has no speech-to-text (built without `whisper`) or the default model is not downloaded (warn): the batch presets and caption skills start by transcribing | build with `--features whisper`; `bashcutpro-cli exec transcript.downloadModel` |
| `kit.commands` | the kit's `scripts/skill-commands.json` (skill → command ids) names core or `bashcut.*` commands that `commands --json` does not list (plugin ids are skipped) (Warn: skills and ids named; Info: the kit has no index, an older kit). The kit is found like `kit`: `--kit`, the Claude Code plugin path, or the clone behind `~/.agents/skills/bcp-*` links. Compared by `bashcut_dirs::kit::kit_all_command_gaps` | Download kit in the Agent panel / `git -C <kit> pull`, or update BashCutPro |
| `kit` | no kit clone (`BASHCUTPRO_KIT_DIR` / `--kit`) and no installed `bcp-*` skills in `~/.agents/skills` or `~/.claude/skills` | install the kit |
| `plugin.<id>` | a Python plugin's interpreter (its `entry`, per OS) is missing or older than `runtime.version` | install that Python |
| `model.<id>` | a downloaded file's size or SHA-256 differs from `models.json` (`BASHCUT_MODELS_MANIFEST`, else `<data dir>/bashcut-plugins/models.json`; cache `BASHCUT_MODELS_DIR`, else `<data dir>/bashcut-models`, the plugins' download cache, not the engine's `<data dir>/models`) | delete and re-download |
| `agent.claude`, `agent.codex`, `agent` | neither agent CLI runs | install Claude Code or Codex |
| `disk` | under 2 GB free (fail) or under `--min-free-gb` (10) at the data dir | free space |
| `ram` | under `--min-ram-gb` (8) | proxies, smaller models |
| `gpu` | info: the wgpu backend (Metal / DX12 / Vulkan, or `WGPU_BACKEND`) | |

Exit 1 when anything failed, else 0. `--json` prints `{"schema":"bashcut-doctor/1","ok","worst","findings":[{id,level,message,fix}]}`:
the batch runner runs it before every batch and the Runs section shows it. Everything goes through the `Env` trait, so
the tests (`apps/bashcut-doctor/tests/bashcut_doctor.rs`) use a fake machine. Every program it runs (engine probes,
`python3 --version`, `df`) runs through `bashcut_dirs::child` with stdin null, its own process group and capped
output, and is stopped after 30 s (`RUN_TIMEOUT`; a stopped probe counts as not starting, DR1).

## Update checks (`--updates`)

Notify only, never installs: installed plugins (`<data dir>/bashcut-plugins/*/bashcut-plugin.json`) against the
registry (`--registry`, `BASHCUTPRO_REGISTRY`, else `registry.json` on the plugins repo's `main` through `curl` or
`gh api`); the kit clone's `HEAD` against `origin`; the CLI's version against the latest BashCutPro release
(`gh release view`). Unreachable sources are reported as skipped.

## Plugin hooks (engine, `crates/engine/src/bashcut/hooks.rs`)

Declared in `bashcut-plugin.json`, run by our own code paths only (not upstream events):

```json
"hooks": [{"event": "export.before", "action": "check-key", "required": false, "timeoutSeconds": 120}]
```

| Event | Fired by |
|---|---|
| `batch.beforeItem`, `batch.afterItem` | the batch runner, through `bashcut.hooks.run {event, context}` |
| `import.after` | `bashcut.media.import` (`file.import`, then the hooks) |
| `export.before` | `bashcut.profiles.apply` with a `path` |

A hook runs the plugin's action (entry command for this OS + the action's `args`, plus `connect.project` with the saved
project when the action `needs` it) in the plugin's folder with `BASHCUT_HOOK_EVENT` and `BASHCUT_HOOK_CONTEXT` (JSON)
set, plus the plugin's settings env as its action runs get it (each setting's first `env` name; secrets from the OS key
store; `BASHCUT_PLUGIN_HOST=1`; RC3), and answers with `bashcut-result/1`. Exit 0 = `ok`, 3 = `handoff` (reported),
other = `failed` (the row carries stderr's tail); a failed `required` hook stops the command. `timeoutSeconds`
(default 300, max 3600) stops a stuck hook: TERM to its process group, KILL after 1.5 s. `BASHCUT_HOOKS=off` or
`hooks: false` turns them off. `bashcut.hooks.list {event?}` shows what is declared.

Note (PLK8): `check_cli` lists every engine id (`commands --json`, no filter; output over 4 MB is ignored) and `kit.commands` compares them with `kit_all_command_gaps`, like the Agent panel (PLK5): core ids and `bashcut.*` are covered, plugin ids (`bashcut.<plugin>`) are not. `REQUIRED_COMMANDS` is unchanged.
