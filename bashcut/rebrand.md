# Rebrand: BashCutPro instead of FilmCraft / ArtCraft

Issue #25. A modified FilmCraft must remove the ArtCraft marks and not be called ArtCraft
(`docs/brand/LICENSE-brand.txt`, "Forks and modified versions"); saying "based on FilmCraft by the
ArtCraft team" in plain text is allowed. This page lists **every upstream file we changed for that**,
so an upstream merge knows where to look. When one of these conflicts, keep upstream's change and
re-apply ours.

## Rules

- Only what a user sees changes. Crates (`filmcraft-*`), binaries (`filmcraft`, `filmcraft-cli`),
  modules, command ids (`help.discord`…), automation ids, `FILMCRAFT_*` env vars, data folders, file
  formats (`.fcproj`, `.fcgt`, UTI `ai.storyteller.filmcraft.project`) and `filmcraft://` resources keep
  their names.
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
| Release file prefix | `filmcraft-<ver>-…` | `bashcutpro-<ver>-…` (`BRAND_SLUG`) |
| Windows MSI UpgradeCode | `31E6FB14-…` | `B6658EBC-E524-4423-AA52-C08E9B38AACB` (new; never change it again) |
| Manufacturer / vendor | Learning Machines LLC | Birch Apps (owner to confirm) |
| Linux package name | `filmcraft` | `bashcutpro` |
| Linux app id / desktop file | `ai.storyteller.filmcraft` | unchanged (see "Not done") |
| Community link (`help.discord`) | discord.gg/artcraft | github.com/dongnguyenvie/BashCutPro/discussions |
| Website (`help.website`) | getartcraft.com | bashcut.birchapps.com |
| App page (`help.appPage`) | getartcraft.com/apps/filmcraft | github.com/dongnguyenvie/BashCutPro/releases |
| Source, issues | github.com/storytold/filmcraft | github.com/dongnguyenvie/BashCutPro |

## Upstream files touched

| File | Lines | What |
|---|---|---|
| `crates/ui-egui/src/brand.rs` | 2 | `include_bytes!` → our wordmarks |
| `crates/ui-egui/src/lib.rs` | 1 | `pub mod bashcut_brand;` |
| `crates/ui-egui/src/i18n/mod.rs` | 4 | `Language::tr` asks `bashcut_brand::text` first (not in this crate's unit tests, so upstream's catalog tests stay as they are) |
| `crates/ui-egui/src/links.rs` | 5 + test | URL constants; the scheme test checks our URLs |
| `crates/ui-egui/src/header.rs` | 2 | community button label (`COMMUNITY_BUTTON`) and its automation label |
| `crates/ui-egui/src/panels/dialogs.rs` | 3 | About: heading `NAME`, website label from `links::WEBSITE`, a NOTICE link (`about.notice`) |
| `apps/filmcraft/src/main.rs` | 2 | runtime window / Dock icon → `bashcut/brand/app-icon/` |
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
| `packaging/linux/flatpak/ai.storyteller.filmcraft.yml` | 1 | icons from `bashcut/brand/` |
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
- **Linux app id** stays `ai.storyteller.filmcraft` (desktop file, metainfo id, MIME, hicolor icon
  names, Wayland `with_app_id` in `apps/filmcraft/src/main.rs`, Flatpak id). Changing it means renaming
  installed files at package time plus one code line.
- **Web build**: `apps/filmcraft-web/web/favicon.png` and the loading page still use FilmCraft's owl
  and name.
- **The word "FilmCraft"** in the window title, menus, dialogs and messages (out of scope in #25; the
  override table in `bashcut_brand.rs` is the place to do it if wanted).
- `metainfo.xml.in` still points its screenshot at upstream's `docs/images/filmcraft-hero.png`.
- Localised versions of the replaced strings (they show in English in every language).
