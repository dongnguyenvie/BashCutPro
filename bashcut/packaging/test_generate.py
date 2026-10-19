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
        self.assertIn("bashcut-mcp.exe", self.read("windows/filmcraft.wxs"))
        self.assertIn("/usr/bin/bashcut-doctor", self.read("linux/nfpm.yaml"))
        self.assertIn("BASHCUT_ROOT", self.read("env.sh"))
        # upstream's own files are untouched
        with open(os.path.join(ROOT, "packaging", "env.sh"), encoding="utf-8") as fh:
            self.assertNotIn("BASHCUT_ROOT:?", fh.read())

    def test_copies_parse(self):
        generate.generate(ROOT, self.out)
        xml.dom.minidom.parse(os.path.join(self.out, "windows", "filmcraft.wxs"))
        bash = shutil.which("bash")
        if bash:
            for rel in ("env.sh", "macos/package.sh", "linux/package.sh", "linux/install.sh", "freebsd/package.sh"):
                r = subprocess.run([bash, "-n", os.path.join(self.out, *rel.split("/"))], capture_output=True, text=True)
                self.assertEqual(r.returncode, 0, rel + ": " + r.stderr)

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
