# Batch runner (`bashcut-batch`)

Issue #45. Batch runs are first-class and headless: every input becomes its own project, up to `concurrency` items
run at once, each step in its own `filmcraft-cli` process, and the job book survives the runner, the app or the
machine stopping. BashCut ran items one at a time inside the open app; here the app does not need to be open.

| Path | What |
|---|---|
| `crates/bashcut-batch/` | library (L4, no workspace deps): pipeline format, job book, runner, presets |
| `apps/bashcut-batch/` | the `bashcut-batch` command (package `bashcut-batch-cli`, L6) |
| `crates/bashcut-batch/tests/batch.rs` | integration tests against a fake `filmcraft-cli` (`tests/support/fake_cli.rs`) |

It sits on the process boundary (the top rung in [extending-upstream.md](extending-upstream.md) §4): it only runs
`filmcraft-cli`, plugins and agents, so it needs no upstream hook. The Batch section (#46) will drive the same
library.

## Pipeline (`bashcut-batch/1`)

```json
{
  "schema": "bashcut-batch/1",
  "name": "captions-draft",
  "inputs": ["clips/", "extra/intro.mov", "old/edit.fcproj"],
  "template": {"project": "templates/vlog.fcproj"},
  "steps": [
    {"command": "sequence.transcribe", "params": {"language": "auto"}},
    {"commands": [{"id": "captions.fromTranscript"}, {"id": "captions.style", "params": {"preset": "word-pop"}}],
     "label": "captions", "timeoutSeconds": 600},
    {"plugin": {"argv": ["python3", "../bashcutpro-plugins/plugins/translate-captions/run.py", "apply", "--project", "{project}"]}},
    {"agent": {"skill": "rough-cut", "brief": "Keep it under 60 s.", "maxTurns": 40, "maxMinutes": 20, "fixRounds": 2}}
  ],
  "outputs": [{"name": "{name}-tiktok.mp4", "profile": "tiktok"}, {"name": "yt/{name}.mp4", "preset": "YouTube 1080p Full HD"}],
  "onFailure": "skip",
  "concurrency": 2,
  "agentConcurrency": 1,
  "outputName": "{index}-{stem}",
  "projectsFolder": "out/projects",
  "outputsFolder": "out/exports"
}
```

(The command ids above are only an example; use ids from `filmcraft-cli commands`.)

- **inputs**: files, folders (their media files, not recursive; `extensions` overrides the list) and `.fcproj`
  projects. Relative paths resolve against the pipeline file's folder (a preset's against the working folder).
- **template**: `{"project": …}` is copied for every item; `{"commands": […]}` builds a new project instead. Without
  one, a media input gets `file.import {paths: ["{media}"]}` then `file.newSequenceFromClip {items: [1]}` (the first item imported into a new project; headless there is no
  Project panel selection). A `.fcproj` input is copied, never edited in place.
