#!/usr/bin/env python3
"""BashCutPro packaging generator (#64): a copy of upstream's `packaging/` that also ships the Pro pieces.

The packaging scripts re-exec the copy this writes (bash: `bashcut/brand/packaging.env`, already sourced by
`packaging/env.sh`; Windows: one hook line in `packaging/windows/package.ps1`), so upstream's packaging files are
never edited for the Pro pieces. Every change is an exact-match substitution that must match the expected number of
times: when upstream rewrites a line we depend on, packaging fails loudly here instead of shipping without our tools.

What the copy adds on every OS: speech-to-text (`--features whisper` for `filmcraft` and `filmcraft-cli`, #146), the binaries `bashcutpro` and its alias `bcpro` (the Pro command line: upstream's engine
built under our name by `apps/bashcutpro-cli`; it takes `filmcraft-cli`'s place, so a package never clashes with an
installed FilmCraft, SB2), `bashcut-doctor`, `bashcut-batch`, `bashcut-mcp` (built
with the app) and
the optional first-run setup (`bashcut/packaging/share/`). Data files stay where they are; only scripts and the
manifests that list files (nfpm.yaml, the WiX source, the Linux tarball installer) are copied and changed.

    python3 bashcut/packaging/generate.py --out target/bashcut-packaging [--root .] [--check]
"""

import argparse
import os
import shutil
import sys

# Our binaries next to the app. bashcutpro (#154, SB2: upstream's engine CLI generated under our name by
# apps/bashcutpro-cli) is not listed: it takes filmcraft-cli's place in upstream's lines (RENAMES); bcpro is its alias.
PRO_BINS = ["bcpro", "bashcut-doctor", "bashcut-batch", "bashcut-mcp"]
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
        "# BashCutPro: Pro tools beside the app executable, the engine CLI among them (#187, SB2);\n"
        "# Mac volumes ignore case, so a tool named like the executable (bashcutpro, BashCutPro) is left out: bcpro covers it\n"
        'brand_lc="$(printf %%s "$BRAND_NAME" | tr "[:upper:]" "[:lower:]")"\n'
        'APP_BINS=""; for b in filmcraft-cli %s; do [ "$b" = "$brand_lc" ] || APP_BINS="$APP_BINS $b"; done\n'
        'for b in $APP_BINS; do cp "$WORK/bin/$b" "$APP/Contents/MacOS/$b"; done\n'
        "# first-run setup in Resources/bashcut\n"
        'mkdir -p "$APP/Contents/Resources/bashcut" && cp "$ROOT/bashcut/packaging/share/"* "$APP/Contents/Resources/bashcut/"\n' % BINS_SH,
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
        'cp "$ROOT/bashcut/packaging/share/"* "$CLI_DIR/"\n' % BINS_SH,
        1,
    ),
    (
        "macos/package.sh",
        'sign --options runtime "$CLI_DIR/filmcraft-cli"\n',
        'sign --options runtime "$CLI_DIR/filmcraft-cli"\n'
        'for b in %s; do sign --options runtime "$CLI_DIR/$b"; done\n' % BINS_SH,
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
        'mkdir -p "$STAGE/usr/share/bashcutpro" && cp "$ROOT/bashcut/packaging/share/"* "$STAGE/usr/share/bashcutpro/"\n' % BINS_SH,
        1,
    ),
    ("linux/package.sh", '"$HERE/install.sh"', '"$BASHCUT_PACKAGING/linux/install.sh"', 1),
    ("linux/package.sh", '"$HERE/nfpm.yaml"', '"$BASHCUT_PACKAGING/linux/nfpm.yaml"', 1),
    (
        "linux/nfpm.yaml",
        "  - src: ${STAGE}/usr/bin/filmcraft-cli\n    dst: /usr/bin/filmcraft-cli\n    file_info: { mode: 0755 }\n",
        "  - src: ${STAGE}/usr/bin/filmcraft-cli\n    dst: /usr/bin/filmcraft-cli\n    file_info: { mode: 0755 }\n"
        + "".join("  - src: ${STAGE}/usr/bin/%s\n    dst: /usr/bin/%s\n    file_info: { mode: 0755 }\n" % (b, b) for b in PRO_BINS),
        1,
    ),
    (
        "linux/install.sh",
        'install -m755 -- "$source_dir/bin/filmcraft-cli" "$prefix/bin/filmcraft-cli"\n',
        'install -m755 -- "$source_dir/bin/filmcraft-cli" "$prefix/bin/filmcraft-cli"\n'
        'for b in %s; do if [ -f "$source_dir/bin/$b" ]; then install -m755 -- "$source_dir/bin/$b" "$prefix/bin/$b"; fi; done\n' % BINS_SH,
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
        'mkdir -p "$STAGE/share/bashcutpro" && cp "$ROOT/bashcut/packaging/share/"* "$STAGE/share/bashcutpro/"\n' % BINS_SH,
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
    "freebsd/package.sh",
    "windows/package.ps1",
    "windows/filmcraft.wxs",
]
RENAMES = [
    ("-p filmcraft-cli ", "-p bashcutpro-cli "),
    ("filmcraft-cli/whisper", "bashcutpro-cli/whisper"),
    ('Id="FilmcraftCliExe"', 'Id="BashcutproExe"'),
    ("filmcraft-cli", "bashcutpro"),
]


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
    for rel, old, new, want in SUBS:
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
    for rel in RENAME_FILES:
        path = os.path.join(out, *rel.split("/"))
        with open(path, encoding="utf-8", newline="") as fh:
            text = fh.read()
        if "filmcraft-cli" not in text:
            raise Fail("packaging/%s: no filmcraft-cli to rename to bashcutpro; upstream changed it: update RENAME_FILES in %s" % (rel, __file__))
        for old, new in RENAMES:
            text = text.replace(old, new)
        with open(path, "w", encoding="utf-8", newline="") as fh:
            fh.write(text)
    return len(SUBS)


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
