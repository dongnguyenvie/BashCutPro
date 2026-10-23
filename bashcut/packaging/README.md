# Pro pieces in the installers (#64)

Users get BashCutPro's own tools in every package, not only from source, without editing upstream's packaging
scripts for them.

## What every package carries

Names (NM2, owner decision 2026-10-23, upstream FilmCraft's convention): the source still builds the app as
`filmcraft` (`-p filmcraft`); the packages install it under our name. No installed name says filmcraft.

| | macOS | Windows | Linux / FreeBSD |
|---|---|---|---|
| app | `BashCutPro.app/Contents/MacOS/BashCutPro` | `bashcutpro.exe` (Start menu, `App Paths\bashcutpro.exe`) | `/usr/bin/bashcutpro` (FreeBSD `bin/bashcutpro`); desktop `Exec=bashcutpro`, AppImage `AppRun` -> `usr/bin/bashcutpro`, Flatpak `command: bashcutpro` |
| command line | `bashcutpro-cli` + `bcpro` (link) in `Contents/MacOS/` and the CLI zip | `bashcutpro-cli.exe` + `bcpro.exe` (copy) | `bashcutpro-cli` + `bcpro` (link) |

The engine lookup (app, setup scripts, MCP) tries `bashcutpro-cli`, then `bcpro`, then `filmcraft-cli`, after the env
overrides (`BASHCUTPRO_CLI`, `BASHCUT_FILMCRAFT_CLI`, `FILMCRAFT_CLI`); `apps/bashcutpro-cli` builds the bins
`bashcutpro-cli` and `bcpro` (NM1).

| Piece | macOS (.app / CLI zip) | Windows (MSI / portable zip) | Linux (tar, deb, rpm, AppImage) / FreeBSD |
|---|---|---|---|
| `bashcutpro-cli` and its alias `bcpro` (#154, in place of `filmcraft-cli`, SB2, NM2), `bashcut-doctor`, `bashcut-batch`, `bashcut-mcp` | `Contents/MacOS/` (no case clash with `BashCutPro`, #187); CLI zip | install folder; portable zip | `/usr/bin` (FreeBSD: `bin/`) |
| first-run setup `bashcut-setup.sh` / `.ps1` | `Contents/Resources/bashcut/`; CLI zip | install folder; portable zip | `/usr/share/bashcutpro/` (FreeBSD: `share/bashcutpro/`) |
| speech-to-text (`--features whisper` for `filmcraft` and `bashcutpro-cli`, #146; candle on the CPU) | app and CLI | app and CLI | app and CLI |

`bcpro` is a relative symlink to `bashcutpro-cli` wherever the package can hold one (deb, rpm, tarball, AppImage,
Flatpak, FreeBSD, the macOS app and CLI zip; PK3, NM2), not a second ~36 MB copy. Windows keeps `bcpro.exe` as a copy
(MSI and zip carry no symlinks; a `.cmd` shim would change Ctrl+C and exit-code behaviour). Linux and FreeBSD docs
install to `share/doc/bashcutpro` (Debian policy), and `bashcut-doctor --version` /
`bashcut-mcp --version` print `<name> <version>` and exit 0.

The batch presets and the kit's caption skills start by transcribing. The Whisper models are not packaged: they
download on first use (`bashcutpro-cli exec transcript.downloadModel`, or Text panel > Transcribe); until then
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
`StartupWMClass=com.birchapps.bashcutpro` matches the window to our desktop entry. FreeBSD ships the same generated
files (`LINUX="$BASHCUT_PACKAGING/linux"` in its copy, NM2). One exception: the `.fcproj` type
`application/x-filmcraft` (shared with FilmCraft: the last installed app opens `.fcproj`). The copy's `install.sh`,
`package.sh`, `flatpak-bundle.sh` messages and the Flatpak manifests' comments name BashCutPro (SUBS, SB5).

| Generated (`target/bashcut-packaging/linux/`) | Installed as (deb/rpm/AppImage/tar; Flatpak under `/app`) |
|---|---|
| `com.birchapps.bashcutpro.desktop` | `/usr/share/applications/com.birchapps.bashcutpro.desktop` |
| `com.birchapps.bashcutpro.metainfo.xml.in` | `/usr/share/metainfo/com.birchapps.bashcutpro.metainfo.xml` |
| `com.birchapps.bashcutpro.mime.xml` | `/usr/share/mime/packages/com.birchapps.bashcutpro.xml` |
| `icons/hicolor/<size>/apps/com.birchapps.bashcutpro.png`, `scalable/…svg` | `/usr/share/icons/hicolor/…` (AppImage: also `com.birchapps.bashcutpro.png` at its root) |
| `flatpak/com.birchapps.bashcutpro.bundle.yml` | used by `flatpak-bundle.sh`: `bashcutpro-<v>-linux-<arch>.flatpak`, `/app/bin/{bashcutpro,bashcutpro-cli,bcpro,bashcut-*}` |
| `flatpak/com.birchapps.bashcutpro.yml` | from source (Flathub): builds `-p filmcraft -p bashcutpro-cli` with whisper and the Pro tools |

From-source Flatpak: `python3 bashcut/packaging/generate.py --out target/bashcut-packaging`, vendor
`packaging/linux/flatpak/cargo-sources.json` as upstream's header says, then
`flatpak-builder --user --install --force-clean build-flatpak target/bashcut-packaging/linux/flatpak/com.birchapps.bashcutpro.yml`
and `flatpak run --command=bashcutpro-cli com.birchapps.bashcutpro --version`. Its sources are the checkout
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
3. `generate.py` applies exact-match substitutions (`SUBS`), then renames `filmcraft-cli` to `bashcutpro-cli` in the
   copied scripts and manifests (`RENAMES`: `-p bashcutpro-cli`, its `whisper` feature, the built and installed file,
   the WiX ids), then installs the app as `bashcutpro` (`APP_SUBS`, exact-match like `SUBS`: cargo's
   `$BIN/filmcraft` stays, every installed path changes): build our packages with the app
   (`-p bashcut-doctor -p bashcut-batch-cli -p bashcut-mcp`), stage, strip and sign the binaries, add them to
   `nfpm.yaml`, the WiX source and the Linux tarball installer, copy `share/`. A substitution that no longer matches
   upstream's text fails the packaging run with the file and line to update; it never ships silently without our tools.
4. `BASHCUT_PACKAGING_PLAIN=1` packages upstream's files unchanged (for comparing).

Branding itself is still a carried edit of `packaging/**` (see `UPSTREAM.md`); moving it into `generate.py` as more
substitutions over upstream's original text is the #39 follow-up and fits the same mechanism.

## First-run setup (optional, every step skippable)

`bashcut-setup.sh [--yes] [--skip-looks] [--skip-profiles] [--skip-kit] [--skip-doctor]` (Windows: `bashcut-setup.ps1`
with `-Yes`, `-Skip…`):

1. Creates `<data dir>/bashcut/{profiles,templates,looks}`. `<data dir>` is BashCutPro's own folder, by the app's rules
   (`bashcut_dirs::data_dir`, SB1): `BASHCUTPRO_DATA_DIR`, `FILMCRAFT_DATA_DIR`, then `BashCutPro` (macOS, Windows;
   `.\data` for a portable Windows install) or `bashcutpro` (XDG). The engine is `bashcutpro-cli`, else `bcpro`,
   else `filmcraft-cli`.
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
- Windows (PK1, first real run 2026-10-23): needs WiX v5 (`dotnet tool install --global wix --version 5.0.2`; a
  user-level .NET from `dotnet-install.ps1` works with `DOTNET_ROOT` set). Windows PowerShell 5.1 and `pwsh` both
  work: the copy writes the portable zip's entries itself with `/` (PK4; 5.1's `Compress-Archive` and
  `ZipFile.CreateFromDirectory` write backslashes, which `unzip` warns about). Without
  signing secrets every binary and the MSI stay unsigned (a warning, not an error). Output in `dist/release/`:
  `bashcutpro-<v>-windows-x64.msi` (product BashCutPro, Birch Apps, its own UpgradeCode, `Program Files\BashCutPro`,
  Start menu shortcut BashCutPro) and `bashcutpro-<v>-windows-x64-portable.zip`. Both carry the app as
  `bashcutpro.exe` (built as `filmcraft.exe`; VERSIONINFO and icon BashCutPro) next to `bashcutpro-cli.exe`, `bcpro.exe`, `bashcut-doctor.exe`,
  `bashcut-batch.exe`, `bashcut-mcp.exe`, `bashcut-setup.ps1`; no `filmcraft-cli.exe`. Every command-line exe has
  the BashCutPro icon and VERSIONINFO too (ProductName BashCutPro, its own FileDescription, the package version;
  `crates/bashcut-winres`, PK4; `bcpro.exe` shares `bashcutpro-cli.exe`'s resource). PK4 run (0.10.0, unsigned, Windows
  PowerShell 5.1): MSI 51.9 MB, portable zip 66.4 MB. Since NM2 the app exe, the `App Paths\bashcutpro.exe` key and
  the WiX ids are BashCutPro's; still upstream's: the log file `Logs/filmcraft.log` (`apps/filmcraft/src/logging.rs`).
  PV1 run after NM1 + NM2 (main `cfe39233`, unsigned, Windows PowerShell 5.1): MSI 51.9 MB, portable zip 66.4 MB; the
  MSI admin image (`msiexec /a`) and the zip hold `bashcutpro.exe`, `bashcutpro-cli.exe`, `bcpro.exe` and no
  `filmcraft*.exe`; `wix msi decompile` shows `App Paths\bashcutpro.exe` and the Start menu shortcut BashCutPro; zip
  entries use `/`. The portable app (run from a temp folder, data in `.\data`, nothing in `%APPDATA%`) took one Agent
  section turn (Claude Code added a marker) and bridged `bcpro inspect sequence` / `bcpro exec clip.rename` with no
  `--project`, both shown in the open window. Still upstream's: the zip's `README.md` (FilmCraft's) and the
  `FilmCraft Previews` folder next to a saved project.
- Linux (PV2 run 2026-10-23, after NM2; aarch64 in podman, Ubuntu 22.04 for `package.sh`, Debian trixie for
  `flatpak-bundle.sh`): needs `nfpm` (deb/rpm), `zsync` (AppImage `.zsync`), `desktop-file-utils` and `appstream`
  (validation); `appimagetool` downloads itself. Output 0.10.0: deb 45.4 MB, rpm 46.2 MB, tar.gz 43.8 MB, AppImage
  40.9 MB (+ `.zsync`), flatpak 28.2 MB. Every format carries `bin/bashcutpro` (the app), `bashcutpro-cli`,
  `bcpro -> bashcutpro-cli`, `bashcut-doctor`, `bashcut-batch`, `bashcut-mcp`, no `filmcraft*` file; desktop
  `Exec=bashcutpro %F`, `TryExec=bashcutpro`, AppImage `AppRun -> usr/bin/bashcutpro`, Flatpak `command=bashcutpro`;
  desktop-file-validate clean, `appstreamcli validate` passes (one info: 22.04's appstream 0.15 does not know
  `<developer>`). The flatpak bundle needs flatpak-builder 1.4+ (Ubuntu 22.04's 1.2 calls `appstream-compose`, which
  the 25.08 SDK no longer has). Still upstream's: the app's own `--version` / usage text says `filmcraft`
  (`apps/filmcraft/src/main.rs`).
- macOS (PV3 run 2026-10-23, 0.10.0 aarch64, ad-hoc): `packaging/macos/package.sh --arch aarch64` gives
  `BashCutPro.app` (`Contents/MacOS/`: `BashCutPro`, `bashcutpro-cli`, `bcpro -> bashcutpro-cli`, `bashcut-doctor`,
  `bashcut-batch`, `bashcut-mcp`), a 40 MB DMG and a 17 MB CLI zip; `codesign --verify --deep --strict` passes with the
  link. The CLI zip is made with `ditto --norsrc --noextattr` (PV3) so it carries no AppleDouble `._*` entries
  (`com.apple.provenance` and other extended attributes), which `unzip` writes out as stray files. From inside the app
  `bashcut-mcp --print` and `bashcut-doctor` find `Contents/MacOS/bashcutpro-cli`. Launched from the Finder the Agent
  section finds `claude` in `~/.local/bin` and the like ([guide](../guide/en/agent.md)); macOS then asks once for the
  project's folder (Desktop, Documents) and a new ad-hoc build asks again.
- Codec patents: for a paid build prefer the OS encoders (VideoToolbox, Media Foundation) and get legal advice before
  selling.

Tests: `python3 bashcut/packaging/test_generate.py` (every substitution matches upstream's current files once, the
copies parse, the WiX source is valid XML, no generated Linux file names or mentions `ai.storyteller.filmcraft`
(`StartupWMClass` included) or ships `filmcraft-cli`, the Flatpak manifests parse and find their sources, the Windows
copy names BashCutPro and zips with `/`, no installed binary name says filmcraft (app `bashcutpro`, CLI
`bashcutpro-cli` + `bcpro`, `Exec=bashcutpro`, `App Paths\bashcutpro.exe`; NM2) (and parses, when PowerShell is there); on Linux,
upstream's installer test runs on the copy).
