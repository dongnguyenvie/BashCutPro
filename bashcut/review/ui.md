# Code review: in-app UI (`crates/ui-egui/src/bashcut_ui/`), RV1

Review round of 2026-10-23 (owner request: duplication, standards, scalability). Scope: every file in
`crates/ui-egui/src/bashcut_ui/` and `crates/ui-egui/tests/bashcut_ui.rs`, at `main` 0e7fd3e5.
Behaviour must not change in this round; findings that would change behaviour are marked so and
left as proposed tickets (prefix `RU`).

Size: S = under an hour, one file or a mechanical sweep; M = half a day; L = a day or more.

## What is already in good shape

- **Strings**: every user-facing literal goes through `strings::s` / `strings::f` (the only bare
  literals are the `"batch"` hint text in `batch_panel.rs:545`, which is a file-name default, and
  test asserts). One catalogue (`strings/vi.tsv`), one lookup.
- **Colours**: no `Color32` literal in the module; everything uses `app.tokens` (`crate::theme::Tokens`).
- **Status lines**: all four panels with a top status line already share `toast::status_line`
  (`agent_panel.rs:103`, `batch_panel.rs:109-110`, `plugins_panel.rs:236`, `runs_panel.rs:152`).
- **Integration tests** (`tests/bashcut_ui.rs`): every test builds its app through `Driver`
  (`Driver::demo` / `Driver::with_data` / the registry variant); `FilmcraftApp::new` appears only in
  the two `Driver` constructors. Two unit tests build an app by hand on purpose (they draw one
  function into a bare `egui::Context`): `runs_panel.rs:380`, `palette.rs:911`; that is fine.
- **Locks**: every mutex goes through `unwrap_or_else(PoisonError::into_inner)`.

## Duplication

### D1. Dim "note" label, ~60 copies, two sizes
`ui.label(egui::RichText::new(text).size(11.0|10.5).color(t.text_dim))` and the same with
`t.danger` for inline errors. Counts per file (text_dim): agent_panel 11, batch_panel 14,
plugins_panel 11, runs_panel 6, library_panel 5, looks_panel 4, view 4, palette 3, rail 1,
side_panel 1. Examples: `agent_panel.rs:112,117,124,155,160,166,240,312,315`,
`batch_panel.rs:128,130,200,210,270,281,309,321,495,520,528,589,615,618,657,662,671`,
`plugins_panel.rs:231,241,259,270,296,300,305,324,337,366,377,464,509,511`,
`library_panel.rs:381,433,441,482,540`, `looks_panel.rs:218,285,309,376`, `runs_panel.rs:156-157`,
`palette.rs:727,790,792`, `rail.rs:116,124`.
- Why: the text size is a per-call magic number. Batch uses 10.5 for nearly everything, the other
  panels 11.0 for notes and 10.5 for meta lines, with no rule; a theme change means touching ~80 lines.
- Fix: two helpers in a new `bashcut_ui/widgets.rs`: `note(ui, t, text) -> Response` (11.0,
  `text_dim`) and `meta(ui, t, text)` (10.5, `text_dim`), plus `error_note` (`danger`); size
  constants `NOTE_SIZE` / `META_SIZE` next to them. Sweep call sites keeping each site's current
  size, so pixels do not change. Unifying the sizes is a UX decision (ticket RU2).
- Size: S per panel (M for the full sweep). Behaviour-neutral if sizes are kept.

### D2. Two `button` helpers that differ silently
`library_panel.rs:349` (`Button::new(label).small()`) and `plugins_panel.rs:156` (normal size,
`pub`, also used by others). Same signature, same doc comment, different look.
- Why: a reader copies one and gets the other's size; there is no "small vs normal" rule.
- Fix: one `widgets::button(app, ui, id, label, enabled)` and `widgets::small_button(...)`; Library
  calls the small one. Size S, behaviour-neutral. (Waits for CC1: it touches `plugins_panel.rs`.)

