#!/usr/bin/env python3
"""Tests for bashcut/packaging/generate.py (#64): python3 bashcut/packaging/test_generate.py"""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import xml.dom.minidom

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)

import generate  # noqa: E402


class GenerateTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="bashcut-packaging-")
        self.out = os.path.join(self.tmp, "out")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def read(self, rel):
        with open(os.path.join(self.out, *rel.split("/")), encoding="utf-8") as fh:
            return fh.read()

    def test_every_substitution_applies_to_the_current_files(self):
        self.assertEqual(generate.generate(ROOT, self.out), len(generate.SUBS) + len(generate.APP_SUBS))
        for rel in ("macos/package.sh", "linux/package.sh", "freebsd/package.sh", "windows/package.ps1"):
            text = self.read(rel)
            for b in generate.PRO_BINS:
                self.assertIn(b, text, rel)
        wxs = self.read("windows/filmcraft.wxs")
        self.assertIn("bashcut-mcp.exe", wxs)
        # the Pro command line ships under both names, in place of filmcraft-cli (#154, SB2, NM2)
        for rel in generate.RENAME_FILES:
            self.assertNotIn("filmcraft-cli", self.read(rel.replace(generate.UPSTREAM_ID, generate.APP_ID)), rel)
        for b in ("bashcutpro", "bashcutpro-cli", "bcpro"):
            self.assertIn("\\%s.exe" % b, wxs)
            self.assertIn("/usr/bin/%s\n" % b, self.read("linux/nfpm.yaml"))
        self.assertIn('File Id="BashcutproCliExe"', wxs)
        self.assertIn('Component Id="BashcutDoctor"', wxs)
        self.assertIn('File Id="BcproExe"', wxs)
        self.assertIn("-p filmcraft -p bashcutpro-cli --features", self.read("windows/package.ps1"))
        # speech-to-text ships in the app and the engine CLI on every OS (#146)
        for rel in ("macos/package.sh", "linux/package.sh", "freebsd/package.sh", "windows/package.ps1"):
            self.assertIn("--features filmcraft/whisper,bashcutpro-cli/whisper", self.read(rel), rel)
        self.assertIn("/usr/bin/bashcut-doctor", self.read("linux/nfpm.yaml"))
        self.assertIn("BASHCUT_ROOT", self.read("env.sh"))
        # upstream's own files are untouched
        with open(os.path.join(ROOT, "packaging", "env.sh"), encoding="utf-8") as fh:
            self.assertNotIn("BASHCUT_ROOT:?", fh.read())

    def test_flatpak_builds_with_speech_and_losing_it_fails(self):
        # flatpak-builder reads the source manifest from packaging/ itself, so speech is a carried edit there (#174)
        with open(os.path.join(ROOT, "packaging", "linux", "flatpak", "ai.storyteller.filmcraft.yml"), encoding="utf-8") as fh:
            self.assertIn("-p filmcraft -p filmcraft-cli --features filmcraft/whisper,filmcraft-cli/whisper", fh.read())
        fake = os.path.join(self.tmp, "root")
        shutil.copytree(os.path.join(ROOT, "packaging"), os.path.join(fake, "packaging"))
        path = os.path.join(fake, "packaging", "linux", "flatpak", "ai.storyteller.filmcraft.yml")
        with open(path, encoding="utf-8") as fh:
            text = fh.read()
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text.replace(" --features filmcraft/whisper,filmcraft-cli/whisper", ""))
        with self.assertRaises(generate.Fail) as e:
            generate.generate(fake, self.out)
        self.assertIn("flatpak", str(e.exception))

    def test_copies_parse(self):
        generate.generate(ROOT, self.out)
        xml.dom.minidom.parse(os.path.join(self.out, "windows", "filmcraft.wxs"))
        bash = shutil.which("bash")
        if bash:
            for rel in ("env.sh", "macos/package.sh", "linux/package.sh", "linux/install.sh", "linux/flatpak-bundle.sh", "freebsd/package.sh"):
                r = subprocess.run([bash, "-n", os.path.join(self.out, *rel.split("/"))], capture_output=True, text=True)
                self.assertEqual(r.returncode, 0, rel + ": " + r.stderr)

    def test_linux_copy_names_bashcutpro(self):
        # SB5: what the Linux scripts print and the Flatpak manifests' comments name BashCutPro, not FilmCraft
        generate.generate(ROOT, self.out)
        install = self.read("linux/install.sh")
        self.assertIn("printf 'Installed BashCutPro in ", install)
        self.assertNotIn("FilmCraft", install)
        for rel in ("linux/package.sh", "linux/flatpak-bundle.sh"):
            self.assertIn('echo "==> BashCutPro $VERSION', self.read(rel), rel)
        for rel in ("linux/flatpak/%s.yml" % generate.APP_ID, "linux/flatpak/%s.bundle.yml" % generate.APP_ID):
            self.assertNotIn("FilmCraft", self.read(rel), rel)

    def test_windows_copy_names_bashcutpro(self):
        # PK1 + PK4: what the Windows copy prints, its comments, portable.txt and the MSI's icon id name BashCutPro
        generate.generate(ROOT, self.out)
        ps1 = self.read("windows/package.ps1")
        wxs = self.read("windows/filmcraft.wxs")
        self.assertIn('Write-Output "BashCutPro $Version for Windows', ps1)
        self.assertIn("Build, sign and package BashCutPro for Windows.", ps1)
        self.assertIn("Delete this file to use %APPDATA%\\BashCutPro instead.", ps1)
        self.assertIn("bashcutpro-<version>-windows-<arch>-portable.zip", ps1)
        for text, rel in ((ps1, "package.ps1"), (wxs, "filmcraft.wxs")):
            self.assertNotIn("FilmCraft", text, rel)
        self.assertNotIn("FilmcraftIcon", wxs)
        self.assertEqual(wxs.count('"BashCutProIcon.ico"'), 3)
        # NM2: the app is staged and installed as bashcutpro.exe (App Paths key too); only cargo's output keeps filmcraft
        self.assertIn("App Paths\\bashcutpro.exe\" Value=\"[#BashcutproExe]\"", wxs)
        self.assertIn("Copy-Item (Join-Path $Bin 'filmcraft.exe') (Join-Path $Stage 'bashcutpro.exe')", ps1)
        self.assertNotIn("filmcraft", wxs.replace("wix build filmcraft.wxs", ""))
        self.assertIn("next to bashcutpro.exe.", ps1)

    def test_archives_ship_our_readme_and_the_pe_check_names_the_installed_exe(self):
        # BR1: copy_docs (Linux, FreeBSD, macOS CLI zip, web) and the portable zip copy BashCutPro's README, never
        # upstream's; the README itself names BashCutPro and its CLI, in English and Vietnamese
        generate.generate(ROOT, self.out)
        env = self.read("env.sh")
        ps1 = self.read("windows/package.ps1")
        self.assertIn('cp "$ROOT/bashcut/packaging/share/README.md" "$dest/README.md"', env)
        self.assertNotIn("for f in README.md", env)
        self.assertIn("Copy-Item (Join-Path $Root 'bashcut\\packaging\\share\\README.md') $Portable", ps1)
        self.assertNotIn("'README.md', 'LICENSE'", ps1)
        self.assertIn("-replace '^filmcraft\\.exe$', 'bashcutpro.exe'", ps1)
        with open(os.path.join(HERE, "share", "README.md"), encoding="utf-8") as fh:
            readme = fh.read()
        for text in ("# BashCutPro", "bashcutpro-cli", "bcpro", "https://github.com/dongnguyenvie/BashCutPro", "Tiếng Việt"):
            self.assertIn(text, readme)
        self.assertNotIn("getartcraft", readme)

    def test_share_folders_get_the_setup_scripts_but_not_the_readme(self):
        # BR2: share/README.md goes to the archive root (copy_docs) only; the setup folders get bashcut-setup.*
        generate.generate(ROOT, self.out)
        dests = {
            "macos/package.sh": ('"$APP/Contents/Resources/bashcut/"', '"$CLI_DIR/"'),
            "linux/package.sh": ('"$STAGE/usr/share/bashcutpro/"',),
            "freebsd/package.sh": ('"$STAGE/share/bashcutpro/"',),
            "linux/flatpak/%s.yml" % generate.APP_ID: ("/app/share/bashcutpro/",),
        }
        for rel, folders in dests.items():
            text = self.read(rel)
            self.assertNotIn("packaging/share/*", text, rel)
            self.assertNotIn('packaging/share/"*', text, rel)
            for folder in folders:
                self.assertIn("bashcut-setup.* " + folder, text, rel)
        names = sorted(os.listdir(os.path.join(HERE, "share")))
        self.assertEqual([n for n in names if not n.startswith("bashcut-setup.")], ["README.md"])

    def test_windows_zip_entries_use_forward_slashes(self):
        # PK4: Compress-Archive on Windows PowerShell 5.1 writes zip entry names with backslashes
        generate.generate(ROOT, self.out)
        ps1 = self.read("windows/package.ps1")
        self.assertNotIn("Compress-Archive -Path", ps1)
        self.assertIn(".Replace('\\', '/')", ps1)
        shell = shutil.which("pwsh") or shutil.which("powershell")
        if not shell:
            self.skipTest("no pwsh / powershell to parse the copy")
        parse = (
            "& { param($p) $e = $null; $null = [System.Management.Automation.Language.Parser]::ParseFile($p, [ref]$null, [ref]$e);"
            " if ($e) { $e | ForEach-Object { Write-Error $_.Message }; exit 1 } }"
        )
        path = os.path.join(self.out, "windows", "package.ps1")
        r = subprocess.run([shell, "-NoProfile", "-NonInteractive", "-Command", parse, path], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_linux_packages_use_our_app_id(self):
        # SB4: desktop file, metainfo, MIME file, icons and Flatpak are com.birchapps.bashcutpro, never FilmCraft's
        generate.generate(ROOT, self.out)
        linux = os.path.join(self.out, "linux")
        app = generate.APP_ID
        for dirpath, _dirs, files in os.walk(linux):
            for name in files:
                rel = os.path.relpath(os.path.join(dirpath, name), self.out)
                self.assertNotIn(generate.UPSTREAM_ID, name, rel)
                if name.endswith(".png"):
                    continue
                text = self.read(rel)
                self.assertNotIn("filmcraft-cli", text, rel)
                # the window class too: the app sets our id (bashcut_brand::APP_ID, SB5)
                self.assertNotIn(generate.UPSTREAM_ID, text, rel)
        desktop = self.read("linux/%s.desktop" % app)
        self.assertIn("StartupWMClass=%s\n" % app, desktop)
        self.assertIn("Icon=%s\n" % app, desktop)
        self.assertIn("Exec=bashcutpro %F\nTryExec=bashcutpro\n", desktop)
        meta = xml.dom.minidom.parseString(self.read("linux/%s.metainfo.xml.in" % app).replace("@VERSION@", "1").replace("@DATE@", "2026-01-01"))
        self.assertEqual(meta.getElementsByTagName("id")[0].firstChild.data, app)
        self.assertEqual(meta.getElementsByTagName("launchable")[0].firstChild.data, app + ".desktop")
        mime = xml.dom.minidom.parseString(self.read("linux/%s.mime.xml" % app))
        # .fcproj keeps upstream's shared type name; whichever app was installed last opens it
        self.assertEqual(mime.getElementsByTagName("mime-type")[0].getAttribute("type"), "application/x-filmcraft")
        self.assertEqual(mime.getElementsByTagName("icon")[0].getAttribute("name"), app)
        for icon in ("256x256/apps/%s.png" % app, "scalable/apps/%s.svg" % app):
            self.assertTrue(os.path.isfile(os.path.join(linux, "icons", "hicolor", *icon.split("/"))), icon)
        for rel in ("linux/package.sh", "linux/flatpak-bundle.sh", "linux/install.sh"):
            self.assertRegex(self.read(rel), r"(?i)app_id=%s\n" % app.replace(".", r"\."), rel)
        for rel in ("linux/package.sh", "linux/flatpak-bundle.sh"):
            self.assertIn('HERE="$BASHCUT_PACKAGING/linux"\n', self.read(rel), rel)
        self.assertNotIn("BRAND_ICON_DIR", self.read("linux/package.sh"))
        self.assertIn("--command=bashcutpro-cli ", self.read("linux/flatpak-bundle.sh"))
        for rel in ("linux/flatpak/%s.yml" % app, "linux/flatpak/%s.bundle.yml" % app):
            text = self.read(rel)
            self.assertIn("\nid: %s\n" % app, text, rel)
            self.assertIn("\ncommand: bashcutpro\n", text, rel)
            for b in ["bashcutpro", "bashcutpro-cli"] + generate.PRO_BINS:
                self.assertIn("/app/bin/%s\n" % b, text, rel)

    def test_no_installed_name_says_filmcraft(self):
        # NM2: the app installs as bashcutpro and the CLI as bashcutpro-cli + bcpro; `filmcraft` stays only as
        # cargo's output name ($BIN/filmcraft, target/release/filmcraft, the PE check in $Bin)
        generate.generate(ROOT, self.out)
        import re

        installed = {
            "linux/package.sh": r'"\$STAGE/usr/bin/(\S+?)"',
            "linux/install.sh": r'"\$prefix/bin/(\S+?)"',
            "linux/nfpm.yaml": r"dst: /usr/bin/(\S+)",
            "linux/flatpak/%s.yml" % generate.APP_ID: r"/app/bin/(\S+)",
            "linux/flatpak/%s.bundle.yml" % generate.APP_ID: r"/app/bin/(\S+)",
            "freebsd/package.sh": r'"\$STAGE/bin/(\S+?)"',
            "windows/filmcraft.wxs": r"\\(\S+?)\.exe\"",
        }
        for rel, pattern in installed.items():
            names = set(re.findall(pattern, self.read(rel)))
            self.assertIn("bashcutpro", names, rel)
            self.assertFalse([n for n in names if "filmcraft" in n], rel)
        linux = self.read("linux/package.sh")
        self.assertIn('install -Dm755 "$BIN/filmcraft" "$STAGE/usr/bin/bashcutpro"\n', linux)
        self.assertIn('ln -s usr/bin/bashcutpro "$APPDIR/AppRun"', linux)
        self.assertIn('ln -s bashcutpro-cli "$STAGE/usr/bin/bcpro"', linux)
        install = self.read("linux/install.sh")
        self.assertIn("for binary in bashcutpro bashcutpro-cli; do", install)
        self.assertIn('ln -sfn -- bashcutpro-cli "$prefix/bin/bcpro"', install)
        freebsd = self.read("freebsd/package.sh")
        self.assertIn('install -m 755 "$BIN/filmcraft" "$STAGE/bin/bashcutpro"\n', freebsd)
        self.assertIn("APP_ID=%s\n" % generate.APP_ID, freebsd)
        self.assertIn('BASENAME="$BRAND_SLUG-', freebsd)
        self.assertNotIn("share/doc/filmcraft", freebsd)
        meta = self.read("linux/%s.metainfo.xml.in" % generate.APP_ID)
        for b in ("bashcutpro", "bashcutpro-cli", "bcpro"):
            self.assertIn("<binary>%s</binary>" % b, meta)
        mac = self.read("macos/package.sh")
        self.assertIn('cp "$WORK/bin/filmcraft" "$APP/Contents/MacOS/$BRAND_NAME"', mac)
        self.assertIn('ln -s bashcutpro-cli "$APP/Contents/MacOS/bcpro"', mac)
        self.assertIn('ln -s bashcutpro-cli "$CLI_DIR/bcpro"', mac)

    def test_the_flatpak_source_manifest_finds_its_files(self):
        # flatpak-builder reads the generated manifest: its sources are the checkout and the generated Linux files
        generate.generate(ROOT, self.out)
        flatpak = os.path.join(self.out, "linux", "flatpak")
        text = self.read("linux/flatpak/%s.yml" % generate.APP_ID)
        self.assertIn("-p filmcraft -p bashcutpro-cli --features filmcraft/whisper,bashcutpro-cli/whisper", text)
        paths = [l.split("path:", 1)[1].strip() for l in text.splitlines() if l.strip().startswith("path:")]
        self.assertEqual(os.path.realpath(os.path.join(flatpak, paths[0])), os.path.realpath(ROOT))
        self.assertEqual(os.path.realpath(os.path.join(flatpak, paths[1])), os.path.realpath(os.path.join(self.out, "linux")))
        self.assertIn("      - %s\n" % os.path.relpath(os.path.realpath(os.path.join(ROOT, "packaging", "linux", "flatpak", "cargo-sources.json")), os.path.realpath(flatpak)).replace(os.sep, "/"), text)
        for rel in set(w for w in text.split() if w.startswith("bashcut-packaging/linux/")):
            self.assertTrue(os.path.exists(os.path.join(self.out, rel[len("bashcut-packaging/"):].rstrip("/"))), rel)
        self.assertTrue(os.path.isdir(os.path.join(ROOT, "bashcut", "packaging", "share")))
        try:
            import yaml
        except ImportError:
            return
        for name in ("%s.yml", "%s.bundle.yml"):
            with open(os.path.join(flatpak, name % generate.APP_ID), encoding="utf-8") as fh:
                doc = yaml.safe_load(fh)
            self.assertEqual((doc["id"], doc["command"]), (generate.APP_ID, "bashcutpro"))

    def test_the_copied_tarball_installer_installs_ours(self):
        # upstream's installer test, run on the copy (needs GNU realpath -m, so Linux)
        if os.name == "nt":
            self.skipTest("Linux installer test: Git Bash has realpath -m but not the Linux paths it installs to")
        generate.generate(ROOT, self.out)
        r = subprocess.run(["realpath", "-m", "/a/../b"], capture_output=True, text=True) if shutil.which("realpath") else None
        if not r or r.returncode != 0:
            self.skipTest("no GNU realpath -m")
        # the copy's package.sh runs as packaging.env re-executes it (FX12: it stopped at env.sh without these)
        env = {**os.environ, "BASHCUT_BRANDED": "1", "BASHCUT_ROOT": ROOT, "BASHCUT_PACKAGING": self.out}
        r = subprocess.run([sys.executable, os.path.join(self.out, "linux", "test_install.py")], capture_output=True, text=True, env=env)
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_the_app_keeps_its_executable_and_ships_the_engine_cli(self):
        # Mac volumes ignore case: copying bashcutpro beside BashCutPro replaced the app executable (#187); the engine
        # is bashcutpro-cli now (NM2), so it ships in the app under its own name with bcpro linked to it
        generate.generate(ROOT, self.out)
        lines = [l for l in self.read("macos/package.sh").splitlines() if l.startswith("APP_BINS=")]
        self.assertEqual(len(lines), 1)
        bins = lines[0].split("=", 1)[1].strip('"').split()
        self.assertEqual(bins[0], "bashcutpro-cli")
        self.assertNotIn("filmcraft-cli", bins)
        self.assertNotIn("bcpro", bins)
        self.assertFalse([b for b in bins if b.lower() == "bashcutpro"])

    def test_bcpro_is_a_link_and_linux_docs_and_metainfo_are_ours(self):
        # PK3 + NM2: bcpro is a relative symlink to bashcutpro-cli wherever a package can hold one, a real file on
        # Windows; Linux and FreeBSD docs go to share/doc/bashcutpro; the metainfo opens with a real sentence (SB4)
        generate.generate(ROOT, self.out)
        links = {
            "linux/package.sh": 'ln -s bashcutpro-cli "$STAGE/usr/bin/bcpro"\n',
            "freebsd/package.sh": 'ln -s bashcutpro-cli "$STAGE/bin/bcpro"\n',
            "linux/install.sh": 'if [[ -L "$source_dir/bin/bcpro" ]]; then ln -sfn -- bashcutpro-cli "$prefix/bin/bcpro"; fi\n',
            "linux/flatpak/%s.yml" % generate.APP_ID: "- ln -s bashcutpro-cli /app/bin/bcpro\n",
            "linux/flatpak/%s.bundle.yml" % generate.APP_ID: "- ln -s bashcutpro-cli /app/bin/bcpro\n",
        }
        for rel, line in links.items():
            text = self.read(rel)
            self.assertIn(line, text, rel)
            self.assertNotRegex(text, r"\b(cp|install)\b[^\n]*\bbcpro\b", rel + ": bcpro is never a copy")
        mac = self.read("macos/package.sh")
        self.assertIn('ln -s bashcutpro-cli "$APP/Contents/MacOS/bcpro"\n', mac)
        self.assertIn('ln -s bashcutpro-cli "$CLI_DIR/bcpro"\n', mac)
        # the app's tools are copied and signed one by one: bcpro (a link) is not among them
        self.assertNotIn("bcpro", mac.split("APP_BINS=", 1)[1].split("\n", 1)[0])
        # nfpm packs the staged link as it is (type: symlink wrote a bad mtime, PK3)
        nfpm = self.read("linux/nfpm.yaml")
        entry = nfpm.split("  - src: ${STAGE}/usr/bin/bcpro\n", 1)[1].split("  - src:", 1)[0]
        self.assertEqual(entry, "    dst: /usr/bin/bcpro\n")
        # Windows has no links in a zip or MSI: bcpro.exe is its own file
        ps1 = self.read("windows/package.ps1")
        self.assertIn("foreach ($b in 'bcpro', ", ps1)
        self.assertIn('<File Id="BcproExe" Source="$(var.BinDir)\\bcpro.exe"', self.read("windows/filmcraft.wxs"))
        # docs
        linux = self.read("linux/package.sh")
        self.assertIn('copy_docs "$STAGE/usr/share/doc/bashcutpro"\n', linux)
        self.assertIn('copy_docs "$STAGE/share/doc/bashcutpro"\n', self.read("freebsd/package.sh"))
        for rel in ("linux/package.sh", "freebsd/package.sh", "linux/flatpak/%s.bundle.yml" % generate.APP_ID, "linux/test_install.py"):
            self.assertNotIn("doc/filmcraft", self.read(rel), rel)
        # the metainfo's first paragraph is a sentence, not a fragment
        meta = xml.dom.minidom.parseString(self.read("linux/%s.metainfo.xml.in" % generate.APP_ID).replace("@VERSION@", "1").replace("@DATE@", "2026-01-01"))
        first = " ".join(meta.getElementsByTagName("description")[0].getElementsByTagName("p")[0].firstChild.data.split())
        self.assertGreaterEqual(len(first), 80, first)
        self.assertTrue(first.startswith("BashCutPro ") and first.endswith("."), first)

    def test_macos_cli_zip_has_no_appledouble_and_the_banner_names_bashcutpro(self):
        # PV3: the CLI zip is made without resource forks and extended attributes (no `._*` entries), and the
        # package banner names BashCutPro
        generate.generate(ROOT, self.out)
        mac = self.read("macos/package.sh")
        zips = [l for l in mac.splitlines() if l.startswith("ditto -c -k") and "$CLI_ZIP" in l]
        self.assertEqual(zips, ['ditto -c -k --norsrc --noextattr --keepParent "$CLI_DIR" "$CLI_ZIP"'])
        self.assertIn('echo "==> BashCutPro $VERSION for macOS (', mac)
        self.assertNotIn('echo "==> FilmCraft', mac)
        if sys.platform == "darwin" and shutil.which("ditto") and shutil.which("xattr"):
            # the flags really leave a file's extended attributes out
            src = os.path.join(self.tmp, "cli")
            os.makedirs(src)
            tool = os.path.join(src, "bashcutpro-cli")
            with open(tool, "w", encoding="utf-8") as fh:
                fh.write("#!/bin/sh\n")
            subprocess.run(["xattr", "-w", "com.example.test", "1", tool], check=True)
            out = os.path.join(self.tmp, "cli.zip")
            subprocess.run(["ditto", "-c", "-k", "--norsrc", "--noextattr", "--keepParent", src, out], check=True)
            import zipfile

            names = zipfile.ZipFile(out).namelist()
            self.assertIn("cli/bashcutpro-cli", names)
            self.assertFalse([n for n in names if "/._" in n or n.startswith("._") or "__MACOSX" in n], names)

    def test_appimage_and_flatpak_start_the_installed_app(self):
        # NM2: the AppImage's AppRun, the desktop file's Exec and both Flatpak manifests' command name the same
        # installed app, bashcutpro
        generate.generate(ROOT, self.out)
        import re

        linux = self.read("linux/package.sh")
        apprun = re.findall(r'ln -s usr/bin/(\S+) "\$APPDIR/AppRun"', linux)
        self.assertEqual(apprun, ["bashcutpro"])
        self.assertIn('install -Dm755 "$BIN/filmcraft" "$STAGE/usr/bin/bashcutpro"\n', linux)
        exe = re.findall(r"^Exec=(\S+)", self.read("linux/%s.desktop" % generate.APP_ID), re.M)
        self.assertEqual(exe, ["bashcutpro"])
        for rel in ("linux/flatpak/%s.yml" % generate.APP_ID, "linux/flatpak/%s.bundle.yml" % generate.APP_ID):
            text = self.read(rel)
            self.assertEqual(re.findall(r"^command: (\S+)$", text, re.M), ["bashcutpro"], rel)
            self.assertIn("/app/bin/bashcutpro\n", text, rel)

    def test_the_output_is_deterministic(self):
        # same input, byte-identical tree (bashcut/scripts/README.md): two runs into different folders
        again = os.path.join(self.tmp, "again")
        generate.generate(ROOT, self.out)
        generate.generate(ROOT, again)

        def tree(top):
            files = {}
            for dirpath, _dirs, names in os.walk(top):
                for name in names:
                    path = os.path.join(dirpath, name)
                    with open(path, "rb") as fh:
                        files[os.path.relpath(path, top)] = (fh.read(), os.access(path, os.X_OK))
            return files

        first, second = tree(self.out), tree(again)
        self.assertTrue(first)
        self.assertEqual(sorted(first), sorted(second))
        for rel in first:
            self.assertEqual(first[rel], second[rel], rel)

    def test_a_changed_upstream_line_fails_loudly(self):
        fake = os.path.join(self.tmp, "root")
        shutil.copytree(os.path.join(ROOT, "packaging"), os.path.join(fake, "packaging"))
        path = os.path.join(fake, "packaging", "macos", "package.sh")
        with open(path, encoding="utf-8") as fh:
            text = fh.read()
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text.replace("for bin in filmcraft filmcraft-cli; do", "for bin in filmcraft filmcraft-cli other; do"))
        with self.assertRaises(generate.Fail) as e:
            generate.generate(fake, self.out)
        self.assertIn("macos/package.sh", str(e.exception))

    def test_crlf_checkouts_keep_their_line_endings(self):
        fake = os.path.join(self.tmp, "root")
        shutil.copytree(os.path.join(ROOT, "packaging"), os.path.join(fake, "packaging"))
        shutil.copytree(os.path.join(ROOT, generate.ICON_DIR), os.path.join(fake, generate.ICON_DIR))
        path = os.path.join(fake, "packaging", "linux", "nfpm.yaml")
        with open(path, encoding="utf-8", newline="") as fh:
            text = fh.read().replace("\r\n", "\n").replace("\n", "\r\n")
        with open(path, "w", encoding="utf-8", newline="") as fh:
            fh.write(text)
        generate.generate(fake, self.out)
        with open(os.path.join(self.out, "linux", "nfpm.yaml"), encoding="utf-8", newline="") as fh:
            out = fh.read()
        self.assertIn("bashcut-batch", out)
        self.assertNotIn("\n", out.replace("\r\n", ""))

    def credits_root(self, json_text, people=True):
        fake = os.path.join(self.tmp, "root")
        os.makedirs(os.path.join(fake, "contributors"), exist_ok=True)
        if json_text is not None:
            with open(os.path.join(fake, "contributors", "contributors.json"), "w", encoding="utf-8") as fh:
                fh.write(json_text)
        if people:
            with open(os.path.join(fake, "contributors", "people.toml"), "w", encoding="utf-8") as fh:
                fh.write('[people.alice]\nname = "Alice"\n')
        return fake

    def test_fresh_credits_forces_the_build_script_to_rerun(self):
        fake = self.credits_root('{"contributors": [{"login": "alice"}, {"login": "bob"}]}')
        paths = [os.path.join(fake, "contributors", n) for n in ("contributors.json", "people.toml")]
        for path in paths:
            os.utime(path, (1_000_000_000, 1_000_000_000))
        self.assertEqual(generate.fresh_credits(fake), 2)
        for path in paths:
            self.assertGreater(os.stat(path).st_mtime, 1_000_000_000 + 3600, path)

    def test_fresh_credits_fails_without_contributors(self):
        json_path = os.path.join("contributors", "contributors.json")
        for text in (None, "{not json", '["alice"]', '{"contributors": []}', '{"contributors": {"alice": 1}}', "{}"):
            with self.subTest(text=text):
                fake = self.credits_root(text)
                with self.assertRaises(generate.Fail) as e:
                    generate.fresh_credits(fake)
                self.assertIn(os.path.join(fake, json_path), str(e.exception))
                shutil.rmtree(fake)
        fake = self.credits_root('{"contributors": [{"login": "alice"}]}', people=False)
        with self.assertRaises(generate.Fail) as e:
            generate.fresh_credits(fake)
        self.assertIn(os.path.join(fake, "contributors", "people.toml"), str(e.exception))
        self.assertIn("missing", str(e.exception))

