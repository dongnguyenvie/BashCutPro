#!/usr/bin/env python3
"""BashCutPro packaging generator (#64): a copy of upstream's `packaging/` that also ships the Pro pieces.

The packaging scripts re-exec the copy this writes (bash: `bashcut/brand/packaging.env`, sourced by
`packaging/env.sh`; Windows: one hook line in `packaging/windows/package.ps1` runs `bashcut/packaging/package.ps1`),
so upstream's packaging files are never edited. Every change is an exact-match substitution that must match the
expected number of times: when upstream rewrites a line we depend on, this fails loudly instead of shipping without
our tools. Same input, byte-identical output (test_generate.py).

The copy adds: speech-to-text (`--features whisper` for `filmcraft` and the engine CLI, #146); the Pro command line
`bashcutpro-cli` + alias `bcpro` in `filmcraft-cli`'s place (SB2, NM2); the app installed as `bashcutpro`
(`bashcutpro.exe`; macOS keeps `BashCutPro.app/Contents/MacOS/BashCutPro`, NM2); `bashcut-doctor`, `bashcut-batch`,
`bashcut-mcp`; the first-run setup (`bashcut/packaging/share/`); on Linux and FreeBSD our app id
`com.birchapps.bashcutpro` (SB4). Only scripts and the manifests that list files are copied and changed. Details:
bashcut/packaging/README.md.

    python3 bashcut/packaging/generate.py --out target/bashcut-packaging [--root .]
"""

import argparse
import os
import shutil
import sys

# Our binaries next to the app. bashcutpro-cli (#154, SB2, NM2: upstream's engine CLI generated under our name by
# apps/bashcutpro-cli) is not listed: it takes filmcraft-cli's place in upstream's lines (RENAMES); bcpro is its alias.
# The app itself is built as `filmcraft` (upstream's source) and installed as `bashcutpro` (APP_SUBS, NM2).
PRO_BINS = ["bcpro", "bashcut-doctor", "bashcut-batch", "bashcut-mcp"]
# PK3: where a package can hold a symlink (Linux, FreeBSD, the macOS app and CLI zip, Flatpak), bcpro is a relative
# link to the engine (written `filmcraft-cli`, renamed to bashcutpro-cli below) instead of a second ~36 MB copy.
# Windows (MSI, portable zip) keeps a real bcpro.exe.
ALIAS = "bcpro"
TOOLS = [b for b in PRO_BINS if b != ALIAS]
TOOLS_SH = " ".join(TOOLS)
PRO_PACKAGES = "-p bashcut-doctor -p bashcut-batch-cli -p bashcut-mcp"
BINS_SH = " ".join(PRO_BINS)
# speech-to-text in the app and the engine CLI (#146): the batch presets and caption skills start by
# transcribing; models still download on first use (`transcript.downloadModel`)
FEATURES = "--features filmcraft/whisper,filmcraft-cli/whisper"
BUILD = "-p filmcraft -p filmcraft-cli %s %s" % (FEATURES, PRO_PACKAGES)


def wix_id(binary):
    """A WiX id for a binary: `bashcut-doctor` -> `BashcutDoctor`, `bcpro` -> `Bcpro`."""
    return "".join(part.capitalize() for part in binary.split("-"))