### D3. Context-held shared host, three copies
`plugins.rs:18-35` (`host`, `existing`), `agent.rs:17-48` (`host`, `with`), `batch.rs:248-260`
(`shared`, `lock`); each is `get_temp::<Arc<Mutex<T>>>(key())` or create-and-insert, plus a
poison-tolerant lock.
- Why: the next long-lived service (a second agent kind, a downloader) copies it a fourth time;
  the copies already differ (`plugins::existing` exists, `agent::poll` inlines the same lookup).
- Fix: `state::shared::<T>(ctx, id, init) -> Arc<Mutex<T>>`, `state::existing::<T>(ctx, id)`,
  `state::lock(&Arc<Mutex<T>>)`. Size S, behaviour-neutral.

### D4. Per-panel temp state get/put, six copies
`key()/get()/put()` around `ctx.data_mut(get_temp_mut_or_default::<T>(key()).clone())`:
`looks_panel.rs:70-81`, `library_panel.rs:111-122`, `palette.rs:179-190`, `state.rs:68,87-101`,
`plugins_panel.rs:147-153` (`drafts`), `agent_panel.rs:36`, `runs_panel.rs:33`, `toast.rs:39`,
`batch_panel.rs:37`.
- Fix: a tiny generic `temp::<T: Clone + Default>(ctx, id)` / `put_temp(ctx, id, v)` (or a trait with
  a `KEY` const and default `get`/`put`/`update` like `State` has). Size S, behaviour-neutral.

### D5. JSON prefs file load/save, five copies, non-atomic
`looks_panel.rs:91-125` (looks-ui.json), `library_panel.rs:128-155` (library-ui.json),
`palette.rs:~440-470`, `state.rs:196-230` (ui.json), `batch.rs:202-230` (batch.json). Each does
"loaded once; size cap; read; parse or Null; remember the saved text; on save compare, mkdir,
`fs::write`, `log::warn`".
- Why: bug risk: `fs::write` is not atomic, so a crash or full disk mid-write leaves a truncated
  file (load then silently drops the prefs). Five places to fix. Scale: every new panel with prefs
  copies it again.
- Fix: one `prefs::Store { path, saved: Option<String> }` with `load(max) -> Value` and
  `save(&Value)` that writes `path.tmp` then renames. Size M. The atomic write changes behaviour on
  failure only (RU4); the extraction alone is neutral.

## Inconsistency

### I1. Text sizes 10.0 / 10.5 / 11.0 chosen per site
See D1; `agent_panel.rs:338` also uses 10.0 for tool input. No documented scale in
`bashcut/in-app-ui.md`. Fix: document three sizes (note 11, meta 10.5, code 10) with D1's
constants. Size S (docs + constants).

### I2. Layout constants inline
`rail.rs:111` (tooltip max width 260), `side_panel.rs:57-59` (header button 20px, offsets 16/14),
`side_panel.rs:65` (body insets 10/6/8/6), `side_panel.rs:117` (first window 56 px down, 520 high).
Fine as private constants, but they are bare numbers; give them names like `HEADER` already has.
Size S, neutral.

### I3. Error strings of `bashcut.ui.*` commands are English-only
`mod.rs:206,224,233,239` (`"section must be one of …"`, `"width (px) is required"`,
`"no section open; pass section"`). They go to the control channel / MCP / palette, and the
palette shows them to the user. Whether command errors are translated is a product decision
(other `bashcut.*` invoke errors are English too). Proposed ticket RU5, changes behaviour.

## Scalability

### S1. "Adding a section is one row in `SECTIONS`" is not true
`mod.rs:8` says so, but a new section also needs:
- `mod.rs:257-260` `params()`: the section list `"agent|batch|looks|library|plugins|runs"` is
  typed out three times; a new section is missing from the palette's parameter hint until someone
  remembers.
- `rail.rs:90-96`: badges are a `match` on section ids (`"runs"`, `"batch"`), and `rail.rs:49`
  special-cases `"agent"` for its shortcut.
