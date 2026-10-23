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
        self.assertEqual(generate.generate(ROOT, self.out), len(generate.SUBS))
        for rel in ("macos/package.sh", "linux/package.sh", "freebsd/package.sh", "windows/package.ps1"):
            text = self.read(rel)
            for b in generate.PRO_BINS:
                self.assertIn(b, text, rel)
        wxs = self.read("windows/filmcraft.wxs")
        self.assertIn("bashcut-mcp.exe", wxs)
        # the Pro command line ships under both names, in place of filmcraft-cli (#154, SB2)
        for rel in generate.RENAME_FILES:
            self.assertNotIn("filmcraft-cli", self.read(rel.replace(generate.UPSTREAM_ID, generate.APP_ID)), rel)
        for b in ("bashcutpro", "bcpro"):
            self.assertIn("\\%s.exe" % b, wxs)
            self.assertIn("/usr/bin/%s" % b, self.read("linux/nfpm.yaml"))
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

    @unittest.skip("test ticket: the Windows copy prints BashCutPro and portable.txt names %APPDATA%\\BashCutPro")
    def test_windows_copy_names_bashcutpro(self):
        pass

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
        self.assertIn("Exec=filmcraft %F\n", desktop)
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
        self.assertIn("--command=bashcutpro ", self.read("linux/flatpak-bundle.sh"))
        for rel in ("linux/flatpak/%s.yml" % app, "linux/flatpak/%s.bundle.yml" % app):
            text = self.read(rel)
            self.assertIn("\nid: %s\n" % app, text, rel)
            for b in ["bashcutpro"] + generate.PRO_BINS:
                self.assertIn("/app/bin/%s\n" % b, text, rel)

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
            self.assertEqual((doc["id"], doc["command"]), (generate.APP_ID, "filmcraft"))

    def test_the_copied_tarball_installer_installs_ours(self):
        # upstream's installer test, run on the copy (needs GNU realpath -m, so Linux)
        if os.name == "nt":
            self.skipTest("Linux installer test: Git Bash has realpath -m but not the Linux paths it installs to")
        generate.generate(ROOT, self.out)
        r = subprocess.run(["realpath", "-m", "/a/../b"], capture_output=True, text=True) if shutil.which("realpath") else None
        if not r or r.returncode != 0:
            self.skipTest("no GNU realpath -m")
        r = subprocess.run([sys.executable, os.path.join(self.out, "linux", "test_install.py")], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_the_app_keeps_its_executable_and_ships_the_engine_cli(self):
        # Mac volumes ignore case: copying bashcutpro beside BashCutPro replaced the app executable (#187)
        generate.generate(ROOT, self.out)
        lines = [l for l in self.read("macos/package.sh").splitlines() if l.startswith(("brand_lc=", "APP_BINS="))]
        self.assertEqual(len(lines), 2)
        bash = shutil.which("bash")
        if not bash:
            return
        for brand, left_out in (("BashCutPro", "bashcutpro"), ("Other", None)):
            r = subprocess.run([bash, "-c", "\n".join(lines) + '\necho $APP_BINS'], env={"BRAND_NAME": brand, "PATH": os.environ["PATH"]}, capture_output=True, text=True)
            bins = r.stdout.split()
            self.assertNotIn("filmcraft-cli", bins, brand)
            self.assertIn("bcpro", bins, brand)
            if left_out:
                self.assertNotIn(left_out, bins)
            else:
                self.assertIn("bashcutpro", bins)

    @unittest.skip("test ticket: bcpro is a symlink to bashcutpro in the Linux stage, nfpm (type: symlink), install.sh, both Flatpak manifests and the macOS CLI zip, a real file in the app and on Windows; docs in usr/share/doc/bashcutpro; the metainfo's first <p> is a sentence of 80+ chars")
    def test_bcpro_is_a_link_and_linux_docs_and_metainfo_are_ours(self):
        pass

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


if __name__ == "__main__":
    unittest.main()