# (file under packaging/, old text, new text, expected matches)
SUBS = [
    # ---- shared: the copy runs from target/, so ROOT is pinned to the checkout
    (
        "env.sh",
        'ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"',
        'ROOT="${BASHCUT_ROOT:?set by bashcut/brand/packaging.env}"',
        1,
    ),
    # BR1: every archive that ships a README (Linux tarball and doc folder, FreeBSD, the macOS CLI zip, web) gets
    # BashCutPro's user README instead of upstream's (FilmCraft's); the MSI and the DMG ship none
    (
        "env.sh",
        "  for f in README.md LICENSE LICENSE-MIT LICENSE-APACHE COPYRIGHT NOTICE; do\n",
        '  cp "$ROOT/bashcut/packaging/share/README.md" "$dest/README.md"\n' "  for f in LICENSE LICENSE-MIT LICENSE-APACHE COPYRIGHT NOTICE; do\n",
        1,
    ),
    # ---- macOS
    (
        "macos/package.sh",
        "-p filmcraft -p filmcraft-cli ",
        "%s " % BUILD,
        1,
    ),
    (
        "macos/package.sh",
        "for bin in filmcraft filmcraft-cli; do\n",
        "for bin in filmcraft filmcraft-cli %s; do\n" % BINS_SH,
        1,
    ),
    (
        "macos/package.sh",
        'copy_font_licences "$APP/Contents/Resources"\n',
        'copy_font_licences "$APP/Contents/Resources"\n'
        "# BashCutPro: Pro tools beside the app executable, the engine CLI among them (#187, SB2); bashcutpro-cli and\n"
        "# bcpro never clash with the executable BashCutPro on a case-insensitive volume (NM2), bcpro is a link to it\n"
        'APP_BINS="filmcraft-cli %s"\n'
        'for b in $APP_BINS; do cp "$WORK/bin/$b" "$APP/Contents/MacOS/$b"; done\n'
        'ln -s filmcraft-cli "$APP/Contents/MacOS/%s"\n'
        "# first-run setup in Resources/bashcut\n"
        'mkdir -p "$APP/Contents/Resources/bashcut" && cp "$ROOT/bashcut/packaging/share/"bashcut-setup.* "$APP/Contents/Resources/bashcut/"\n' % (TOOLS_SH, ALIAS),
        1,
    ),
    (
        "macos/package.sh",
        'sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/$BRAND_NAME"\n',
        'for b in $APP_BINS; do sign --options runtime "$APP/Contents/MacOS/$b"; done\n'
        'sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/$BRAND_NAME"\n',
        1,
    ),
    (
        "macos/package.sh",
        'cp "$WORK/bin/filmcraft-cli" "$CLI_DIR/"\n',
        'cp "$WORK/bin/filmcraft-cli" "$CLI_DIR/"\n'
        'for b in %s; do cp "$WORK/bin/$b" "$CLI_DIR/"; done\n'
        'ln -s filmcraft-cli "$CLI_DIR/%s"\n'
        'cp "$ROOT/bashcut/packaging/share/"bashcut-setup.* "$CLI_DIR/"\n' % (TOOLS_SH, ALIAS),
        1,
    ),
    (
        "macos/package.sh",
        'sign --options runtime "$CLI_DIR/filmcraft-cli"\n',
        'sign --options runtime "$CLI_DIR/filmcraft-cli"\n'
        'for b in %s; do sign --options runtime "$CLI_DIR/$b"; done\n' % TOOLS_SH,
        1,
    ),
    # PV3: no AppleDouble `._*` entry per file (extended attributes such as com.apple.provenance);
    # `unzip` writes them out as stray files next to the tools
    (
        "macos/package.sh",
        'ditto -c -k --keepParent "$CLI_DIR" "$CLI_ZIP"\n',
        'ditto -c -k --norsrc --noextattr --keepParent "$CLI_DIR" "$CLI_ZIP"\n',
        1,
    ),
    ("macos/package.sh", 'echo "==> FilmCraft $VERSION for macOS', 'echo "==> BashCutPro $VERSION for macOS', 1),
    # SG1: notarize with a notarytool keychain profile (`MACOS_NOTARY_PROFILE`, e.g. an App Store Connect API key
    # stored by `xcrun notarytool store-credentials`) as well as upstream's APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID
    (
        "macos/package.sh",
        'elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then\n'
        "  NOTARIZE=1\n",
        'elif [ -n "${MACOS_NOTARY_PROFILE:-}" ]; then\n'
        "  NOTARIZE=1\n"
        '  NOTARY_AUTH=(--keychain-profile "$MACOS_NOTARY_PROFILE")\n'
        'elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then\n'
        "  NOTARIZE=1\n"
        '  NOTARY_AUTH=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")\n',
        1,
    ),
    (
        "macos/package.sh",
        'xcrun notarytool submit "$file" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \\\n'
        '    --team-id "$APPLE_TEAM_ID" --wait',
        'xcrun notarytool submit "$file" "${NOTARY_AUTH[@]}" --wait',
        1,
    ),
    (
        "macos/package.sh",
        'xcrun notarytool log "$id" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" || true',
        'xcrun notarytool log "$id" "${NOTARY_AUTH[@]}" || true',
        1,
    ),
    (
        "macos/package.sh",
        'warn "macOS: APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID incomplete; signed but not notarized"',
        'warn "macOS: no MACOS_NOTARY_PROFILE and APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID incomplete; signed but not notarized"',
        1,
    ),
    # ---- Linux
    ("linux/package.sh", "-p filmcraft -p filmcraft-cli", BUILD, 2),
    (
        "linux/package.sh",
        'install -Dm755 "$BIN/filmcraft-cli" "$STAGE/usr/bin/filmcraft-cli"\n',
        'install -Dm755 "$BIN/filmcraft-cli" "$STAGE/usr/bin/filmcraft-cli"\n'
        "# BashCutPro: Pro tools and the first-run setup\n"
        'for b in %s; do install -Dm755 "$BIN/$b" "$STAGE/usr/bin/$b"; "${CROSS_COMPILE:-}strip" "$STAGE/usr/bin/$b" 2>/dev/null || true; done\n'
        'ln -s filmcraft-cli "$STAGE/usr/bin/%s"\n'
        'mkdir -p "$STAGE/usr/share/bashcutpro" && cp "$ROOT/bashcut/packaging/share/"bashcut-setup.* "$STAGE/usr/share/bashcutpro/"\n' % (TOOLS_SH, ALIAS),
        1,
    ),
    # SB4: the copy's install.sh, nfpm.yaml, desktop file, metainfo, MIME file and icons (renamed to APP_ID)
    ("linux/package.sh", 'HERE="$ROOT/packaging/linux"\n', 'HERE="$BASHCUT_PACKAGING/linux"\n', 1),
    ("linux/package.sh", 'cp -R "$BRAND_ICON_DIR/hicolor" ', 'cp -R "$HERE/icons/hicolor" ', 1),
    ("linux/package.sh", '"$BRAND_ICON_DIR/hicolor/256x256/apps/$APP_ID.png"', '"$HERE/icons/hicolor/256x256/apps/$APP_ID.png"', 1),
    # PK3: the docs folder is named after the package (Debian policy 12.3, lintian)
    ("linux/package.sh", '"$STAGE/usr/share/doc/filmcraft"', '"$STAGE/usr/share/doc/bashcutpro"', 2),
    ("linux/flatpak/ai.storyteller.filmcraft.bundle.yml", "cp -R share/doc/filmcraft /app/share/doc/", "cp -R share/doc/bashcutpro /app/share/doc/", 1),
    # PK3: a real first description paragraph (software centres show it; appstream wants a full sentence)
    (
        "linux/ai.storyteller.filmcraft.metainfo.xml.in",
        "    <p>\n      BashCutPro is a video editor based on FilmCraft. Edit video, color and sound\n    </p>\n",
        "    <p>\n      BashCutPro is a native, open-source video editor based on FilmCraft: cut on a timeline with source\n"
        "      and program monitors, grade color with scopes, add effects, titles and captions, mix sound with\n"
        "      loudness meters, and export.\n    </p>\n"
        "    <p>\n      The bashcutpro command line, the batch runner and the MCP server drive the same editor from\n"
        "      scripts and AI agents, and speech-to-text writes captions on your own computer.\n    </p>\n",
        1,
    ),
    ("linux/flatpak-bundle.sh", 'HERE="$ROOT/packaging/linux"\n', 'HERE="$BASHCUT_PACKAGING/linux"\n', 1),
    (
        "linux/flatpak/ai.storyteller.filmcraft.bundle.yml",
        "      - install -Dm755 bin/filmcraft-cli /app/bin/filmcraft-cli\n",
        "      - install -Dm755 bin/filmcraft-cli /app/bin/filmcraft-cli\n"
        "      # BashCutPro: Pro tools and the first-run setup\n"
        + "".join("      - install -Dm755 bin/%s /app/bin/%s\n" % (b, b) for b in TOOLS)
        + "      - ln -s filmcraft-cli /app/bin/%s\n" % ALIAS
        + "      - mkdir -p /app/share/bashcutpro && cp -R share/bashcutpro/. /app/share/bashcutpro/\n",
        1,
    ),
    # SB4: the from-source manifest in the copy builds and installs ours; flatpak-builder reads it from the copy
    # (`flatpak-builder … target/bashcut-packaging/linux/flatpak/com.birchapps.bashcutpro.yml`)
    ("linux/flatpak/ai.storyteller.filmcraft.yml", "-p filmcraft -p filmcraft-cli %s" % FEATURES, BUILD, 1),
    (
        "linux/flatpak/ai.storyteller.filmcraft.yml",
        "      - install -Dm755 target/release/filmcraft-cli /app/bin/filmcraft-cli\n",
        "      - install -Dm755 target/release/filmcraft-cli /app/bin/filmcraft-cli\n"
        "      # BashCutPro: Pro tools and the first-run setup\n"
        + "".join("      - install -Dm755 target/release/%s /app/bin/%s\n" % (b, b) for b in TOOLS)
        + "      - ln -s filmcraft-cli /app/bin/%s\n" % ALIAS
        + "      - mkdir -p /app/share/bashcutpro && cp bashcut/packaging/share/bashcut-setup.* /app/share/bashcutpro/\n",
        1,
    ),
    ("linux/flatpak/ai.storyteller.filmcraft.yml", " packaging/linux/ai.storyteller.filmcraft.", " bashcut-packaging/linux/ai.storyteller.filmcraft.", 3),
    ("linux/flatpak/ai.storyteller.filmcraft.yml", "cp -R bashcut/brand/app-icon/hicolor ", "cp -R bashcut-packaging/linux/icons/hicolor ", 1),
    (
        "linux/flatpak/ai.storyteller.filmcraft.yml",
        "build-flatpak packaging/linux/flatpak/ai.storyteller.filmcraft.yml\n",
        "build-flatpak target/bashcut-packaging/linux/flatpak/ai.storyteller.filmcraft.yml\n",
        1,
    ),
    (
        "linux/flatpak/ai.storyteller.filmcraft.yml",
        "From the repo root:\n#\n",
        "From the repo root:\n#\n"
        "#   python3 bashcut/packaging/generate.py --out target/bashcut-packaging   # BashCutPro: writes this file (SB4)\n",
        1,
    ),
    ("linux/flatpak/ai.storyteller.filmcraft.yml", "        path: ../../..\n", "        path: @ROOT@\n", 1),
    (
        "linux/flatpak/ai.storyteller.filmcraft.yml",
        "      - cargo-sources.json\n",
        "      # BashCutPro: the generated desktop file, metainfo, MIME file and icons\n"
        "      - type: dir\n        path: ..\n        dest: bashcut-packaging/linux\n"
        "      - @CARGO_SOURCES@\n",
        1,
    ),
    (
        "linux/nfpm.yaml",
        "  - src: ${STAGE}/usr/bin/filmcraft-cli\n    dst: /usr/bin/filmcraft-cli\n    file_info: { mode: 0755 }\n",
        "  - src: ${STAGE}/usr/bin/filmcraft-cli\n    dst: /usr/bin/filmcraft-cli\n    file_info: { mode: 0755 }\n"
        + "".join("  - src: ${STAGE}/usr/bin/%s\n    dst: /usr/bin/%s\n    file_info: { mode: 0755 }\n" % (b, b) for b in TOOLS)
        # the staged link itself: nfpm keeps it a symlink with the build's mtime (type: symlink wrote mtime 2042)
        + "  - src: ${STAGE}/usr/bin/%s\n    dst: /usr/bin/%s\n" % (ALIAS, ALIAS),
        1,
    ),
    (
        "linux/install.sh",
        'install -m755 -- "$source_dir/bin/filmcraft-cli" "$prefix/bin/filmcraft-cli"\n',
        'install -m755 -- "$source_dir/bin/filmcraft-cli" "$prefix/bin/filmcraft-cli"\n'
        'for b in %s; do if [ -f "$source_dir/bin/$b" ]; then install -m755 -- "$source_dir/bin/$b" "$prefix/bin/$b"; fi; done\n'
        'if [ -L "$source_dir/bin/%s" ]; then ln -sfn -- filmcraft-cli "$prefix/bin/%s"; fi\n' % (TOOLS_SH, ALIAS, ALIAS),
        1,
    ),
    # ---- FreeBSD
    ("freebsd/package.sh", "-p filmcraft -p filmcraft-cli", BUILD, 1),
    ("freebsd/package.sh", "  for b in filmcraft filmcraft-cli; do\n", "  for b in filmcraft filmcraft-cli %s; do\n" % BINS_SH, 1),
    (
        "freebsd/package.sh",
        'install -m 755 "$BIN/filmcraft" "$BIN/filmcraft-cli" "$STAGE/bin/"\n',
        'install -m 755 "$BIN/filmcraft" "$BIN/filmcraft-cli" "$STAGE/bin/"\n'
        "# BashCutPro: Pro tools and the first-run setup\n"
        'for b in %s; do install -m 755 "$BIN/$b" "$STAGE/bin/"; done\n'
        'ln -s filmcraft-cli "$STAGE/bin/%s"\n'
        'mkdir -p "$STAGE/share/bashcutpro" && cp "$ROOT/bashcut/packaging/share/"bashcut-setup.* "$STAGE/share/bashcutpro/"\n' % (TOOLS_SH, ALIAS),
        1,
    ),
    # ---- Windows
    (
        "windows/package.ps1",
        "$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\\..')).Path\n",
        "$Root = $env:BASHCUT_ROOT\n",
        1,
    ),
    (
        "windows/package.ps1",
        "-p filmcraft -p filmcraft-cli --target $Target",
        "%s --target $Target" % BUILD,
        1,
    ),
    (
        "windows/package.ps1",
        "Copy-Item (Join-Path $Bin 'filmcraft.exe'), (Join-Path $Bin 'filmcraft-cli.exe') $Stage\n",
        "Copy-Item (Join-Path $Bin 'filmcraft.exe'), (Join-Path $Bin 'filmcraft-cli.exe') $Stage\n"
        "# BashCutPro: Pro tools and the first-run setup\n"
        "foreach ($b in %s) { Copy-Item (Join-Path $Bin \"$b.exe\") $Stage }\n"
        "Copy-Item (Join-Path $Root 'bashcut\\packaging\\share\\bashcut-setup.ps1') $Stage\n"
        "& (Join-Path $PSScriptRoot 'sign.ps1') %s\n"
        % (", ".join("'%s'" % b for b in PRO_BINS), " ".join("(Join-Path $Stage '%s.exe')" % b for b in PRO_BINS)),
        1,
    ),
    (
        "windows/package.ps1",
        "Copy-Item (Join-Path $Stage '*.exe') $Portable\n",
        "Copy-Item (Join-Path $Stage '*.exe') $Portable\n" "Copy-Item (Join-Path $Stage 'bashcut-setup.ps1') $Portable\n",
        1,
    ),
    (
        "windows/filmcraft.wxs",
        '        <File Id="FilmcraftCliExe" Source="$(var.BinDir)\\filmcraft-cli.exe" KeyPath="yes" />\n      </Component>\n',
        '        <File Id="FilmcraftCliExe" Source="$(var.BinDir)\\filmcraft-cli.exe" KeyPath="yes" />\n      </Component>\n'
        + "".join(
            '\n      <Component Id="%s">\n        <File Id="%sExe" Source="$(var.BinDir)\\%s.exe" KeyPath="yes" />\n      </Component>\n'
            % (wix_id(b), wix_id(b), b)
            for b in PRO_BINS
        )
        + '\n      <Component Id="BashcutSetup">\n        <File Id="BashcutSetupPs1" Source="$(var.BinDir)\\bashcut-setup.ps1" KeyPath="yes" />\n      </Component>\n',
        1,
    ),
    # PK1: what the Windows copy prints and writes into portable.txt names BashCutPro and its own %APPDATA% folder
    # (bashcut_dirs::data_dir); the app exe is bashcutpro.exe (NM2)
    ("windows/package.ps1", 'Write-Output "FilmCraft $Version for Windows', 'Write-Output "BashCutPro $Version for Windows', 1),
    (
        "windows/package.ps1",
        "  'FilmCraft portable: preferences, presets, logs and crash recovery are kept in the data folder',\n"
        "  'next to filmcraft.exe. Delete this file to use %APPDATA%\\FilmCraft instead.'\n",
        "  'BashCutPro portable: preferences, presets, logs and crash recovery are kept in the data folder',\n"
        "  'next to bashcutpro.exe. Delete this file to use %APPDATA%\\BashCutPro instead.'\n",
        1,
    ),
    # BR1: the portable zip's README is BashCutPro's (bashcut/packaging/share/README.md), not upstream's
    (
        "windows/package.ps1",
        "foreach ($f in 'README.md', 'LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE') {\n",
        "Copy-Item (Join-Path $Root 'bashcut\\packaging\\share\\README.md') $Portable\n"
        "foreach ($f in 'LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE') {\n",
        1,
    ),
    # BR1: the PE check reads cargo's filmcraft.exe; its line names the exe as installed
    (
        "windows/package.ps1",
        '  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"\n',
        "  Write-Output \"ok $($check[0] -replace '^filmcraft\\.exe$', 'bashcutpro.exe'): $Arch, PE subsystem $($h.Subsystem)\"\n",
        1,
    ),
    # PK4: Windows PowerShell 5.1's Compress-Archive (and ZipFile.CreateFromDirectory on .NET Framework) writes the
    # zip entry names with backslashes; entries named with / unpack the same everywhere (pwsh 7, unzip, Explorer)
    (
        "windows/package.ps1",
        "Compress-Archive -Path $Portable -DestinationPath $Zip\n",
        "# BashCutPro: zip entry names with / on Windows PowerShell 5.1 too (Compress-Archive writes \\)\n"
        "Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem\n"
        "$ZipBase = (Get-Item -LiteralPath $Portable).Parent.FullName\n"
        "$ZipFull = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Zip)\n"
        "$Archive = [IO.Compression.ZipFile]::Open($ZipFull, [IO.Compression.ZipArchiveMode]::Create)\n"
        "try {\n"
        "  foreach ($f in Get-ChildItem -Recurse -File $Portable) {\n"
        "    $Entry = $f.FullName.Substring($ZipBase.Length).TrimStart('\\').Replace('\\', '/')\n"
        "    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($Archive, $f.FullName, $Entry, [IO.Compression.CompressionLevel]::Optimal)\n"
        "  }\n"
        "} finally { $Archive.Dispose() }\n",
        1,
    ),
    # PK4: the Windows copy's comments and the MSI's icon id name BashCutPro (the app exe, its App Paths key and the
    # other WiX ids: APP_SUBS, NM2)
    ("windows/package.ps1", "  Build, sign and package FilmCraft for Windows.\n", "  Build, sign and package BashCutPro for Windows.\n", 1),
    (
        "windows/package.ps1",
        "    filmcraft-<version>-windows-<arch>.msi            per-machine installer (WiX v5)\n"
        "    filmcraft-<version>-windows-<arch>-portable.zip   filmcraft.exe + filmcraft-cli.exe + portable.txt\n",
        "    bashcutpro-<version>-windows-<arch>.msi            per-machine installer (WiX v5)\n"
        "    bashcutpro-<version>-windows-<arch>-portable.zip   bashcutpro.exe (the app), bashcutpro-cli.exe,\n"
        "      %s.exe, bashcut-setup.ps1, portable.txt\n" % ".exe, ".join(PRO_BINS),
        1,
    ),
    (
        "windows/package.ps1",
        "# The marker that makes FilmCraft keep its data in .\\data next to the exe",
        "# The marker that makes BashCutPro keep its data in .\\data next to the exe",
        1,
    ),
    ("windows/filmcraft.wxs", "  FilmCraft MSI (WiX Toolset v5", "  BashCutPro MSI (WiX Toolset v5", 1),
    ("windows/filmcraft.wxs", '"FilmcraftIcon.ico"', '"BashCutProIcon.ico"', 3),
    # SB5: the Linux copy names BashCutPro where it tells the user what it installs or builds
    ("linux/install.sh", "  echo 'Installs FilmCraft and", "  echo 'Installs BashCutPro and", 1),
    ("linux/install.sh", "printf 'Installed FilmCraft in ", "printf 'Installed BashCutPro in ", 1),
    ("linux/package.sh", "# Build and package FilmCraft for Linux", "# Build and package BashCutPro for Linux", 1),
    ("linux/package.sh", 'echo "==> FilmCraft $VERSION', 'echo "==> BashCutPro $VERSION', 1),
    ("linux/flatpak-bundle.sh", "# Repackage a FilmCraft Linux tarball", "# Repackage a BashCutPro Linux tarball", 1),
    ("linux/flatpak-bundle.sh", 'echo "==> FilmCraft $VERSION', 'echo "==> BashCutPro $VERSION', 1),
    ("linux/flatpak/ai.storyteller.filmcraft.yml", "# Flatpak manifest for FilmCraft,", "# Flatpak manifest for BashCutPro,", 1),
    ("linux/flatpak/ai.storyteller.filmcraft.yml", "video editors do, FilmCraft reads", "video editors do, BashCutPro reads", 1),
    ("linux/flatpak/ai.storyteller.filmcraft.bundle.yml", "video editors do, FilmCraft reads", "video editors do, BashCutPro reads", 1),
]


