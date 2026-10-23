# Rebrand: BashCutPro instead of FilmCraft / ArtCraft

Issue #25. A modified FilmCraft must remove the ArtCraft marks and not be called ArtCraft
(`docs/brand/LICENSE-brand.txt`, "Forks and modified versions"); saying "based on FilmCraft by the
ArtCraft team" in plain text is allowed. This page lists **every upstream file we changed for that**,
so an upstream merge knows where to look. When one of these conflicts, keep upstream's change and
re-apply ours.

## Rules

- Only what a user sees changes. Crates (`filmcraft-*`), binaries (`filmcraft`, `filmcraft-cli`),
  modules, command ids (`help.discord`…), automation ids, `FILMCRAFT_*` env vars, file
  formats (`.fcproj`, `.fcgt`, UTI `ai.storyteller.filmcraft.project`) and `filmcraft://` resources keep
  their names.
- **One exception, the command line (#154, owner decision 2026-10-20).** Users and agents type the CLI's name, so
  Pro ships it as `bashcutpro-cli`, short alias `bcpro` (NM1: the pair follows upstream's `filmcraft` /
  `filmcraft-cli`; the GUI is `bashcutpro`), built from upstream's source by `apps/bashcutpro-cli`
  (`build.rs` copies `apps/filmcraft-cli/src` and renames the help/usage/version text; SB2, `decisions.md`).
  Upstream's package, binary, `Cargo.toml` and packaging are not renamed: the installers ship `bashcutpro-cli` and
  `bcpro` in place of `filmcraft-cli` through `bashcut/packaging/generate.py` (`RENAMES`), so they never clash with an
  installed FilmCraft. Our tools look for `bashcutpro-cli`, then `bcpro`, then `filmcraft-cli` (dev builds), never plain `bashcutpro` (the Linux GUI); the env var is `BASHCUTPRO_CLI`,
  falling back to `FILMCRAFT_CLI`. Not `bcp` (SQL Server's bulk copy tool) and not `bashcut` (Swift BashCut).
- Upstream files are never deleted: `docs/brand/` (the ArtCraft marks) and `assets/app-icon/`
  (FilmCraft's owl) stay in the repository, as the brand licence allows for copies of the
  repository; our builds and pages just don't use them.
- Our assets are new files in [`brand/`](brand/README.md), each with a `.attribution` sidecar and a
  row in `ATTRIBUTION.md` (section "BashCutPro assets").
- Branded UI strings are replaced at lookup time by `crates/ui-egui/src/bashcut_brand.rs` (new file),
  so upstream's English strings and its 12 `i18n/*.tsv` catalogs stay untouched. The replacements
  are English in every language.

## Names and ids

| What | Upstream | BashCutPro |
|---|---|---|
| Product name (bundle, installer, packages) | FilmCraft | BashCutPro |
| macOS bundle id | `ai.storyteller.filmcraft` | `com.birchapps.bashcutpro` (owner to confirm) |
| GUI app | `filmcraft` | `BashCutPro.app/Contents/MacOS/BashCutPro`, `bashcutpro.exe`, `/usr/bin/bashcutpro` (upstream's source still builds `filmcraft`; packaging renames it) |
| Command line | `filmcraft-cli` | `bashcutpro-cli`, alias `bcpro` (`bashcutpro-cli.exe` + `bcpro.exe`; Linux `bcpro` is a symlink; generated from `filmcraft-cli`'s source and shipped in its place, NM1) |
| Release file prefix | `filmcraft-<ver>-…` | `bashcutpro-<ver>-…` (`BRAND_SLUG`) |
| Windows MSI UpgradeCode | `31E6FB14-…` | `B6658EBC-E524-4423-AA52-C08E9B38AACB` (new; never change it again) |
| Manufacturer / vendor | Learning Machines LLC | Birch Apps (owner to confirm) |
| Linux package name | `filmcraft` | `bashcutpro` |
| Data folder (SB1) | `~/Library/Application Support/FilmCraft`, `%APPDATA%\FilmCraft`, XDG `filmcraft` | `…/BashCutPro`, `%APPDATA%\BashCutPro`, XDG `bashcutpro` (`crates/bashcut-dirs`; `BASHCUTPRO_DATA_DIR`, then `FILMCRAFT_DATA_DIR`, then portable `<exe dir>/data`); engine hook in `autosave::default_data_dir` |
| Previews folder (BR1) | `FilmCraft Previews/<project>` next to a saved project and in the temp dir | `BashCutPro Previews/<project>`; an existing `FilmCraft Previews/<project>` of an older project is kept (`crates/engine/src/bashcut/brand.rs`, two one-line hooks in `previews.rs`; the engine's unit tests keep upstream's name) |
| App command-line output (BR1) | `filmcraft 0.10.0`, `Usage: filmcraft …`, `filmcraft: <error>`, message box FilmCraft | `bashcutpro …` and BashCutPro (`apps/filmcraft/src/bashcut_cli.rs`, two one-line hooks in `main.rs`; `FILMCRAFT_*` variable names stay) |
| Log file (BR1) | `<data dir>/Logs/filmcraft.log` (+ `filmcraft.<n>.log`) | `<data dir>/Logs/bashcutpro.log` (+ `bashcutpro.<n>.log`; carried edit in `apps/filmcraft/src/logging.rs`) |
| Log's first line (BR2) | `FilmCraft <version> (<os> <arch>)` | `BashCutPro <version> …` (`bashcut_cli::log_message`, called from the carried edit in `logging.rs`) |
| UI strings that name the app (BR2) | `FilmCraft Project` (open/save filter, File ▸ Export ▸ Selection as …), crash/restore notices, `FilmCraft hit an error`, RAM and media-cache notes, `Not supported in/by FilmCraft…`, graphics template | `BashCutPro …` through `bashcut_brand::OVERRIDES` (in Vietnamese from `bashcut_ui/strings/vi.tsv` when our strings are); `.fcproj`, `.fcgt` and `application/x-filmcraft` stay |
| Help ▸ Help… (BR3) | `FilmCraft Help…` opens github.com/storytold/filmcraft/tree/main/docs | `BashCutPro Help…` opens our guide, `bashcut/guide/en` (`bashcut/guide/vi` when our strings are Vietnamese; `bashcut_brand::help_url`, carried edits in `panels/keyboard.rs` and `links.rs`) |
| Window-creation failure (BR3) | message box `FilmCraft` / "FilmCraft could not start its window: …" | `BashCutPro` / "BashCutPro could not start its window: …", Vietnamese when `bashcut/ui.json` picks it (hook in `main.rs`, `bashcut_cli::window_failed`). eframe's app name in `run_native` stays `FilmCraft`: eframe 0.36 uses it only as the fallback title and app id (both set by our hooks) and for its storage folder, which this build doesn't have (no `persistence` feature; decisions.md 2026-10-22) |
| Crash log, logs without a data dir (BR3) | `=== filmcraft-ui-egui FilmCraft <ver>`, `<temp>/FilmCraft Logs` | `=== … BashCutPro <ver>`, `<temp>/BashCutPro Logs` (carried edits in `crash.rs`, `menu_dialogs.rs`) |
| macOS app menu, App Nap, Linux dev entry (BR3) | app submenu `FilmCraft`, activity reason "FilmCraft plays…", dev `Name=FilmCraft` | `BashCutPro` (hook in `native_menu.rs`; carried edits in `app_nap.rs`, `dev_icon.rs`; the dev entry is `com.birchapps.bashcutpro.desktop` with icons of that name, matching the window app id, CK1). macOS draws the bold menu-bar title from the bundle's `CFBundleName` (BashCutPro in the packaged `.app`), not from the submenu text: an unbundled `cargo xtask dev` run shows the binary name `filmcraft` there while the app menu's items say BashCutPro (checked on macOS 26, CK1) |
| README in packages (BR1) | upstream's `README.md` | `bashcut/packaging/share/README.md` (en + vi) in every archive with a README (Linux, FreeBSD, macOS CLI zip, web, Windows portable zip; `generate.py`); the MSI and DMG ship none; the setup folders (`share/bashcutpro`, `Resources/bashcut`) get only `bashcut-setup.*` (BR2) |
| Linux app id / desktop file | `ai.storyteller.filmcraft` | `com.birchapps.bashcutpro`: desktop file, metainfo, MIME file, hicolor icon names, Flatpak id (SB4, generated by `bashcut/packaging/generate.py`); the window class too (`StartupWMClass`; the app sets `bashcut_brand::APP_ID`, SB5) and `.fcproj` keeps `application/x-filmcraft` |
| Community link (`help.discord`) | discord.gg/artcraft | github.com/dongnguyenvie/BashCutPro/discussions |
| Website (`help.website`) | getartcraft.com | bashcut.birchapps.com |
| App page (`help.appPage`) | getartcraft.com/apps/filmcraft | github.com/dongnguyenvie/BashCutPro/releases |
| Source, issues | github.com/storytold/filmcraft | github.com/dongnguyenvie/BashCutPro |

## Upstream files touched

Hooks and carried edits are also in `UPSTREAM.md`, which `bashcut/scripts/upstream.sh footprint` checks. Since #39 the
files upstream changes often (`dialogs.rs`, `main.rs`, `header.rs`) carry one hook line each; `brand.rs`, `links.rs`,
`build.rs` and packaging stay edited because upstream rarely touches them and a hook can't do the job (reasons in
`UPSTREAM.md` "Carried edits").

| File | Lines | What |
|---|---|---|
| `crates/ui-egui/src/brand.rs` | 2 | `include_bytes!` → our wordmarks |
| `crates/ui-egui/src/lib.rs` | 1 | `pub mod bashcut_brand; // BashCutPro…` (hook) |
| `crates/ui-egui/src/i18n/mod.rs` | 4 | `Language::tr` asks `bashcut_brand::text` first (not in this crate's unit tests, so upstream's catalog tests stay as they are) |
| `crates/ui-egui/src/links.rs` | 5 + test | URL constants; the scheme test checks our URLs |
| `crates/ui-egui/src/control.rs` | 1 (hook) | `bashcut_brand::engine_commands!` answers `engine.commands` through `bashcut_brand::commands`, which calls the `bashcut-brand` crate's rule shared with `bashcutpro-cli` (RC14): "FilmCraft" → BashCutPro in labels, menus and params, `filmcraft-cli` → `bashcutpro-cli` (BR6; English, ids unchanged) |
| `crates/ui-egui/src/header.rs` | 1 (hook) | community button label from `bashcut_brand::community_button` (the automation label stays upstream's) |
| `crates/ui-egui/src/panels/dialogs.rs` | 1 (hook) | About ▸ About is drawn by `bashcut_brand::about_tab`: heading `NAME`, website label from `links::WEBSITE`, a NOTICE link (`about.notice`) |
| `apps/filmcraft/src/main.rs` | 1 (hook) | `.with_icon(bashcut_brand::app_icon())` after upstream's: window / Dock icon from `bashcut/brand/app-icon/` |
| `apps/filmcraft/src/logging.rs` | 3 + docs + tests | `LOG_FILE` and the rotation name say `bashcutpro` (BR1); each message goes through `bashcut_cli::log_message` (BR2) |
| `crates/engine/src/previews.rs` | 2 (hooks) | `previews_temp_first!` / `previews_dir_first!`: `BashCutPro Previews` (BR1) |
| `apps/filmcraft/src/main.rs` | 2 (hooks) | `mod bashcut_cli;` and `bashcut_cli::tell_first!` in `tell` (BR1) |
| `apps/filmcraft/build.rs` | 5 | Windows exe icon, ProductName, FileDescription, LegalCopyright |
| `packaging/env.sh` | 3 | sources `bashcut/brand/packaging.env` (`BRAND_NAME`, `BRAND_SLUG`, `BRAND_ICON_DIR`); `copy_docs` also copies `NOTICE` |
| `packaging/macos/package.sh` | 8 | `$BRAND_NAME.app`, executable, `.icns`, DMG volume, `$BRAND_SLUG-…` file names |
| `packaging/macos/Info.plist.in` | 8 | display name, name, executable, bundle id, icon file, microphone text, copyright |
| `packaging/windows/filmcraft.wxs` | 13 | name, manufacturer, UpgradeCode, component GUID, URLs, install folder, shortcut |
| `packaging/windows/package.ps1` | 5 | icon path, `bashcutpro-…` MSI / zip names, `NOTICE` in the zip |
| `packaging/linux/package.sh` | 7 | maintainer, `$BRAND_SLUG-…` names, icons from `$BRAND_ICON_DIR`, AppImage update repo |
| `packaging/linux/nfpm.yaml` | 4 | package name, vendor, homepage, description |
| `packaging/linux/ai.storyteller.filmcraft.metainfo.xml.in` | 8 | name, developer, description, URLs |
| `packaging/linux/ai.storyteller.filmcraft.desktop` | 1 | `Name=BashCutPro` |
| `packaging/linux/flatpak-bundle.sh` | 1 | tarball name |
| `packaging/linux/flatpak/ai.storyteller.filmcraft.yml` | 2 | icons from `bashcut/brand/`; speech-to-text (`--features …/whisper`, #174: flatpak-builder reads this file directly, so `bashcut/packaging/generate.py` cannot add it; generate.py checks it is still there) |
| `packaging/freebsd/package.sh` | 1 | icons from `$BRAND_ICON_DIR` |
| `omastore.toml` | 5 | icon path, `bashcutpro-…` tarball names |
| `.github/workflows/release.yml` | 1 | `app-title: BashCutPro` (`app-name` stays `filmcraft`: it is the cargo package) |
| `NOTICE` | 4 added | our copyright line above upstream's |
| `LICENSE-MIT` | 1 added | our copyright line below upstream's |
| `ATTRIBUTION.md` | section added at the end | "BashCutPro assets" rows |

New files (ours, no conflicts): `crates/ui-egui/src/bashcut_brand.rs`, `bashcut/brand/**`,
`bashcut/rebrand.md`, `.github/README.md` (GitHub shows it instead of the root `README.md`, which
stays upstream's).

## Branded strings replaced at lookup time

`bashcut_brand::OVERRIDES` maps these upstream English strings to ours (Help menu, Home screen links,
header tooltip, About, macOS app menu): "Join the ArtCraft Discord…", "ArtCraft Website",
"FilmCraft on getartcraft.com", "FilmCraft on GitHub", "Join us on Discord", "Join the ArtCraft
Discord (discord.gg/artcraft)", "Join the ArtCraft Discord", the About tagline ("… Part of the
ArtCraft family."), "About FilmCraft", "Hide FilmCraft", "Quit FilmCraft". Its test fails when one of
them disappears from upstream's catalogs, so a renamed string can't silently bring ArtCraft back.

## Not done (follow-ups)

- **DMG window** (`packaging/macos/dmg/`): the background still shows FilmCraft's owl, and the prebuilt
  `.DS_Store` positions `FilmCraft.app` on a volume named `FilmCraft`; with our names Finder falls back
  to default icon positions and may drop the background. Needs a BashCutPro background and a
  regenerated layout under `bashcut/brand/dmg/`, with `package.sh` pointing there.
- **Web build**: `apps/filmcraft-web/web/favicon.png` and the loading page still use FilmCraft's owl
  and name.
- **The word "FilmCraft"** in menus, dialogs and messages (out of scope in #25; the
  override table in `bashcut_brand.rs` is the place to do it if wanted). The native window title says BashCutPro since #90.
- `metainfo.xml.in` still points its screenshot at upstream's `docs/images/filmcraft-hero.png`.
- Localised versions of the replaced strings (they show in English in every language).
