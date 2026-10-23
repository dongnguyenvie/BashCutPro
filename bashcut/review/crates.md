# Code review: our Rust crates and apps (non-UI), 2026-10

Ticket RV2. Scope: `crates/bashcut-{agent,analysis,batch,connect,dirs,plugin-host}`, `crates/engine/src/bashcut/`,
`apps/bashcut-{batch,doctor,mcp}`, `apps/bashcutpro-cli` (build.rs), `xtask/src/bashcut.rs`. Every `.rs` file was read
at `main` `0e7fd3e5`. Goal: one obvious place to change when we add an engine name, a plugin kind, a step kind, an
agent, a CLI consumer or an OS. Behaviour does not change in this round.

Size: **S** < 1 h, **M** half a day, **L** more. **Beh.** = whether the proposed fix changes behaviour.
**done** RV2 = refactored in this ticket (see "Refactors done"). CC1 (control.json discovery) touches `bashcut-plugin-host`,
`bashcut-mcp` and `bashcut-connect`, so fixes there wait for CC1.

## Duplication

| # | Where (all copies) | Why it matters | Fix | Size | Beh. |
|---|---|---|---|---|---|
| D1 | Engine lookup `bashcutpro → bcpro → filmcraft-cli` + env vars: `bashcut-batch/src/cli.rs`, `apps/bashcut-mcp/src/main.rs` `find_cli`, `apps/bashcut-doctor/src/lib.rs` `cli_program` (no `bcpro`, no `BASHCUT_FILMCRAFT_CLI`, no "next to me", probes `bashcutpro --version`) | Doctor can report a different engine from the one batch/mcp run (inside the macOS bundle it misses `bcpro`, #187); a new name = 3 edits | One lookup in `bashcut_dirs::engine` (**done** RV2: batch/UI use it); mcp **done** RCA; doctor → RC2 (needs an injectable lookup for its `Env` fakes) | S | no / doctor yes |
| D2 | Child supervisor (own process group, TERM→grace→KILL, capped line pumps, grandchild-pipe timeouts): `bashcut-plugin-host/src/runner.rs` `start/poll/supervise/terminate/for_each_line` and `bashcut-agent/src/process.rs` same five; `terminate` byte-identical; third partial copy in `engine/src/bashcut/hooks.rs` `run_hook` | A fix (Windows job objects, pipe hang) lands in one copy only; a new runner kind = a 3rd copy | Shared `child` module in a no-deps crate | M | no |
| D3 | `pid_alive`/`alive`: `bashcut-batch/src/process.rs` and `bashcut-connect/src/lib.rs` (byte-identical) | Same | `bashcut_dirs::pid_alive`, both call it (**done** RCA) | S | no |
| D4 | `Error(String)` + impls + `Result` + `err()` + `read_capped`: `bashcut-plugin-host/src/lib.rs`, `bashcut-agent/src/lib.rs`; another `read_capped` in `bashcut-batch/src/pipeline.rs`; `write_atomic` in `bashcut-agent/src/lib.rs`, `bashcut-batch/src/book.rs` | Drift; plugin-host `settings.rs`/`install.rs` write with plain `fs::write`, so a crash mid-save truncates settings.json | One fs helper module (`read_capped`, `write_atomic`); use it in plugin-host settings/install (**done** RC4: `bashcut_dirs::fs` holds `Error`/`Result`/`err`/`read_capped`/`write_atomic`; plugin-host and agent re-export the error, batch's book/presets/pipeline and plugin-host settings/install write through it) | S | no (crash-safety only) |
| D5 | `clip` on a char boundary: `bashcut-agent/src/lib.rs` (adds `…`), `bashcut-plugin-host/src/view.rs`, `ui-egui/.../runs_panel.rs`; `stream_json::capped_input` = `codex_json::capped` | Drift | One `clip(text, max, ellipsis)` (**done** RC4: `bashcut_dirs::fs::clip`, agent + plugin-host view; `runs_panel.rs` clips by chars and is left to a UI ticket); one `capped` (**done** RV2: Codex uses `stream_json::capped_input`) | S | no |
| D6 | Plugin manifest read 3×: `bashcut-plugin-host/src/manifest.rs` (validated), `engine/src/bashcut/hooks.rs` `discover/os_key`, `apps/bashcut-doctor/src/lib.rs` `installed_plugins/entry_program` (raw `consts::OS`, no linux fallback) | A schema change = 3 parsers; hooks/doctor skip validation | Serde manifest + `entry_for(os)` in a no-deps crate the engine (L4) can use | M | edge cases |
| D7 | Thin `data_dir` wrappers: `bashcut-batch/src/presets.rs` `default_data_dir`, `bashcut-connect/src/lib.rs` `default_data_dir`; `opts.data_dir.or_else(...)` repeated in `bashcut-batch/src/runner.rs` and `apps/bashcut-batch/src/main.rs` | Noise; one place to change the rule | Call `bashcut_dirs::data_dir` directly (**done** RV2 for batch, RCA for connect) | S | no |
| D8 | Folder-name literals `"bashcut-plugins"`, `"bashcut-models"`: `engine/src/bashcut/hooks.rs`, `apps/bashcut-doctor/src/lib.rs` (3×), `bashcut-batch/src/runner.rs`, `bashcut-dirs/src/lib.rs`, `bashcut-plugin-host/src/lib.rs` | Renaming a folder misses a copy | `bashcut_dirs::PLUGINS_DIR` (**done** RV2 outside plugin-host, RCA in plugin-host). Note: doctor checks `<data>/bashcut-models` while `bashcut_dirs::models_dir` is `<data>/models` (RC2) | S | no |
| D9 | Hand-written arg parsers: `apps/bashcut-batch/src/main.rs` `Args`, `apps/bashcut-mcp/src/main.rs` (copy), `apps/bashcut-doctor/src/main.rs` (3rd style, ignores a bad `--min-free-gb`) | Every new CLI consumer copies it again | Small shared `args` helper | S–M | doctor yes |
| D10 | Engine rounding: `summary.rs` `s3` = `analysis.rs` `secs`; `round2` in `analysis.rs` and `review.rs`; inline `(x*1000).round()/1000` in `analysis.rs`, `summary.rs`, `captions_fit.rs` | Drift in report precision | `data::round_to(v, places)` (**done** RV2) | S | no |
| D11 | Undo fold twice: `engine/src/bashcut/batch.rs` `Before::take/fold` and `undo.rs` `UndoGroup` (already disagree on order of the `pushed == 0` / history-reset checks) | Divergent undo for a step that opens a project | `Before` wraps `UndoGroup` (**done** RC10: both orders give 0 / false in every case, since a reset leaves `pushed == 0`) | S | no |
| D12 | Engine data base computed twice: `engine/src/bashcut/data.rs` and `hooks.rs` `plugins_dir` | Drift | `data::base(s)` (**done** RV2) | S | no |
| D13 | Batch "engine argv → Spec → spawn → outcome" block 6× in `bashcut-batch/src/runner.rs` (profile export, export, create, review, inspect, engine step) | New engine step = 15 copied lines; env/data-dir args can drift | `run_engine(...)` helper (**done** RV2) | M | no |
| D14 | Tail/cap readers: `bashcut-batch/src/process.rs` `tail` vs `agent.rs` `read_log`; `agent.rs` `cap` vs inline in `runner.rs` | Drift | One `tail`, one `cap` (**done** RC7: `process::tail_raw` / `process::cap`) | S | no |
| D15 | Test benches `Bench`/`max_concurrent` copied in `bashcut-batch/tests/batch.rs` and `tests/governor.rs` (governor copy counts `agent-start` as an end) | Wrong concurrency count in a test | `tests/support/mod.rs` (**done** RC7) | S | test only |

## Inconsistency

| # | Where | Why it matters | Fix | Size | Beh. |
|---|---|---|---|---|---|
| I1 | Timeout: plugin runner kills hard at once (`runner.rs` timeout branch), agent turn goes TERM→grace→KILL | A timed-out plugin cannot clean up its MCP child | Soft first in both (with D2) | S | yes |
| I2 | Hook runs vs plugin action runs: `hooks.rs` `run_hook` gets no settings/secrets env, no `BASHCUTPRO_CLI`, drops stderr, no `catch_unwind` | The documented `check-key` hook example cannot see its key | Share the env builder + child module (D2, D6) | M–L | yes |
| I3 | Plugin value checks: `settings::check` vs `manifest::param_text` (bool-from-string only in settings; 16 KiB vs 64 KiB cap; min/max only for params) | Same field type accepts different input | One `coerce(kind, …)` | S–M | yes |
| I4 | Registry location parsed 3 ways: `host.rs` `Host::new` (`split_location`), `host.rs` registry command (`is_file()` first), `install.rs` `fetch_registry` | Local registry shows as `path@main` vs `path@` | `registry::Location::parse` | S | label only |
| I5 | Batch UI vs CLI options: `ui-egui/.../batch.rs` `options()` sets only `data_dir`; CLI also resolves kit, MCP, keep-awake (`find_kit`/`find_mcp` live in the app) | The same pipeline behaves differently from the UI | `Options::resolve` in the library | M | yes |
| I6 | `apps/bashcut-batch` makes `--cli` absolute but not `--data-dir`/`--mcp`, though steps run in the item folder | A relative `--mcp` breaks agent steps | `pipeline::absolute` on both | S | yes |
| I7 | `speech::engine_models` and doctor's speech probe use `Command::output()` with no timeout / inherited stdin | A stuck engine blocks the run start | `run_watched` with a timeout | S | yes |
| I8 | Crate manifests: `bashcut-dirs` lacked `[lints] workspace = true`; `apps/bashcut-doctor` lacks `rust-version.workspace`; `publish = false` missing on most of ours; `apps/bashcut-mcp` lacks the crate-level `deny(clippy::unwrap_used, …)` the other apps have | Never-crash lints silently off for a crate | Add the lines (`bashcut-dirs` lints **done** RV2, mcp `deny` **done** RCA) | S | no |
| I9 | Background task panic reported as `Task::Install` even for a registry fetch (`host.rs` `spawn_task`, `Disconnected` branch) | Misleading status | Carry the task kind | S | no |
| I10 | `KIT_TOOLS` in `bashcut-batch/src/agent.rs` hard-codes `python3/python/py`; `python.rs` probes its own list | New OS/interpreter = 2 edits | Build one from the other (**done** RC7: `agent::kit_tools()` from `python::AGENT_PYTHONS`) | S | no |

## Scalability

| # | Where | Why it matters | Fix | Size | Beh. |
|---|---|---|---|---|---|
| S1 | New agent event format: `manifest.rs` `AGENT_EVENTS` (only `claude-stream-json`), `bashcut-agent/src/process.rs` `check` + `Decoder`, `stream_json::FORMAT` redefined; no trait despite `in-app-ui.md` | An agent plugin cannot declare `codex-json` though the crate supports it | Format registry in `bashcut-agent`; manifest accepts its names | M | yes |
| S2 | `AgentLaunch` (`host.rs`) hand-mirrors part of `AgentSpec`, copied field by field in `ui-egui/.../agent.rs`; no `prompt_args`/`timeout_secs`/mcp templates in the manifest | A Codex-style agent plugin cannot be described | Serde-compatible `AgentLaunch` = `AgentSpec` subset | M | additive |
| S3 | New batch step kind touches 7 places (`Step` fields, `kind`, `describe`, `validate`, `run_step`, `speech_model`, UI) | Cost of each new step | Step-kind table/trait | L | no |
| S4 | New batch agent touches `AGENTS`, `agent::argv`, `read_round` (mixed Claude/Codex match) | Cost of each new agent | `AgentAdapter` table | M | no |
| S5 | OS mapping in ~5 places: `plugin-host/src/lib.rs` `os()`, `manifest.rs` `entry_for`, `registry.rs` `Entry::supports` = `Manifest::supports`, `settings.rs` `cfg!` chains, `bashcut-dirs` | A new OS = 5 edits, and they already disagree on naming | One `Os` enum in `bashcut-dirs`; OS-keyed map in the manifest | M | no |
| S6 | `engine/src/bashcut/errors.rs` `SLOTS: [Run; 96]` positional trampolines | Command 97+ silently loses typed errors (34 today; only a test catches it) | More slots (S) or a dispatch-side hook (M, upstream) (**done** RC10: `spec!` in `bashcut/mod.rs` wraps each `run` in its own fn; no table) | S/M | no |
| S7 | `bashcut-batch/src/pipeline.rs` `item_names` / `expand_inputs` dedup are O(n²–n³) with up to 10 000 inputs | Same file name on every card (`C0001.MP4`) ≈ hang | `HashSet` (**done** RV2: `item_names` and `expand_inputs`) | S | no |
| S8 | `book.rs` reads `state.json` with a 4 MB cap but writes it uncapped, pretty; `Shared::with` rewrites the whole book on every step event | A large job is created but cannot be resumed; quadratic writes | Bigger cap + compact JSON (**done** RCC: 256 MB on read and write); coalesce writes | S/M | yes |
| S9 | `bashcut-agent/src/store.rs` `list` loads up to 500 conversations in filesystem order, then sorts | Past 500 the newest can be missing | Sort by mtime first | M | yes |
| S10 | Plugin-host OS key store shells out with no timeout (`settings.rs` `tool`), on the UI thread | `secret-tool` unlock prompt freezes the app | Timeout runner (with D2) (**timeout done** RCC: 20 s; still on the UI thread) | S | yes |
| S11 | `bashcut-agent` `write_mcp_config` names the file by pid only and reuses it when the bridge changes | Two agents share/delete it | Name by bridge; rewrite on change | S | yes |

## Dead code

| # | Where | Fix | Size | Beh. |
|---|---|---|---|---|
| X1 | `bashcut-batch/src/cli.rs` `engine(cli)` is the identity (wrapper gone since SB2) | Inline (**done** RV2) | S | no |
| X2 | `governor::Limits::agent_concurrency` validated, never read (runner uses the top-level `agentConcurrency`), yet documented in `batch.md` | Remove or alias; fix doc | S | no / yes |
| X3 | `bashcut_dirs::filmcraft_dir` (no outside callers), `Running::wait` (tests only), `host::job_summary` (internal), `Turn::is_finished`/`Running::is_finished` (unread), `bashcut-analysis` `loudness::measure`, `beats::click_track` (tests only) | `pub(crate)` or `#[cfg(test)]` | S | no |

## Docs drift

| # | Where | Fix | Size |
|---|---|---|---|
| T1 | `bashcut/README.md`: `bashcutpro` "runs upstream's `filmcraft-cli` beside it" (wrapper removed in SB2) | Reword (**done** RV2) | S |
| T2 | `bashcut-batch/src/lib.rs` and `bashcut/batch.md` say "no workspace deps" (it uses `bashcut-dirs`); `bashcut/agent-connect.md` says the same of connect | Fix (**done** RV2 for batch) | S |
| T3 | `bashcut/in-app-ui.md`: `bashcut-analysis` deps (only `audio-dsp`), "xtask knows three crates" (ten), `filmcraft-cli mcp --bridge` (engine is `bashcutpro` first) | Fix | S |
| T4 | Help text: `bashcut-mcp` / `bashcut-batch` `--cli` omit `bcpro`; `--data-dir` says "FilmCraft data dir"; doctor speech message always says "(filmcraft-cli)" | Fix (help-only; mcp **done** RCA) | S |
| T5 | `bashcut/plugin-host.md` "killed after its declared timeout" (no manifest field; `DEFAULT_TIMEOUT` or `timeoutSeconds` param) | Fix | S |
| T6 | No list of the 34 `bashcut.*` commands (`commands.md` is upstream's) | Generate from `bashcutpro commands bashcut --json` + drift test | S–M |
| T7 | `bashcutpro-cli/build.rs` blanket `filmcraft-cli → bashcutpro` replace would also rename a future protocol string; anchors are tripwires, not documented as such | Comment + narrow the replace | S |

## What adding X costs today

- **An engine name / env var:** one edit in `bashcut_dirs::engine` after RV2, plus doctor (RC2) until it lands (mcp uses it since RCA).
- **A plugin kind / agent event format:** manifest constants + agent decoder + UI mirror (S1, S2).
- **A batch step kind:** 7 places (S3). **A batch agent:** 3 places (S4).
- **A CLI consumer:** copy an arg parser (D9) and call `bashcut_dirs::engine::default_cli`.
- **An OS:** ~5 places (S5).

## Refactors done in RV2

Each fix marked **done** RV2 above; all behaviour-neutral, gated with fmt, clippy `-D warnings` on the touched
crates and the upstream footprint (speed mode: no new tests; existing ones unchanged).

| PR | What |
|---|---|
| #269 | This review |
| #270 | Engine lookup in `bashcut_dirs::engine` (D1 for batch and the UI), identity `cli::engine` inlined (X1), README (T1) |
| #272 | `bashcut-dirs` workspace lints (I8), `bashcut_dirs::PLUGINS_DIR` (D8) |
| #274 | Batch: `run_engine` (D13), `HashSet` dedup (S7), `bashcut_dirs::data_dir` directly (D7), docs/help (T2, T4) |
| #275 | Engine: `data::round_to` / `data::secs` (D10), `data::base` (D12) |
| #277 | Agent: one `capped_input` (D5, part) |

After CC1, RCA did RC1 (mcp lookup, help, `deny`), RC11 (`bashcut_dirs::pid_alive`, connect's `default_data_dir`
gone) and `PLUGINS_DIR` in `bashcut-plugin-host`.

## Proposed tickets (prefix RC)

| Key | Owns | Do | Acceptance | Same files as |
|---|---|---|---|---|
| RC1 | `apps/bashcut-mcp` | After CC1: `find_cli` → `--cli` else `bashcut_dirs::engine::default_cli`; help text (T4); `deny(clippy::unwrap_used…)` (I8) | No copy of the name list in mcp; gate green | CC1 |
| RC2 | `apps/bashcut-doctor` | `cli_program` uses the shared lookup through an injectable `Host`-style seam (D1); manifest read via RC5 later | Doctor finds `bcpro` in a bundle; test with fake env | RC5 |
| RC3 | `bashcut-plugin-host`, `bashcut-agent`, `engine/src/bashcut/hooks.rs` | Shared child supervisor (D2), soft-first timeout (I1), key-store timeout (S10), hooks reuse it (I2) | One `terminate`; hook gets settings env | RC4, RC5 |
| RC4 | `bashcut-plugin-host`, `bashcut-agent`, `bashcut-batch` | Shared fs helpers + error type (D4), `clip` (D5), `write_atomic` for settings/install | No duplicate `read_capped`/`write_atomic` | RC3 |
| RC5 | new no-deps manifest crate, plugin-host, hooks, doctor | One manifest reader + `Os` enum (D6, S5, `Entry::supports`) | 1 parser; FreeBSD maps the same everywhere | RC2, RC3 |
| RC6 | `bashcut-agent`, plugin-host manifest, `ui-egui/.../agent.rs` | Agent format registry + serde `AgentLaunch` (S1, S2) | A `codex-json` agent plugin validates and runs | RC5 |
| RC7 | `bashcut-batch` | `engine_run` helper (D13), `tail`/`cap` (D14), test support module (D15), `KIT_TOOLS` (I10) | `runner.rs` has one spawn block for engine steps | RC8 |
| RC8 | `bashcut-batch`, `apps/bashcut-batch`, `ui-egui/.../batch.rs` | `Options::resolve` (I5), absolute `--data-dir`/`--mcp` (I6), engine-models timeout (I7), book cap + compact writes (S8), `agentConcurrency` (X2) | UI and CLI build the same options | RC7 |
| RC9 | `bashcut-batch` | Step-kind table (S3) and agent adapter table (S4) | New step = one table row + its run fn | RC7 |
| RC10 | `engine/src/bashcut/` | `round_to` (D10), undo fold via `UndoGroup` (D11), `data::base` (D12), `CommandSpec` helper, errors slots (S6) (**done** RC10: `spec!`) | Existing engine tests green | — |
| RC11 | `bashcut-connect`, `bashcut-batch` | After CC1: `pid_alive` in one place (D3); drop connect's `default_data_dir` (D7) | No duplicate | CC1 |
| RC12 | `bashcut/`, crates manifests | Remaining docs drift (T3, T5, T6, T7), manifest fields (I8), dead code (X3), `store::list` order (S9), `write_mcp_config` (S11) | Docs match code | — |