# Carried edits (UPSTREAM.md) in files the packaging runs straight from packaging/, not from our copy: checked, so a
# merge that drops one fails here. The Flatpak source manifest is read by flatpak-builder itself (#174); the
# bundle repackages the Linux tarball, which the copy builds with FEATURES.
CARRIED = [("linux/flatpak/ai.storyteller.filmcraft.yml", "-p filmcraft -p filmcraft-cli %s" % FEATURES)]


# SB2: after SUBS, these copied files ship bashcutpro where upstream ships filmcraft-cli, so a BashCutPro package never
# clashes with an installed FilmCraft. (old, new) in order; each file must contain filmcraft-cli, and none may be left.
RENAME_FILES = [
    "macos/package.sh",
    "linux/package.sh",
    "linux/install.sh",
    "linux/nfpm.yaml",
    "linux/flatpak-bundle.sh",
    "linux/flatpak/ai.storyteller.filmcraft.bundle.yml",
    "linux/flatpak/ai.storyteller.filmcraft.yml",
    "linux/test_install.py",
    "freebsd/package.sh",
    "windows/package.ps1",
    "windows/filmcraft.wxs",
]
RENAMES = [
    ('Id="FilmcraftCliExe"', 'Id="BashcutproCliExe"'),
    ('Id="FilmcraftCli"', 'Id="BashcutproCli"'),
    ("filmcraft-cli", "bashcutpro-cli"),
]


