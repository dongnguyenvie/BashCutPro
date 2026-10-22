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
            self.assertNotIn("filmcraft-cli", self.read(rel), rel)
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
            for rel in ("env.sh", "macos/package.sh", "linux/package.sh", "linux/install.sh", "freebsd/package.sh"):
                r = subprocess.run([bash, "-n", os.path.join(self.out, *rel.split("/"))], capture_output=True, text=True)
                self.assertEqual(r.returncode, 0, rel + ": " + r.stderr)

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
