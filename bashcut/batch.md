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
    {"agent": {"skill": "rough-cut", "brief": "Keep it under 60 s."}}
  ],
  "outputs": [{"name": "{name}-tiktok.mp4", "profile": "tiktok"}, {"name": "yt/{name}.mp4", "preset": "YouTube 1080p Full HD"}],
  "onFailure": "skip",
  "concurrency": 2,
  "outputName": "{index}-{stem}",
  "projectsFolder": "out/projects",
  "outputsFolder": "out/exports"
}
```

(The command ids above are only an example; use ids from `filmcraft-cli commands`.)

- **inputs**: files, folders (their media files, not recursive; `extensions` overrides the list) and `.fcproj`
  projects. Relative paths resolve against the pipeline file's folder (a preset's against the working folder).
- **template**: `{"project": …}` is copied for every item; `{"commands": […]}` builds a new project instead. Without
  one, a media input gets `file.import {paths: ["{media}"]}` then `file.newSequenceFromClip` (to confirm in the first
  live run). A `.fcproj` input is copied, never edited in place.
- **steps**: exactly one of `command` (+ `params`), `commands` (one process: all saved or none), `plugin` (`argv`,
  `env`: any program; it connects to the project itself, `bashcut-result/1` on stdout, exit 3 = hand-off) or
  `agent` (`skill`, `brief`, optional `argv`, default `claude -p {prompt}`; #60 adds review rounds and caps).
  `timeoutSeconds` on any step.
- **outputs**: `name` (inside the outputs folder, may have subfolders), and either a `profile` (#43: `tiktok`,
  `reels`, `shorts`, `youtube`, `youtube-4k`, `facebook-square`… exported with `bashcut.profiles.apply`, which also
  applies the profile's loudness and caption rules) or `preset`, `format`, `settings` (ExportSettings JSON).
- **description**: one line shown by `presets list`.
- **placeholders** in params, argv, env and output names: `{input}`, `{media}`, `{name}`, `{project}`, `{itemDir}`,
  `{outputs}`, `{cli}` (the filmcraft-cli in use) and `{plugins}` (`<data dir>/bashcut-plugins`, where the plugin
  host installs plugins: `{plugins}/bashcut.translate-captions/plugin.py`); `{prompt}` in agent argv. `outputName`
  takes `{stem}` and `{index}`.
- **onFailure**: `skip` (the item stops, the others go on) or `stop` (nothing new starts; running items stop before
  their next step and become `notRun`).

## How a job runs

Each item has the steps `create`, `step-1`…, `export-1`…:

| Step | Process |
|---|---|
| create | copy (template / input project), or `filmcraft-cli run create.jsonl --save-as <tmp>`; renamed to `<name>.fcproj` when done, so a half-made project never takes the name |
| engine step | `filmcraft-cli run step-N.jsonl --project <name>.fcproj --save --compact` (the CLI stops at the first failing line, before saving) |
| plugin / agent | its argv in the item folder (agent: in the project's folder), env `BASHCUT_PROJECT`, `BASHCUT_FILMCRAFT_CLI`, `BASHCUT_BATCH_ITEM`, `BASHCUT_BATCH_JOB` |
| export | `filmcraft-cli export <out> --project … --preset … --compact`; with a profile, `filmcraft-cli run` on one `bashcut.profiles.apply {profile, path, wait: true}` line |

Exit statuses map to error categories: 0 ok, 1 `command` (`offlineMedia` for an export that drew placeholders), 2
`usage`, 3 `handoff` (item `needsAttention`), anything else or a signal `crashed`; plus `timeout`, `cancelled`,
`spawn`, `io`, `tripped`. The message is the last `"ok": false` result line, else the last stderr line.

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
bashcut-batch run --preset captions-draft --input clips/ --projects out/
bashcut-batch status <job> [--wait]      # job folder printed by run / list
bashcut-batch resume <job>
bashcut-batch retry <job> [--items a,b]
bashcut-batch cancel <job>
bashcut-batch list <projectsFolder>
bashcut-batch validate pipeline.json     # the items it would make
bashcut-batch presets [list | show NAME | save NAME FILE | install FILE|FOLDER | delete NAME]
                                         # <data dir>/bashcut/batch-presets/
bashcut-batch presets install ../bashcutpro-plugins/packs/batch    # the built-in presets (#59)
```

Output is JSON; exit 0 when every item is done, 1 when the job ended failed, stopped, cancelled or needing
attention, 2 on usage errors. `filmcraft-cli` is `--cli`, else `$BASHCUT_FILMCRAFT_CLI`, else the one next to
`bashcut-batch`, else `filmcraft-cli` on `PATH`.

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