# NM2 (owner decision 2026-10-23, upstream's convention): the app is built as `filmcraft` and installed as
# `bashcutpro` (`/usr/bin/bashcutpro`, `bashcutpro.exe`; macOS keeps `BashCutPro.app/Contents/MacOS/BashCutPro`), so
# no installed name says filmcraft. Applied after RENAMES, same rules as SUBS: build-output paths (`$BIN/filmcraft`,
# `target/release/filmcraft`, the PE check in $Bin) keep the cargo name, every installed path changes.
APP_SUBS = [
    # ---- Linux
    ("linux/package.sh", 'install -Dm755 "$BIN/filmcraft" "$STAGE/usr/bin/filmcraft"\n', 'install -Dm755 "$BIN/filmcraft" "$STAGE/usr/bin/bashcutpro"\n', 1),
    ("linux/package.sh", '"${CROSS_COMPILE:-}strip" "$STAGE/usr/bin/filmcraft" ', '"${CROSS_COMPILE:-}strip" "$STAGE/usr/bin/bashcutpro" ', 1),
    ("linux/package.sh", 'ln -s usr/bin/filmcraft "$APPDIR/AppRun"', 'ln -s usr/bin/bashcutpro "$APPDIR/AppRun"', 1),
    ("linux/nfpm.yaml", "  - src: ${STAGE}/usr/bin/filmcraft\n    dst: /usr/bin/filmcraft\n", "  - src: ${STAGE}/usr/bin/bashcutpro\n    dst: /usr/bin/bashcutpro\n", 1),
    ("linux/install.sh", "for binary in filmcraft bashcutpro-cli; do\n", "for binary in bashcutpro bashcutpro-cli; do\n", 1),
    ("linux/install.sh", 'install -m755 -- "$source_dir/bin/filmcraft" "$prefix/bin/filmcraft"\n', 'install -m755 -- "$source_dir/bin/bashcutpro" "$prefix/bin/bashcutpro"\n', 1),
    ("linux/install.sh", "\"$prefix/bin/filmcraft\"", "\"$prefix/bin/bashcutpro\"", 2),
    ("linux/install.sh", 'run \"%s/bin/filmcraft\".', 'run \"%s/bin/bashcutpro\".', 1),
    ("linux/ai.storyteller.filmcraft.desktop", "Exec=filmcraft %F\nTryExec=filmcraft\n", "Exec=bashcutpro %F\nTryExec=bashcutpro\n", 1),
    (
        "linux/ai.storyteller.filmcraft.metainfo.xml.in",
        "    <binary>filmcraft</binary>\n",
        "    <binary>bashcutpro</binary>\n    <binary>bashcutpro-cli</binary>\n    <binary>bcpro</binary>\n",
        1,
    ),
    ("linux/flatpak-bundle.sh", '[ -x "$WORK/stage/bin/filmcraft" ] || { echo "error: $TARBALL has no bin/filmcraft" >&2', '[ -x "$WORK/stage/bin/bashcutpro" ] || { echo "error: $TARBALL has no bin/bashcutpro" >&2', 1),
    ("linux/flatpak/ai.storyteller.filmcraft.bundle.yml", "\ncommand: filmcraft\n", "\ncommand: bashcutpro\n", 1),
    ("linux/flatpak/ai.storyteller.filmcraft.bundle.yml", "      - install -Dm755 bin/filmcraft /app/bin/filmcraft\n", "      - install -Dm755 bin/bashcutpro /app/bin/bashcutpro\n", 1),
    ("linux/flatpak/ai.storyteller.filmcraft.yml", "\ncommand: filmcraft\n", "\ncommand: bashcutpro\n", 1),
    ("linux/flatpak/ai.storyteller.filmcraft.yml", "      - install -Dm755 target/release/filmcraft /app/bin/filmcraft\n", "      - install -Dm755 target/release/filmcraft /app/bin/bashcutpro\n", 1),
    # upstream's installer test, run on the copy: the bundle carries bin/bashcutpro (the tarball test stages from
    # the cargo names, so its `filmcraft` fixture stays)
    ("linux/test_install.py", 'for name in ("filmcraft", "bashcutpro-cli"):\n            (self.bundle', 'for name in ("bashcutpro", "bashcutpro-cli"):\n            (self.bundle', 1),
    ("linux/test_install.py", 'for name in ("filmcraft", "bashcutpro-cli"):\n                self.assertTrue', 'for name in ("bashcutpro", "bashcutpro-cli"):\n                self.assertTrue', 1),
    ("linux/test_install.py", 'bin/filmcraft"', 'bin/bashcutpro"', 3),
    # FX12: the bundle's docs folder is share/doc/bashcutpro (PK3), so the fixture and the check use it too
    ("linux/test_install.py", "doc/filmcraft/LICENSE-MIT", "doc/bashcutpro/LICENSE-MIT", 3),
    # FX12: the tarball test stages what package.sh installs, the Pro tools included (they are not optional there)
    (
        "linux/test_install.py",
        'for name in ("filmcraft", "bashcutpro-cli"):\n            binary = binaries',
        'for name in ("filmcraft", "bashcutpro-cli", %s):\n            binary = binaries' % ", ".join('"%s"' % b for b in TOOLS),
        1,
    ),
    # ---- FreeBSD: the Linux copy's freedesktop files (our app id, Exec=bashcutpro) and the same names
    ("freebsd/package.sh", 'LINUX="$ROOT/packaging/linux"\nAPP_ID=ai.storyteller.filmcraft\n', 'LINUX="$BASHCUT_PACKAGING/linux"\nAPP_ID=%s\n' % "com.birchapps.bashcutpro", 1),
    ("freebsd/package.sh", 'cp -R "$BRAND_ICON_DIR/hicolor" ', 'cp -R "$LINUX/icons/hicolor" ', 1),
    ("freebsd/package.sh", 'BASENAME="filmcraft-$VERSION-freebsd-$ARCH"', 'BASENAME="$BRAND_SLUG-$VERSION-freebsd-$ARCH"', 1),
    (
        "freebsd/package.sh",
        'install -m 755 "$BIN/filmcraft" "$BIN/bashcutpro-cli" "$STAGE/bin/"\n',
        'install -m 755 "$BIN/filmcraft" "$STAGE/bin/bashcutpro"\ninstall -m 755 "$BIN/bashcutpro-cli" "$STAGE/bin/"\n',
        1,
    ),
    ("freebsd/package.sh", 'strip "$STAGE/bin/filmcraft" ', 'strip "$STAGE/bin/bashcutpro" ', 1),
    ("freebsd/package.sh", "for f in bin/filmcraft bin/bashcutpro-cli ", "for f in bin/bashcutpro bin/bashcutpro-cli bin/bcpro ", 1),
    ("freebsd/package.sh", "share/doc/filmcraft", "share/doc/bashcutpro", 6),
    ("freebsd/package.sh", 'echo "==> FilmCraft $VERSION', 'echo "==> BashCutPro $VERSION', 1),
    ("freebsd/package.sh", "# Build and package FilmCraft for FreeBSD:", "# Build and package BashCutPro for FreeBSD:", 1),
    # ---- Windows: stage the app as bashcutpro.exe; MSI file, App Paths key and ids follow
    (
        "windows/package.ps1",
        "Copy-Item (Join-Path $Bin 'filmcraft.exe'), (Join-Path $Bin 'bashcutpro-cli.exe') $Stage\n",
        "Copy-Item (Join-Path $Bin 'filmcraft.exe') (Join-Path $Stage 'bashcutpro.exe')\n"
        "Copy-Item (Join-Path $Bin 'bashcutpro-cli.exe') $Stage\n",
        1,
    ),
    ("windows/package.ps1", "(Join-Path $Stage 'filmcraft.exe')", "(Join-Path $Stage 'bashcutpro.exe')", 1),
    ("windows/filmcraft.wxs", '"FilmcraftFiles"', '"BashcutproFiles"', 2),
    ("windows/filmcraft.wxs", 'Component Id="FilmcraftApp"', 'Component Id="BashcutproApp"', 1),
    ("windows/filmcraft.wxs", 'File Id="FilmcraftExe" Source="$(var.BinDir)\\filmcraft.exe"', 'File Id="BashcutproExe" Source="$(var.BinDir)\\bashcutpro.exe"', 1),
    ("windows/filmcraft.wxs", "App Paths\\filmcraft.exe\" Value=\"[#FilmcraftExe]\"", "App Paths\\bashcutpro.exe\" Value=\"[#BashcutproExe]\"", 1),
]


