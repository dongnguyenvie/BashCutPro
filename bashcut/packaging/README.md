# Pro pieces in the installers (#64)

Users get BashCutPro's own tools in every package, not only from source, without editing upstream's packaging
scripts for them.

## What every package carries

| Piece | macOS (.app / CLI zip) | Windows (MSI / portable zip) | Linux (tar, deb, rpm, AppImage) / FreeBSD |
|---|---|---|---|
| `bashcutpro` and its alias `bcpro` (#154, in place of `filmcraft-cli`, SB2), `bashcut-doctor`, `bashcut-batch`, `bashcut-mcp` | `Contents/MacOS/` (only `bcpro` there: `bashcutpro` would replace `BashCutPro` on a Mac volume, #187); CLI zip | install folder; portable zip | `/usr/bin` (FreeBSD: `bin/`) |
| first-run setup `bashcut-setup.sh` / `.ps1` | `Contents/Resources/bashcut/`; CLI zip | install folder; portable zip | `/usr/share/bashcutpro/` (FreeBSD: `share/bashcutpro/`) |
| speech-to-text (`--features whisper` for `filmcraft` and `bashcutpro-cli`, #146; candle on the CPU) | app and CLI | app and CLI | app and CLI |

The batch presets and the kit's caption skills start by transcribing. The Whisper models are not packaged: they
download on first use (`bashcutpro exec transcript.downloadModel`, or Text panel > Transcribe); until then
`bashcut-doctor` warns and a batch step that transcribes fails with that command as its next step.
The Flatpak bundle repackages the Linux tarball. Upstream's Flatpak source manifest
(`packaging/linux/flatpak/ai.storyteller.filmcraft.yml`) keeps its carried speech-to-text edit (#174), which
`generate.py` checks; our own from-source manifest is the generated one (below).

## Linux app id (SB4)

The copy's Linux files use `com.birchapps.bashcutpro` instead of upstream's `ai.storyteller.filmcraft`, so the
packages never replace an installed FilmCraft's desktop entry, icons, MIME file or Flatpak (decisions.md, SB4).
`generate.py` renames the files named after the id, replaces the id in every copied Linux file, copies the
hicolor icons from `$BRAND_ICON_DIR` (default `bashcut/brand/app-icon`) under the new name to
`linux/icons/hicolor/`, and points `package.sh` / `flatpak-bundle.sh` at the copy (`HERE`). The app sets the same
id as its Wayland app id / X11 class (`bashcut_brand::APP_ID`, a hook in `apps/filmcraft/src/main.rs`, SB5), so
`StartupWMClass=com.birchapps.bashcutpro` matches the window to our desktop entry. One exception: the `.fcproj` type
`application/x-filmcraft` (shared with FilmCraft: the last installed app opens `.fcproj`). The copy's `install.sh`,
`package.sh`, `flatpak-bundle.sh` messages and the Flatpak manifests' comments name BashCutPro (SUBS, SB5).

| Generated (`target/bashcut-packaging/linux/`) | Installed as (deb/rpm/AppImage/tar; Flatpak under `/app`) |
|---|---|
| `com.birchapps.bashcutpro.desktop` | `/usr/share/applications/com.birchapps.bashcutpro.desktop` |
| `com.birchapps.bashcutpro.metainfo.xml.in` | `/usr/share/metainfo/com.birchapps.bashcutpro.metainfo.xml` |
| `com.birchapps.bashcutpro.mime.xml` | `/usr/share/mime/packages/com.birchapps.bashcutpro.xml` |
| `icons/hicolor/<size>/apps/com.birchapps.bashcutpro.png`, `scalable/…svg` | `/usr/share/icons/hicolor/…` (AppImage: also `com.birchapps.bashcutpro.png` at its root) |
| `flatpak/com.birchapps.bashcutpro.bundle.yml` | used by `flatpak-bundle.sh`: `bashcutpro-<v>-linux-<arch>.flatpak`, `/app/bin/{filmcraft,bashcutpro,bcpro,bashcut-*}` |
| `flatpak/com.birchapps.bashcutpro.yml` | from source (Flathub): builds `-p filmcraft -p bashcutpro-cli` with whisper and the Pro tools |

From-source Flatpak: `python3 bashcut/packaging/generate.py --out target/bashcut-packaging`, vendor
`packaging/linux/flatpak/cargo-sources.json` as upstream's header says, then
`flatpak-builder --user --install --force-clean build-flatpak target/bashcut-packaging/linux/flatpak/com.birchapps.bashcutpro.yml`
and `flatpak run --command=bashcutpro com.birchapps.bashcutpro --version`. Its sources are the checkout
(relative path written by the generator) and the generated Linux folder (`dest: bashcut-packaging/linux`).

Profiles, templates and looks are built into the binaries (`bashcut.profiles.*`, `bashcut.template.*`,
`bashcut.looks.*`); the setup writes their data-dir files, so the installers need no data tree of their own.

## How (add-only)

1. Every bash packaging script sources `packaging/env.sh`, which sources our `bashcut/brand/packaging.env` (existing
   hook). That file now runs `bashcut/packaging/generate.py`, which writes a copy of `packaging/` to
   `$CARGO_TARGET_DIR/bashcut-packaging/` with our additions, and re-execs the same script from the copy
   (`BASHCUT_BRANDED=1`, `BASHCUT_ROOT` pins the checkout).
2. Windows: one inserted hook line in `packaging/windows/package.ps1` (row in `UPSTREAM.md`) runs
   `bashcut/packaging/package.ps1`, which does the same for PowerShell.
3. `generate.py` applies exact-match substitutions (`SUBS`), then renames `filmcraft-cli` to `bashcutpro` in the
   copied scripts and manifests (`RENAMES`: `-p bashcutpro-cli`, its `whisper` feature, the staged/installed file): build our packages with the app
   (`-p bashcut-doctor -p bashcut-batch-cli -p bashcut-mcp`), stage, strip and sign the binaries, add them to
   `nfpm.yaml`, the WiX source and the Linux tarball installer, copy `share/`. A substitution that no longer matches
   upstream's text fails the packaging run with the file and line to update; it never ships silently without our tools.
4. `BASHCUT_PACKAGING_PLAIN=1` packages upstream's files unchanged (for comparing).

Branding itself is still a carried edit of `packaging/**` (see `UPSTREAM.md`); moving it into `generate.py` as more
substitutions over upstream's original text is the #39 follow-up and fits the same mechanism.

## First-run setup (optional, every step skippable)

`bashcut-setup.sh [--yes] [--skip-looks] [--skip-profiles] [--skip-kit] [--skip-doctor]` (Windows: `bashcut-setup.ps1`
with `-Yes`, `-Skip…`):

1. Creates `<data dir>/bashcut/{profiles,templates,looks}`.
2. Writes the look pack as `.cube` LUTs + `looks.json` (`bashcut.looks.export`).
3. Compiles the output profiles into the `BashCut <name>` export presets (`bashcut.profiles.compile`).
4. Installs the agent kit as a Claude Code plugin when `claude` is found, and for Codex (clone + `install-codex.sh`) when `codex` is found.
5. Plugins come from the registry in the app; models download on first use, verified against `models.json`.
6. Runs `bashcut-doctor`.

The in-app first-run screen (the same steps from the side panel) belongs to the UI owner; it can call these commands
and `bashcut-doctor --json`.

## Release checklist

- Package on each OS (`packaging/<os>/package.sh`, `packaging/windows/package.ps1`), then `bashcut/scripts/smoke.sh` /
  `smoke.ps1` on mac 01 and geekomT ([smoke.md](../smoke.md)); install the package, launch with the rail, run a
  2-item batch.
- Codec patents: for a paid build prefer the OS encoders (VideoToolbox, Media Foundation) and get legal advice before
  selling.

Tests: `python3 bashcut/packaging/test_generate.py` (every substitution matches upstream's current files once, the
copies parse, the WiX source is valid XML, no generated Linux file names or mentions `ai.storyteller.filmcraft`
(`StartupWMClass` included) or ships `filmcraft-cli`, the Flatpak manifests parse and find their sources; on Linux,
upstream's installer test runs on the copy).