- `mod.rs:149`: `runs_visible` hard-codes `"runs"` (also `rail.rs:63`).
- `mod.rs:170-186`: command routing is a chain of `strip_prefix` per module, and `mod.rs:291-296`
  repeats the same module list for `commands()`.
- Fix (neutral): build the `params()` section list from `SECTIONS` (needs `String` or a
  `OnceLock<String>`; done in this round only if covered, see "Done"); move `badge` and `shortcut`
  into optional `Section` fields (`badge: Option<fn(&egui::Context, &State) -> Option<Badge>>`,
  `shortcut: Option<&'static str>`); one `MODULES: &[(&str prefix, invoke fn, commands fn)]` table
  for routing + commands. Size M overall; the `params()` part is S.

### S2. Inline-reading "on screen" in two places
`rail.rs:62` (`on_screen` closure) and `mod.rs:149` compute "docked or popped out" separately.
Fix: `State::on_screen(id)`. Size S, neutral.

## Dead code

None found by reading or by `rustc` (the crate builds with `-D warnings`, so unused private items
would already fail). `rail::Badge` is `pub` but only used inside `rail.rs`; harmless.

## Docs drift

- `mod.rs:8` (see S1).
- `bashcut/in-app-ui.md` has no section on shared widgets / text sizes (I1); add it with D1.

## Done in this round

Behaviour-neutral, merged to `main`:
- S2: `State::on_screen` replaces the two hand-written "docked or popped out" checks (#268).
- S1 (part): unit test `bashcut_ui::tests::params_list_every_section` keeps the `params()` section
  hints in sync with `SECTIONS`; the `mod.rs` header now says what adding a section really takes (#268, this PR).
- D2: `widgets::button` / `widgets::small_button` (new `widgets.rs`); Library's private copy is gone (#271).
- D3: `shared::get_or_init` / `existing` / `lock` (new `shared.rs`) hold the plugin host, agent and
  batch; unit test `shared::tests::created_once_then_found`.

Everything else is below.

## Proposed tickets

| Key | Owns | Do | Acceptance | Touches same files as |
|-----|------|----|------------|------------------------|
| RU1 | `bashcut_ui/widgets.rs`, all `*_panel.rs`, `rail.rs`, `palette.rs`, `view.rs` | D1 + D2: `note` / `meta` / `error_note` / `button` / `small_button` helpers; sweep every call site keeping its current size | no `RichText::new(..).size(..).color(t.text_dim)` left outside `widgets.rs`; screenshots unchanged; `bashcut_ui` tests green | RU2, RU3 (panels). Start after CC1 merges (`plugins_panel.rs`). |
| RU2 | `widgets.rs`, `bashcut/in-app-ui.md` | I1: pick one size per role (UX decision), document the scale | doc section "Text"; one size per role in code | RU1 |
| RU3 | `shared.rs`, panels' `key/get/put` | D4 (D3 done): generic temp-state helper next to `shared.rs` | no hand-written `get_temp_mut_or_default` outside the helper; tests green | RU1 (panel headers) |
| RU4 | new `bashcut_ui/prefs.rs`, `looks_panel.rs`, `library_panel.rs`, `palette.rs`, `state.rs`, `batch.rs` | D5: one prefs store, atomic write (tmp + rename) | unit test: an interrupted write (tmp left behind) never truncates the real file; each panel's existing load/save tests green | RU3 |
| RU5 | `mod.rs`, other `invoke` fns, `strings/vi.tsv` | I3: decide whether `bashcut.*` command errors are translated; if yes, route through `s()`/`f()` with vi rows | decision in `bashcut/decisions.md`; errors in both languages | — |
| RU6 | `mod.rs`, `rail.rs`, `state.rs` | S1 + S2: section badge / shortcut as `Section` fields, `State::on_screen`, one module table for routing + `commands()` | adding a test-only section needs one `SECTIONS` row + its `draw`; `mod.rs:8` true again | RU1 (rail.rs). After CC1 (rail menu). |
| RU7 | `side_panel.rs`, `rail.rs` | I2: name the inline layout numbers | no bare px literal in layout code | RU6 |