# SB4: the copy's Linux files use BashCutPro's own app id, so a BashCutPro package never replaces FilmCraft's desktop
# file, metainfo, MIME file, icons or Flatpak. Files named after the id are renamed and every mention replaced, the
# window class included: the app sets the same id as its Wayland app id / X11 class (bashcut_brand::APP_ID, hook in
# apps/filmcraft/src/main.rs, SB5), and StartupWMClass is how the desktop matches its window to our desktop file. The
# .fcproj MIME type keeps upstream's name (application/x-filmcraft, a shared format): whichever app was installed
# last opens it.
UPSTREAM_ID = "ai.storyteller.filmcraft"
APP_ID = "com.birchapps.bashcutpro"
# icons named after the id, copied from the brand folder into the copy (linux/icons/hicolor/<size>/apps/APP_ID.*)
ICON_DIR = os.path.join("bashcut", "brand", "app-icon", "hicolor")


class Fail(Exception):
    pass


def generate(root, out):
    src = os.path.join(root, "packaging")
    if not os.path.isdir(src):
        raise Fail("%s: no packaging/ folder" % root)
    for rel, text in CARRIED:
        with open(os.path.join(src, *rel.split("/")), encoding="utf-8") as fh:
            if text not in fh.read():
                raise Fail("packaging/%s: the carried edit %r is gone; re-apply it (UPSTREAM.md, bashcut/rebrand.md)" % (rel, text))
    if os.path.isdir(out):
        shutil.rmtree(out)
    shutil.copytree(src, out)
    apply(out, SUBS)
    for rel in RENAME_FILES:
        path = os.path.join(out, *rel.split("/"))
        with open(path, encoding="utf-8", newline="") as fh:
            text = fh.read()
        if "filmcraft-cli" not in text:
            raise Fail("packaging/%s: no filmcraft-cli to rename to bashcutpro-cli; upstream changed it: update RENAME_FILES in %s" % (rel, __file__))
        for old, new in RENAMES:
            text = text.replace(old, new)
        with open(path, "w", encoding="utf-8", newline="") as fh:
            fh.write(text)
    apply(out, APP_SUBS)
    linux_app_id(root, out)
    return len(SUBS) + len(APP_SUBS)


