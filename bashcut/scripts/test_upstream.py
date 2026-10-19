#!/usr/bin/env python3
"""Tests for upstream.py on throwaway repos (a fake upstream, a fork, a bare origin).

  python3 bashcut/scripts/test_upstream.py
"""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "upstream.py")

LIB_V1 = "mod a;\nmod b;\n\nfn main() {\n    run();\n}\n"
HOOK = "mod bashcut; // BashCutPro: test hook (bashcut/extending-upstream.md)"
LEDGER = """# Upstream

## Sync log

| Date (sync) | Upstream commit | Upstream commit date | Note |
|---|---|---|---|

## Hooks

| File | Lines | First line | Why | Design |
|---|---|---|---|---|
| `src/lib.rs` | 1 | `{hook}` | test | - |

## Carried edits

| Files | Kind | Why | Until |
|---|---|---|---|
| `packaging/**` | edits | branding | test |
| `NOTICE` | additions | copyright | keep |
"""


class Repos(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="bashcut-upstream-")
        home = os.path.join(self.tmp, "home")
        os.mkdir(home)
        self.env = dict(os.environ, HOME=home, GIT_CONFIG_NOSYSTEM="1", GIT_AUTHOR_NAME="t",
                        GIT_AUTHOR_EMAIL="t@example.com", GIT_COMMITTER_NAME="t",
                        GIT_COMMITTER_EMAIL="t@example.com")
        self.env.pop("GIT_DIR", None)
        self.up = os.path.join(self.tmp, "up")
        self.fork = os.path.join(self.tmp, "fork")
        origin = os.path.join(self.tmp, "origin.git")
        os.mkdir(self.up)
        self.git(self.up, "init", "-q", "-b", "main")
        self.write(self.up, "src/lib.rs", LIB_V1)
        self.write(self.up, "NOTICE", "Upstream notice\n")
        self.write(self.up, "packaging/build.sh", "NAME=FilmCraft\n")
        self.commit(self.up, "v1")
        self.git(self.up, "tag", "v1")
        self.git(self.tmp, "clone", "-q", "-o", "upstream", self.up, self.fork)
        self.git(self.tmp, "init", "-q", "--bare", "-b", "main", origin)
        self.git(self.fork, "remote", "add", "origin", origin)
        # The fork: one hook, an addition, a carried edit, our own files.
        self.write(self.fork, "UPSTREAM.md", LEDGER.format(hook=HOOK))
        self.write(self.fork, "src/lib.rs", LIB_V1.replace("mod a;\n", "mod a;\n%s\n" % HOOK))
        self.write(self.fork, "NOTICE", "Our notice\nUpstream notice\n")
        self.write(self.fork, "packaging/build.sh", "NAME=BashCutPro\n")
        self.write(self.fork, "bashcut/notes.md", "ours\n")
        self.write(self.fork, "crates/bashcut-x/src/lib.rs", "pub fn x() {}\n")
        self.write(self.fork, "src/bashcut_extra.rs", "pub fn y() {}\n")
        self.write(self.fork, "src/bashcut_ui/mod.rs", "pub fn z() {}\n")
        self.write(self.fork, "xtask/src/bashcut.rs", "pub fn w() {}\n")
        self.write(self.fork, ".github/README.md", "new file\n")
        self.commit(self.fork, "fork")
        self.git(self.fork, "push", "-q", "origin", "main")
        self.git(self.fork, "fetch", "-q", "origin")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def git(self, cwd, *args):
        p = subprocess.run(["git", *args], cwd=cwd, env=self.env, capture_output=True, text=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        return p.stdout

    def write(self, repo, path, text):
        full = os.path.join(repo, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w", encoding="utf-8") as fh:
            fh.write(text)

    def read(self, repo, path):
        with open(os.path.join(repo, path), encoding="utf-8") as fh:
            return fh.read()

    def commit(self, repo, msg):
        self.git(repo, "add", "-A")
        self.git(repo, "commit", "-q", "-m", msg)

    def tool(self, *args):
        p = subprocess.run([sys.executable, SCRIPT, *args], cwd=self.fork, env=self.env,
                           capture_output=True, text=True)
        return p.returncode, p.stdout + p.stderr


class Footprint(Repos):
    def test_fork_with_ledgered_changes_passes(self):
        rc, out = self.tool("footprint", "-v")
        self.assertEqual(rc, 0, out)
        self.assertIn("src/lib.rs", out)
        self.assertIn("hook", out)
        self.assertIn("additions", out)
        self.assertIn("carried edit", out)
        self.assertIn("New files outside our folders (no conflict risk): .github/README.md", out)
        self.assertNotIn("bashcut/notes.md", out)
        self.assertNotIn("crates/bashcut-x", out)
        self.assertNotIn("bashcut_extra", out)
        self.assertNotIn("bashcut_ui", out)
        self.assertNotIn("xtask", out)

    def test_cargo_lock_is_generated_not_checked(self):
        self.write(self.fork, "Cargo.lock", "upstream\n")
        self.commit(self.fork, "lock")
        self.git(self.fork, "push", "-q", "origin", "main")
        self.git(self.fork, "fetch", "-q", "origin")
        self.write(self.fork, "Cargo.lock", "upstream\nours\n")
        self.commit(self.fork, "our crate")
        rc, out = self.tool("footprint", "--pr")
        self.assertEqual(rc, 0, out)
        self.assertIn("Generated (take upstream's on a sync, then regenerate): Cargo.lock", out)

    def test_line_without_hook_row_fails(self):
        self.write(self.fork, "src/lib.rs", self.read(self.fork, "src/lib.rs") + "fn extra() {}\n")
        self.commit(self.fork, "extra")
        rc, out = self.tool("footprint")
        self.assertEqual(rc, 1, out)
        self.assertIn("line not in the UPSTREAM.md Hooks table: fn extra() {}", out)

    def test_changed_upstream_line_fails(self):
        self.write(self.fork, "src/lib.rs", self.read(self.fork, "src/lib.rs").replace("run();", "run2();"))
        self.commit(self.fork, "edit")
        rc, out = self.tool("footprint")
        self.assertEqual(rc, 1, out)
        self.assertIn("changes or removes upstream line: run();", out)

    def test_removal_in_additions_file_fails(self):
        self.write(self.fork, "NOTICE", "Our notice\n")
        self.commit(self.fork, "drop")
        rc, out = self.tool("footprint")
        self.assertEqual(rc, 1, out)
        self.assertIn("NOTICE", out)
        self.assertIn("changes or removes upstream line: Upstream notice", out)

    def test_deleted_upstream_file_fails(self):
        os.remove(os.path.join(self.fork, "NOTICE"))
        self.commit(self.fork, "rm")
        rc, out = self.tool("footprint")
        self.assertEqual(rc, 1, out)
        self.assertIn("NOTICE: deletes an upstream file", out)

    def test_stale_and_unmarked_rows_fail(self):
        ledger = LEDGER.format(hook=HOOK) + ""
        ledger = ledger.replace("| `src/lib.rs` | 1 |", "| `src/lib.rs` | 1 | `mod gone;` | x | - |\n| `src/lib.rs` | 1 |")
        self.write(self.fork, "UPSTREAM.md", ledger)
        self.commit(self.fork, "stale")
        rc, out = self.tool("footprint")
        self.assertEqual(rc, 1, out)
        self.assertIn("Hooks row not found in the diff (stale?): mod gone;", out)
        self.assertIn("hook line has no 'BashCutPro' marker: mod gone;", out)

    def test_multi_line_hook(self):
        block = ["#[cfg(not(test))] // BashCutPro: two lines (x)", "use crate::bashcut_extra;"]
        self.write(self.fork, "src/lib.rs", self.read(self.fork, "src/lib.rs").replace("mod b;\n", "mod b;\n%s\n" % "\n".join(block)))
        self.write(self.fork, "UPSTREAM.md", self.read(self.fork, "UPSTREAM.md").replace(
            "| test | - |\n", "| test | - |\n| `src/lib.rs` | 2 | `%s` | test | - |\n" % block[0], 1))
        self.commit(self.fork, "block")
        rc, out = self.tool("footprint")
        self.assertEqual(rc, 0, out)
        self.assertIn("hooks (2)", out)

    def test_rust_budget(self):
        rc, out = self.tool("footprint", "--budget", "0")
        self.assertEqual(rc, 1, out)
        self.assertIn("over the budget of 0", out)

    def test_pr_mode_checks_only_the_branch(self):
        self.git(self.fork, "switch", "-q", "-c", "feat")
        rc, out = self.tool("footprint", "--pr")
        self.assertEqual(rc, 0, out)
        self.assertIn("no upstream file changed", out)
        self.write(self.fork, "src/lib.rs", self.read(self.fork, "src/lib.rs") + "fn extra() {}\n")
        self.commit(self.fork, "extra")
        rc, out = self.tool("footprint", "--pr")
        self.assertEqual(rc, 1, out)
        self.assertIn("fn extra() {}", out)

    def test_pr_mode_may_change_our_own_hook_line(self):
        self.git(self.fork, "switch", "-q", "-c", "feat")
        new_hook = HOOK.replace("test hook", "renamed hook")
        self.write(self.fork, "src/lib.rs", self.read(self.fork, "src/lib.rs").replace(HOOK, new_hook))
        self.write(self.fork, "UPSTREAM.md", self.read(self.fork, "UPSTREAM.md").replace(HOOK, new_hook))
        self.commit(self.fork, "rename hook")
        rc, out = self.tool("footprint", "--pr")
        self.assertEqual(rc, 0, out)

    def test_pr_mode_may_restore_upstream_lines(self):
        # Turning a carried edit back into upstream's text (and dropping its row) is not a new edit.
        self.git(self.fork, "switch", "-q", "-c", "feat")
        self.write(self.fork, "packaging/build.sh", "NAME=FilmCraft\n")
        self.write(self.fork, "UPSTREAM.md", self.read(self.fork, "UPSTREAM.md").replace("| `packaging/**` | edits | branding | test |\n", ""))
        self.commit(self.fork, "restore")
        rc, out = self.tool("footprint", "--pr")
        self.assertEqual(rc, 0, out)
        self.assertEqual(self.tool("footprint")[0], 0)

    def test_pr_mode_sees_old_violations_in_touched_files(self):
        # A branch touching a file is answerable for everything we carry in it.
        self.write(self.fork, "src/lib.rs", self.read(self.fork, "src/lib.rs") + "fn old() {}\n")
        self.commit(self.fork, "old violation")
        self.git(self.fork, "push", "-q", "origin", "main")
        self.git(self.fork, "fetch", "-q", "origin")
        self.git(self.fork, "switch", "-q", "-c", "feat")
        self.write(self.fork, "src/lib.rs", self.read(self.fork, "src/lib.rs").replace(HOOK, HOOK + " "))
        self.commit(self.fork, "touch")
        rc, out = self.tool("footprint", "--pr")
        self.assertEqual(rc, 1, out)
        self.assertIn("fn old() {}", out)

    def test_worktree_includes_uncommitted(self):
        self.write(self.fork, "src/lib.rs", self.read(self.fork, "src/lib.rs") + "fn extra() {}\n")
        self.assertEqual(self.tool("footprint")[0], 0)
        self.assertEqual(self.tool("footprint", "--worktree")[0], 1)


class Merge(Repos):
    def test_rerere_replays_a_recorded_resolution(self):
        # Upstream inserts a line where our hook sits: a conflict on every sync until rerere learns it.
        self.write(self.up, "src/lib.rs", LIB_V1.replace("mod a;\n", "mod a;\nmod c;\n"))
        self.commit(self.up, "v2")
        self.git(self.up, "tag", "v2")

        rc, out = self.tool("merge", "v2", "--no-check", "--branch", "sync-1")
        self.assertEqual(rc, 1, out)
        self.assertIn("src/lib.rs", out)
        self.assertEqual(self.git(self.fork, "config", "rerere.enabled").strip(), "true")
        resolved = LIB_V1.replace("mod a;\n", "mod a;\nmod c;\n%s\n" % HOOK)
        self.write(self.fork, "src/lib.rs", resolved)
        self.git(self.fork, "add", "src/lib.rs")
        self.git(self.fork, "commit", "-q", "--no-edit")

        # A second sync from the same point (dry run): rerere resolves it, the tool commits and checks.
        self.git(self.fork, "switch", "-q", "main")
        rc, out = self.tool("merge", "v2", "--no-check", "--no-fetch", "--branch", "sync-2")
        self.assertEqual(rc, 0, out)
        self.assertIn("resolved by rerere", out)
        self.assertEqual(self.read(self.fork, "src/lib.rs"), resolved)
        self.assertEqual(self.git(self.fork, "rev-parse", "--abbrev-ref", "HEAD").strip(), "sync-2")
        self.assertEqual(self.git(self.fork, "status", "--porcelain", "--untracked-files=no"), "")
        self.assertIn("| `%s` |" % self.git(self.up, "rev-parse", "v2").strip(), out)
        self.assertIn("Merge v2 |", out)

    def test_already_up_to_date_prints_no_sync_row(self):
        rc, out = self.tool("merge", "v1", "--no-check", "--no-fetch", "--branch", "sync-0")
        self.assertEqual(rc, 0, out)
        self.assertIn("Already up to date with v1", out)
        self.assertNotIn("sync-log row:", out)

    def test_refuses_a_dirty_tree(self):
        self.write(self.fork, "src/lib.rs", "dirty\n")
        rc, out = self.tool("merge", "v1", "--no-check", "--no-fetch")
        self.assertEqual(rc, 2, out)
        self.assertIn("working tree has changes", out)

    def test_default_ref_is_the_newest_upstream_tag(self):
        self.write(self.up, "README", "x\n")
        self.commit(self.up, "v3")
        self.git(self.up, "tag", "v3")
        self.write(self.up, "README", "y\n")
        self.commit(self.up, "after v3")
        rc, out = self.tool("merge", "--no-check", "--branch", "sync-3")
        self.assertEqual(rc, 0, out)
        self.assertIn("Merging v3", out)
        self.assertEqual(self.read(self.fork, "README"), "x\n")


if __name__ == "__main__":
    unittest.main()