- **steps**: exactly one of `command` (+ `params`), `commands` (one process: all saved or none), `plugin` (`argv`,
  `env`: any program; it connects to the project itself, `bashcut-result/1` on stdout, exit 3 = hand-off) or
  `agent` (see [Agent steps](#agent-steps)). On Windows a plugin argv starting with `python3` (the Microsoft Store
  stub there) runs with the first of `python3`, `py -3`, `python` that answers.
  `timeoutSeconds` on any step.
- **outputs**: `name` (inside the outputs folder, may have subfolders), and either a `profile` (#43: `tiktok`,
  `reels`, `shorts`, `youtube`, `youtube-4k`, `facebook-square`… exported with `bashcut.profiles.apply`, which also
  applies the profile's loudness and caption rules) or `preset`, `format`, `settings` (ExportSettings JSON).
- **description**: one line shown by `presets list`.
- **placeholders** in params, argv, env and output names: `{input}`, `{media}`, `{name}`, `{project}`, `{itemDir}`,
  `{outputs}`, `{cli}` (the engine in use: `bashcutpro` or `filmcraft-cli`) and `{plugins}` (`<data dir>/bashcut-plugins`, where the plugin
  host installs plugins: `{plugins}/bashcut.translate-captions/plugin.py`); `{prompt}`, `{session}`, `{mcpConfig}` in
  an agent's own argv. `outputName`
  takes `{stem}` and `{index}`.
- **onFailure**: `skip` (the item stops, the others go on) or `stop` (nothing new starts; running items stop before
  their next step and become `notRun`).

## How a job runs

Each item has the steps `create`, `step-1`…, `export-1`…:

| Step | Process |
|---|---|
| create | copy (template / input project), or `filmcraft-cli run create.jsonl --save-as <tmp>`; renamed to `<name>.fcproj` when done, so a half-made project never takes the name |
| engine step | `filmcraft-cli run step-N.jsonl --project <name>.fcproj --save --compact` (the CLI stops at the first failing line, before saving) |
| plugin / agent | its argv in the item folder (agent: in the project's folder), env `BASHCUT_PROJECT`, `BASHCUTPRO_CLI` and `BASHCUT_FILMCRAFT_CLI` (the engine), `BASHCUT_BATCH_ITEM`, `BASHCUT_BATCH_JOB` |
| export | `filmcraft-cli export <out> --project … --preset … --compact`; with a profile, `filmcraft-cli run` on one `bashcut.profiles.apply {profile, path, wait: true}` line |

Exit statuses map to error categories: 0 ok, 1 `command` (`offlineMedia` for an export that drew placeholders), 2
`usage`, 3 `handoff` (item `needsAttention`; once a person or agent has done that step, `retry --items <name>
--done` goes on after it), anything else or a signal `crashed`; plus `timeout`, `cancelled`,
`spawn`, `io`, `tripped`. The message is the last `"ok": false` result line, else the last stderr line.

## Agent steps

Issue #60. `{"agent": {"skill", "brief", "agent"?, "argv"?, "maxTurns"?, "maxMinutes"?, "review"?, "reviewProfile"?,
"fixRounds"?}}` runs a kit skill headless against the item's project:

1. **Start.** `agent: "claude"` (default) runs `claude -p <prompt> --output-format stream-json --verbose --max-turns N
   --mcp-config <items/name/step-N.mcp.json> --allowedTools mcp__bashcutpro`; `agent: "codex"` runs `codex exec --json
   --skip-git-repo-check -c mcp_servers.bashcutpro.…`; an own `argv` gets `{prompt}`, `{session}`, `{mcpConfig}`. The
   MCP server is `bashcut-mcp --headless --project <item> --roots <item folder, outputs>` (writes only there, #51) when
   the runner has one (`--mcp`, `$BASHCUT_MCP` or next to `bashcut-batch`), else `filmcraft-cli mcp --project`. The
   agent works in the projects folder; the prompt names the skill, the brief, the project, where it may write, and asks
   it to save.
2. **Review.** Unless `review: false`, `bashcut.review.run` (#57, `reviewProfile` as its profile) runs on the saved
   project (its markers are saved too).
3. **Fix rounds.** Blocking findings go back to the agent (Claude Code: `--resume <session>`, Codex: `exec resume`),
   at most `fixRounds` times (default 2, at most 5). Findings left after that end the item **needsFix** (category
   `review`), which `retry` reopens.
4. **Caps.** `maxTurns` per round (default 40; hitting it fails the item, category `limit`), `maxMinutes` for the whole
   step (default 30, every round included; `timeout`), and at most `agentConcurrency` agent steps at once across items
   (default 1, separate from `concurrency`).
5. **Book.** The step's `agent` holds the adapter, session, rounds, turns, input / output tokens, cost (as Claude Code
   reports it), time and each round's transcript (`items/<name>/<step>.round-N.out`); its `review` holds the last
   verdict, counts, scores and blocking findings. The run summary (and `status`) totals agent use over the job.

## Job book

`<projectsFolder>/.bashcut-batch/<id>/`: `state.json` (`bashcut-batch-state/1`: job, items, steps with status,
attempts, pid, start / end / duration, error `{category, message}`, outputs), `pipeline.json` (the resolved pipeline
the job runs, so editing the original file does not change a running job), `items/<name>/` (step scripts,
`<step>.out` / `.err` logs), `cancel`, `runner.lock`. It is rewritten (temp file + rename) after every change.

**Tripwire.** A step is marked `running` with its pid before the process starts. A runner that finds a step still
`running` from an earlier run (the runner or the machine died) does not run it again, because it may have crashed
half way: the step becomes `tripped`, the item `needsAttention`, and `resume` reports it. A person or agent checks
the project, then `retry` runs it. Items that were between two steps simply continue.

## Commands

```bash
bashcut-batch run pipeline.json [--id ID] [--concurrency N] [--cli path/to/filmcraft-cli]
bashcut-batch run --preset captions-draft --input clips/ --projects out/   # exports in out/exports
bashcut-batch run --preset captions-draft --input clips/ --projects out/ --outputs done/
bashcut-batch status <job> [--wait]      # job folder printed by run / list
bashcut-batch resume <job>
bashcut-batch retry <job> [--items a,b]
bashcut-batch retry <job> --items a --done   # a's failed / tripped step was finished by hand: go on after it
bashcut-batch cancel <job>
bashcut-batch list <projectsFolder>
bashcut-batch validate pipeline.json     # the items it would make
bashcut-batch presets [list | show NAME | save NAME FILE | install FILE|FOLDER | delete NAME]
                                         # <data dir>/bashcut/batch-presets/
bashcut-batch presets install ../bashcutpro-plugins/packs/batch    # the built-in presets (#59)
```

Output is JSON; exit 0 when every item is done, 1 when the job ended failed, stopped, cancelled or needing
attention, 2 on usage errors. The engine is `--cli`, else `$BASHCUTPRO_CLI` (or the older `$BASHCUT_FILMCRAFT_CLI`,
then `$FILMCRAFT_CLI`), else `bashcutpro` or `filmcraft-cli` next to `bashcut-batch`, else the same on `PATH`
(`crates/bashcut-batch/src/cli.rs`, #154). On Windows a step runs the `filmcraft-cli` beside a `bashcutpro`, so the
memory watch and Cancel reach the engine itself (the wrapper cannot replace itself there as it does on Unix).

## Built-in presets (#59)

`bashcutpro-plugins/packs/batch/`: `vietsub-folder`, `podcast-shorts`, `every-platform`, `captions-draft`,
`clean-audio`, installed with `presets install`, run with `run --preset NAME --input <folder> --projects <out>`.
Their recipes and limits are in that folder's README.

## Decisions and open items

- One process per step (not one per item): every finished step is saved, so resume and the tripwire work per step.
  The cost is reopening the project per step; FilmCraft keeps decoded media per session, so small items are cheap.
- Existing projects are never overwritten: `run` refuses when `<name>.fcproj` exists.
- A `batch` skill for the kit (when to batch, writing a pipeline, checking results) belongs in
  `bashcutpro-agent-kit`; it is left to the kit worker (not in this PR).
- Live check pending: 3 short clips → captions → two exports at `concurrency 2` against a real `filmcraft-cli`.

## Resources (#61)

`resources` in the pipeline (all optional): `memoryPerItemGb` (3), `maxItemMemoryGb` (8; 0 = no cap),
`minFreeDiskGb` (2), `exportReserveGb` (1), `halfCores` (true), `agentConcurrency` (1, #60). Code:
`crates/bashcut-batch/src/governor.rs`.

- **Concurrency**: before every item the runner asks the governor whether one more may start: at most
  `concurrency`, half the cores, and only while `memoryPerItemGb` of RAM is free (one item always runs). Workers above
  that wait and re-check, so the batch widens again when memory frees up.
- **Memory cap**: every 2 s a step's process memory is read; over `maxItemMemoryGb` the process is
  stopped, the step goes back to `pending` and the item to `notRun` with the category `resources`; the run's summary
  says so and `resume` runs it again (by then with fewer items at once if RAM is short). Engine steps save only when
  they finish, so stopping one loses nothing.
- **Disk**: before an item's project is created and before each export, the projects and outputs volumes need
  `minFreeDiskGb` (+ `exportReserveGb` for an export). When low, the job book's `paused` says why and the batch waits;
  it goes on by itself once space is freed, or stops on `cancel`.
- **Sleep**: `bashcut-batch run` keeps the machine awake while it runs (`caffeinate`, `systemd-inhibit`,
  `SetThreadExecutionState`; `--no-keep-awake` turns it off). Timeouts count only time the machine was awake (a gap
  of 30 s or more between two polls is a sleep). After a restart, `resume` continues through the job book (tripwire).
- **Report**: the run's summary has `resources`: peak process memory, items finished, items per hour, the
  concurrency the machine allowed at start, and the count and total / average time of every step label.
- Probes (no extra crates): `/proc` and `df` on Linux, `vm_stat`, `ps` and `df` on macOS, PowerShell / CIM and
  `tasklist` on Windows; an unknown value never limits anything.
- **Not done here** (follow-ups): hardware encoders and proxies for 4K inputs (they belong to the export presets /
  output profiles and the proxy commands), GPU encoder slots in the concurrency.