def apply(out, subs):
    """Exact-match substitutions: (file under the copy, old, new, expected matches)."""
    for rel, old, new, want in subs:
        path = os.path.join(out, *rel.split("/"))
        with open(path, encoding="utf-8", newline="") as fh:
            text = fh.read()
        # match independently of the checkout's line endings
        crlf = "\r\n" in text
        if crlf:
            text = text.replace("\r\n", "\n")
        got = text.count(old)
        if got != want:
            raise Fail("packaging/%s: expected %d match(es) of %r, found %d; upstream changed it: update SUBS in %s" % (rel, want, old.splitlines()[0], got, __file__))
        text = text.replace(old, new)
        if crlf:
            text = text.replace("\n", "\r\n")
        with open(path, "w", encoding="utf-8", newline="") as fh:
            fh.write(text)


def linux_app_id(root, out):
    linux = os.path.join(out, "linux")
    flatpak = os.path.join(linux, "flatpak")
    for dirpath, _dirs, files in os.walk(linux):
        for name in files:
            path = os.path.join(dirpath, name)
            with open(path, encoding="utf-8", newline="") as fh:
                text = fh.read()
            text = text.replace(UPSTREAM_ID, APP_ID)
            if dirpath == flatpak and name.endswith(".yml"):
                # the from-source manifest builds the checkout and the cargo sources upstream's docs vendor
                rel = lambda p: os.path.relpath(os.path.realpath(p), os.path.realpath(flatpak)).replace(os.sep, "/")
                text = text.replace("@ROOT@", rel(root))
                text = text.replace("@CARGO_SOURCES@", rel(os.path.join(root, "packaging", "linux", "flatpak", "cargo-sources.json")))
            with open(path, "w", encoding="utf-8", newline="") as fh:
                fh.write(text)
            if UPSTREAM_ID in name:
                os.replace(path, os.path.join(dirpath, name.replace(UPSTREAM_ID, APP_ID)))
    src = os.environ.get("BRAND_ICON_DIR")
    src = os.path.join(src, "hicolor") if src else os.path.join(root, ICON_DIR)
    if not os.path.isdir(src):
        raise Fail("%s: no hicolor icons for the Linux packages" % src)
    found = 0
    for size in sorted(os.listdir(src)):
        apps = os.path.join(src, size, "apps")
        if not os.path.isdir(apps):
            continue
        for name in sorted(os.listdir(apps)):
            # licence sidecars stay in the source tree (ATTRIBUTION.md); packages carry the image only
            if name.startswith(UPSTREAM_ID + ".") and not name.endswith(".attribution"):
                dst = os.path.join(linux, "icons", "hicolor", size, "apps")
                os.makedirs(dst, exist_ok=True)
                shutil.copyfile(os.path.join(apps, name), os.path.join(dst, APP_ID + name[len(UPSTREAM_ID):]))
                found += 1
    if not found:
        raise Fail("%s: no %s.* icons to ship as %s" % (src, UPSTREAM_ID, APP_ID))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument("--root", default=os.path.dirname(os.path.dirname(here)))
    ap.add_argument("--out", required=True)
    args = ap.parse_args(argv)
    try:
        n = generate(os.path.abspath(args.root), os.path.abspath(args.out))
    except Fail as e:
        print("bashcut packaging: %s" % e, file=sys.stderr)
        return 1
    print("bashcut packaging: %d additions written to %s" % (n, args.out), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