@unittest.skipIf(os.name == "nt" or not shutil.which("bash"), "bashcut-setup.sh runs on macOS / Linux / FreeBSD")
class SetupScriptTest(unittest.TestCase):
    """bashcut/packaging/share/bashcut-setup.sh run from a fake install, every program in it a recording stand-in."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="bashcut-setup-")
        self.log = os.path.join(self.tmp, "ran.log")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def fake(self, path, name):
        # a program that appends "<name> <args>" to the log
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as fh:
            fh.write('#!/bin/sh\necho "%s $*" >>"%s"\n' % (name, self.log))
        os.chmod(path, 0o755)

    def run_setup(self, setup_dir):
        os.makedirs(setup_dir, exist_ok=True)
        script = os.path.join(setup_dir, "bashcut-setup.sh")
        shutil.copy(os.path.join(HERE, "share", "bashcut-setup.sh"), script)
        env = {"PATH": "/usr/bin:/bin", "HOME": self.tmp, "BASHCUTPRO_DATA_DIR": os.path.join(self.tmp, "data")}
        r = subprocess.run(["bash", script, "--yes", "--skip-kit"], capture_output=True, text=True, env=env, timeout=60)
        self.assertEqual(r.returncode, 0, r.stderr)
        ran = ""
        if os.path.exists(self.log):
            with open(self.log, encoding="utf-8") as fh:
                ran = fh.read()
        return r.stdout, ran

    def test_linux_layout_runs_the_engine_not_the_gui_beside_it(self):
        # PV2 / NM2: share/bashcutpro/bashcut-setup.sh with bin/bashcutpro (the app) and bin/bashcutpro-cli beside
        # it: looks, profiles and the doctor's --cli use bashcutpro-cli, the app never starts
        prefix = os.path.join(self.tmp, "usr")
        for name in ("bashcutpro", "bcpro", "filmcraft-cli"):
            self.fake(os.path.join(prefix, "bin", name), name)
        self.fake(os.path.join(prefix, "bin", "bashcutpro-cli"), "cli")
        self.fake(os.path.join(prefix, "bin", "bashcut-doctor"), "doctor")
        out, ran = self.run_setup(os.path.join(prefix, "share", "bashcutpro"))
        cli = os.path.join(prefix, "bin", "bashcutpro-cli")
        self.assertIn("command line (bashcutpro-cli) %s\n" % cli, out)
        self.assertNotIn("/../", out, "printed without ../..")
        self.assertIn("cli exec bashcut.looks.export dir=", ran)
        self.assertIn("cli exec bashcut.profiles.compile", ran)
        self.assertIn("doctor --cli %s" % cli, ran)
        for other in ("bashcutpro ", "bcpro ", "filmcraft-cli "):
            self.assertNotIn("\n" + other, "\n" + ran, other + "ran")

    def test_macos_app_layout_never_takes_the_app_executable(self):
        # #187: Resources/bashcut/bashcut-setup.sh with Contents/MacOS/BashCutPro (the app) and bashcutpro-cli
        macos = os.path.join(self.tmp, "BashCutPro.app", "Contents", "MacOS")
        self.fake(os.path.join(macos, "BashCutPro"), "gui")
        self.fake(os.path.join(macos, "bashcutpro-cli"), "cli")
        out, ran = self.run_setup(os.path.join(self.tmp, "BashCutPro.app", "Contents", "Resources", "bashcut"))
        self.assertIn("command line (bashcutpro-cli) %s\n" % os.path.join(macos, "bashcutpro-cli"), out)
        self.assertIn("cli exec bashcut.looks.export", ran)
        self.assertNotIn("gui", ran)

    def test_falls_back_to_bcpro_and_never_to_the_gui(self):
        prefix = os.path.join(self.tmp, "usr")
        self.fake(os.path.join(prefix, "bin", "bashcutpro"), "gui")
        self.fake(os.path.join(prefix, "bin", "bcpro"), "bcpro")
        out, ran = self.run_setup(os.path.join(prefix, "share", "bashcutpro"))
        self.assertIn("command line (bashcutpro-cli) %s\n" % os.path.join(prefix, "bin", "bcpro"), out)
        self.assertIn("bcpro exec bashcut.looks.export", ran)
        # only the app: no engine, nothing runs
        os.remove(os.path.join(prefix, "bin", "bcpro"))
        os.remove(self.log)
        out, ran = self.run_setup(os.path.join(prefix, "share", "bashcutpro"))
        self.assertIn("command line (bashcutpro-cli) not found\n", out)
        self.assertEqual(ran, "")


if __name__ == "__main__":
    unittest.main()
