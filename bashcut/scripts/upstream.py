#!/usr/bin/env python3
"""Upstream-sync tooling for BashCutPro (issue #38; rules in bashcut/extending-upstream.md).

  upstream.py footprint [--pr | --base REV] [--worktree] [-v]
      Every hunk we carry in upstream files (our own files and new files left out), lines per
      file, files upstream changed most in the last 30 days flagged HOT. Fails when a hunk is not
      allowed by UPSTREAM.md (a "Hooks" row or a "Carried edits" row), when a Hooks row no longer
      matches, or when the Rust lines in upstream files go over the budget (150).
      Base: the last upstream merge (merge-base of HEAD and upstream/main). --pr limits the check to
      the upstream files this branch changes (since its merge-base with origin/main); run it before
      every PR.
  upstream.py merge [REF] [--no-check] [--no-fetch] [--branch NAME]
      Turn on git rerere, merge REF (default: newest upstream tag) on upstream-sync/<date>, take
      upstream's Cargo.lock, then cargo check --workspace, the bashcut-* tests and xtask layers.
      Prints the UPSTREAM.md sync-log row.
  upstream.py setup
      Remote `upstream` (fetch only, push disabled) and rerere for this clone.

Stdlib only (Python 3.9+). bashcut/scripts/upstream.sh is the shell entry point.
"""

import argparse
import datetime
import fnmatch
import os
import re
import subprocess
import sys

UPSTREAM_URL = "https://github.com/storytold/filmcraft.git"
UPSTREAM_REF = "upstream/main"
ORIGIN_REF = "origin/main"
MARKER = "BashCutPro"
RUST_BUDGET = 150
HOT_COMMITS = 20
HOT_DAYS = 30
# Generated files: on a sync take upstream's and regenerate (cargo adds our crates back).
GENERATED = ("Cargo.lock",)


class Fail(Exception):
    pass


def git(*args, check=True, cwd=None):
    # git prints UTF-8; decode it as such everywhere (Windows would use the ANSI code page and fail on diffs)
    p = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if check and p.returncode != 0:
        raise Fail("git %s: %s" % (" ".join(args), p.stderr.strip()))
    return p.stdout


def git_ok(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True, encoding="utf-8", errors="replace").returncode == 0


def is_ours(path):
    """Files that are ours by location (bashcut/extending-upstream.md §1 rule 2): any path part named
    `bashcut`, `bashcut_*`, `bashcut-*` or `bashcut.*` (`bashcut/`, `crates/bashcut-*`, `bashcut_ui/`,
    `bashcut_brand.rs`, `xtask/src/bashcut.rs`), and this ledger."""
    if path == "UPSTREAM.md":
        return True
    return any(p == "bashcut" or p.startswith(("bashcut_", "bashcut-", "bashcut.")) for p in path.split("/"))


# ---------------------------------------------------------------- UPSTREAM.md ledger


def split_row(line):
    cells, cur, i = [], "", 0
    s = line.strip()
    if s.startswith("|"):
        s = s[1:]
    if s.endswith("|") and not s.endswith("\\|"):
        s = s[:-1]
    while i < len(s):
        if s[i] == "\\" and i + 1 < len(s) and s[i + 1] == "|":
            cur += "|"
            i += 2
            continue
        if s[i] == "|":
            cells.append(cur.strip())
            cur = ""
        else:
            cur += s[i]
        i += 1
    cells.append(cur.strip())
    return cells


def code(cell):
    """`text` -> text (one code span; the whole cell otherwise)."""
    m = re.fullmatch(r"(`+)\s?(.*?)\s?\1", cell)
    return m.group(2) if m else cell


def read_table(text, heading):
    """Rows (dicts keyed by lower-case header) of the first table under `## <heading>`."""
    lines = text.splitlines()
    try:
        start = next(i for i, l in enumerate(lines) if l.strip().lower() == "## " + heading.lower())
    except StopIteration:
        return None
    rows, header = [], None
    for line in lines[start + 1:]:
        if line.startswith("## "):
            break
        if not line.strip().startswith("|"):
            if header is not None and not line.strip():
                break
            continue
        cells = split_row(line)
        if header is None:
            header = [c.lower() for c in cells]
        elif all(set(c) <= set("-: ") for c in cells):
            continue
        else:
            rows.append(dict(zip(header, cells)))
    return rows


class Hook:
    def __init__(self, path, lines, first, why):
        self.path, self.lines, self.first, self.why = path, lines, first, why
        self.used = 0


class Ledger:
    def __init__(self, text):
        self.hooks, self.carried, self.errors = [], [], []
        hooks = read_table(text, "Hooks")
        carried = read_table(text, "Carried edits")
        if hooks is None:
            self.errors.append("UPSTREAM.md has no '## Hooks' table")
        if carried is None:
            self.errors.append("UPSTREAM.md has no '## Carried edits' table")
        for r in hooks or []:
            path, first = code(r.get("file", "")), code(r.get("first line", ""))
            try:
                n = int(r.get("lines", "1") or "1")
            except ValueError:
                n = 0
            if not path or not first or n < 1:
                self.errors.append("bad Hooks row: %s" % r)
                continue
            self.hooks.append(Hook(path, n, first.strip(), r.get("why", "")))
        for r in carried or []:
            globs = re.findall(r"`([^`]+)`", r.get("files", ""))
            kind = r.get("kind", "").strip().lower()
            if not globs or kind not in ("additions", "edits", "deletes"):
                self.errors.append("bad Carried edits row: %s" % r)
                continue
            for g in globs:
                self.carried.append((g, kind, r.get("why", "")))

    def carried_kind(self, path):
        # a `deletes` row wins over a broader `edits` glob that also matches the file
        kinds = [kind for g, kind, _ in self.carried if fnmatch.fnmatchcase(path, g)]
        if "deletes" in kinds:
            return "deletes"
        return kinds[0] if kinds else None


# ---------------------------------------------------------------- diff


class FileDiff:
    def __init__(self, path):
        self.path, self.status, self.binary = path, "M", False
        self.hunks = []  # (header, [removed lines], [[added lines]])

    @property
    def added(self):
        return sum(len(r) for _, _, runs in self.hunks for r in runs)

    @property
    def removed(self):
        return sum(len(rm) for _, rm, _ in self.hunks)


def parse_diff(text):
    files, cur, hunk = [], None, None
    for line in text.splitlines():
        if line.startswith("diff --git "):
            m = re.match(r"diff --git a/(.*) b/(.*)$", line)
            cur = FileDiff(m.group(2) if m else line[11:])
            files.append(cur)
            hunk = None
        elif cur is None:
            continue
        elif line.startswith("new file mode"):
            cur.status = "A"
        elif line.startswith("deleted file mode"):
            cur.status = "D"
        elif line.startswith("Binary files"):
            cur.binary = True
        elif line.startswith("@@"):
            # With -U0 a hunk is its removed lines, then one run of added lines.
            hunk = (line.split("@@")[1].strip() if line.count("@@") >= 2 else line, [], [[]])
            cur.hunks.append(hunk)
        elif hunk is None:
            continue  # ---/+++ headers come before the first hunk
        elif line.startswith("+"):
            hunk[2][0].append(line[1:])
        elif line.startswith("-"):
            hunk[1].append(line[1:])
    return files


# ---------------------------------------------------------------- footprint


def last_upstream_merge(upstream_ref):
    base = git("merge-base", "HEAD", upstream_ref, check=False).strip()
    if base:
        return base
    # No upstream remote here: the newest row of UPSTREAM.md's sync log.
    try:
        with open("UPSTREAM.md", encoding="utf-8") as fh:
            rows = read_table(fh.read(), "Sync log") or []
    except OSError:
        rows = []
    for r in rows:
        sha = code(r.get("upstream commit", ""))
        if re.fullmatch(r"[0-9a-f]{7,40}", sha) and git_ok("cat-file", "-e", sha + "^{commit}"):
            return sha
    raise Fail("no upstream base: add the `upstream` remote (upstream.py setup) or pass --base")


def hot_counts(upstream_ref, paths):
    if not paths or not git_ok("rev-parse", "--verify", "-q", upstream_ref):
        return None
    tip = git("log", "-1", "--format=%cI", upstream_ref).strip()
    try:
        since = datetime.datetime.fromisoformat(tip) - datetime.timedelta(days=HOT_DAYS)
    except ValueError:
        return None
    out = git("log", "--since=" + since.isoformat(), "--format=", "--name-only", upstream_ref, "--", *paths)
    counts = {}
    for p in out.splitlines():
        if p.strip():
            counts[p] = counts.get(p, 0) + 1
    return counts


def classify(f, ledger, problems):
    """Kind of carried change for an upstream file; appends rule breaks to `problems`."""
    kind = ledger.carried_kind(f.path)
    if kind == "deletes":
        if f.status == "D":
            return "deleted"
        problems.append("%s: listed as deleted in UPSTREAM.md but still present" % f.path)
        return "VIOLATION"
    if kind == "edits":
        # a hook may also sit in a carried-edit file: count it so its row is not reported as stale
        for _, _, runs in f.hunks:
            for run in runs:
                for line in run:
                    hook = next((h for h in ledger.hooks if h.path == f.path and h.first == line.strip()), None)
                    if hook is not None:
                        hook.used += 1
        return "carried edit"
    if f.status == "D":
        problems.append("%s: deletes an upstream file" % f.path)
        return "VIOLATION"
    if f.binary:
        problems.append("%s: changes an upstream binary file" % f.path)
        return "VIOLATION"
    bad = False
    for header, removed, _ in f.hunks:
        for line in removed:
            problems.append("%s @@ %s: changes or removes upstream line: %s" % (f.path, header, line.strip()))
            bad = True
    if kind == "additions":
        return "VIOLATION" if bad else "additions"
    hooks = [h for h in ledger.hooks if h.path == f.path]
    nhooks = 0
    for header, _, runs in f.hunks:
        for run in runs:
            i = 0
            while i < len(run):
                text = run[i].strip()
                hook = next((h for h in hooks if h.first == text and not h.used), None)
                if hook is None:
                    hook = next((h for h in hooks if h.first == text), None)
                if hook is None:
                    problems.append("%s @@ %s: line not in the UPSTREAM.md Hooks table: %s" % (f.path, header, text))
                    bad = True
                    i += 1
                    continue
                hook.used += 1
                nhooks += 1
                i += hook.lines
    if bad:
        return "VIOLATION"
    return "hook" if nhooks == 1 else "hooks (%d)" % nhooks


def footprint(args):
    with open("UPSTREAM.md", encoding="utf-8") as fh:
        ledger = Ledger(fh.read())
    head = [] if args.worktree else ["HEAD"]
    touched = None  # --pr: the upstream files this branch changes
    if args.base:
        base = git("rev-parse", args.base).strip()
        what = args.base
    else:
        base = last_upstream_merge(args.upstream)
        what = "last upstream merge"
        if args.pr:
            fork = git("merge-base", "HEAD", args.origin).strip()
            touched = set(git("diff", "--name-only", "--no-renames", fork, *head, "--").split("\n")) - {""}
            what += "; files this branch changes since %s" % args.origin
    full = not args.base  # every hook row in scope must be found
    diff = git("diff", "--no-color", "--no-renames", "--no-ext-diff", "-U0", base, *head, "--")
    files = parse_diff(diff)
    if touched is not None:
        files = [f for f in files if f.path in touched]
        ledger.hooks = [h for h in ledger.hooks if h.path in touched]

    problems = list(ledger.errors)
    generated = [f.path for f in files if f.path in GENERATED]
    files = [f for f in files if f.path not in GENERATED]
    upstream_files = [f for f in files if not is_ours(f.path) and f.status != "A"]
    new_files = [f.path for f in files if not is_ours(f.path) and f.status == "A"]
    hot = hot_counts(args.upstream, [f.path for f in upstream_files])

    rows, rust, plus, minus = [], 0, 0, 0
    for f in upstream_files:
        kind = classify(f, ledger, problems)
        h = hot.get(f.path, 0) if hot is not None else None
        rows.append((f, kind, h))
        plus += f.added
        minus += f.removed
        if f.path.endswith(".rs"):
            rust += f.added + f.removed

    if touched is None:
        # a merge can bring a deleted file back unchanged, so it shows in no diff: look at the tree
        tree = git("ls-files") if args.worktree else git("ls-tree", "-r", "--name-only", "HEAD")
        seen = {f.path for f, _, _ in rows}
        for path in sorted(set(tree.splitlines()) - seen - {""}):
            if ledger.carried_kind(path) == "deletes":
                problems.append("%s: listed as deleted in UPSTREAM.md but still present" % path)

    for hook in ledger.hooks:
        if hook.first.find(MARKER) < 0:
            problems.append("%s: hook line has no '%s' marker: %s" % (hook.path, MARKER, hook.first))
        if full and hook.used == 0:
            problems.append("%s: Hooks row not found in the diff (stale?): %s" % (hook.path, hook.first))
        if hook.used > 1:
            problems.append("%s: Hooks row matches %d places (one row per hook): %s" % (hook.path, hook.used, hook.first))
    if rust > args.budget:
        problems.append("Rust lines in upstream files: %d, over the budget of %d" % (rust, args.budget))

    short = git("rev-parse", "--short", base).strip()
    print("Footprint %s..%s (base: %s)" % (short, "worktree" if args.worktree else "HEAD", what))
    if rows:
        width = max(len(f.path) for f, _, _ in rows)
        print("  %-*s  st  %5s %5s  %s  %s" % (width, "upstream file", "+", "-", "30d", "kind"))
        for f, kind, h in rows:
            flag = "-" if h is None else ("%3d%s" % (h, " HOT" if h >= args.hot else "    "))
            print("  %-*s  %s   %5d %5d  %s  %s" % (width, f.path, f.status, f.added, f.removed, flag, kind))
            if args.verbose:
                for header, removed, runs in f.hunks:
                    print("      @@ %s" % header)
                    for line in removed:
                        print("      -%s" % line)
                    for run in runs:
                        for line in run:
                            print("      +%s" % line)
    else:
        print("  (no upstream file changed)")
    print("Upstream files: %d, +%d -%d; Rust lines %d / %d" % (len(rows), plus, minus, rust, args.budget))
    if hot is None:
        print("30-day churn: unknown (no %s; run upstream.py setup)" % args.upstream)
    if new_files:
        print("New files outside our folders (no conflict risk): %s" % ", ".join(new_files))
    if generated:
        print("Generated (take upstream's on a sync, then regenerate): %s" % ", ".join(generated))
    print("Ledger: %d hook rows, %d carried-edit globs" % (len(ledger.hooks), len(ledger.carried)))
    if problems:
        print("\nFAIL: %d problem(s)" % len(problems))
        for p in problems:
            print("  - " + p)
        print("Fix the change, or add the hook to UPSTREAM.md (bashcut/extending-upstream.md §1).")
        return 1
    print("OK")
    return 0


# ---------------------------------------------------------------- merge / setup


def setup(_args=None):
    remotes = git("remote").split()
    if "upstream" not in remotes:
        git("remote", "add", "upstream", UPSTREAM_URL)
    git("remote", "set-url", "--push", "upstream", "DISABLED")
    git("config", "rerere.enabled", "true")
    git("config", "rerere.autoupdate", "true")
    print("upstream: %s (push disabled); rerere on" % git("remote", "get-url", "upstream").strip())
    return 0


def run(cmd):
    print("$ " + " ".join(cmd), flush=True)
    return subprocess.run(cmd).returncode


def our_packages():
    pkgs = []
    for top in ("crates", "apps"):
        if not os.path.isdir(top):
            continue
        for d in sorted(os.listdir(top)):
            manifest = os.path.join(top, d, "Cargo.toml")
            if d.startswith("bashcut-") and os.path.isfile(manifest):
                with open(manifest, encoding="utf-8") as fh:
                    m = re.search(r'^name\s*=\s*"([^"]+)"', fh.read(), re.M)
                if m:
                    pkgs.append(m.group(1))
    return pkgs


def merge(args):
    if git("status", "--porcelain", "--untracked-files=no").strip():
        raise Fail("working tree has changes; commit or stash them first")
    git("config", "rerere.enabled", "true")
    git("config", "rerere.autoupdate", "true")
    if not args.no_fetch:
        git("fetch", "upstream", "--tags")
    ref = args.ref or git("describe", "--tags", "--abbrev=0", args.upstream, check=False).strip() or args.upstream
    branch = args.branch or "upstream-sync/" + datetime.date.today().isoformat()
    current = git("rev-parse", "--abbrev-ref", "HEAD").strip()
    if current != branch:
        if git_ok("rev-parse", "--verify", "-q", "refs/heads/" + branch):
            raise Fail("branch %s exists; switch to it or pass --branch" % branch)
        git("switch", "-c", branch)
    print("Merging %s into %s (rerere on)" % (ref, branch), flush=True)
    before = git("rev-parse", "HEAD").strip()
    if run(["git", "merge", "--no-ff", "-m", "Merge upstream %s" % ref, ref]) != 0:
        unmerged = git("diff", "--name-only", "--diff-filter=U").split()
        if "Cargo.lock" in unmerged:
            git("checkout", "--theirs", "--", "Cargo.lock")
            git("add", "Cargo.lock")
            unmerged.remove("Cargo.lock")
        if unmerged:
            print("\nConflicts left (search for '// %s' to re-place a hook):" % MARKER)
            for p in unmerged:
                print("  " + p)
            print("Resolve, `git add`, `git commit` (rerere records it), then re-run with --no-fetch.")
            return 1
        staged = git("diff", "--cached", "--no-color", "-U0")
        if re.search(r"^\+(<<<<<<<|>>>>>>>) ", staged, re.M):
            print("conflict markers left in the index; resolve by hand")
            return 1
        git("commit", "--no-edit")
        print("All conflicts resolved by rerere / upstream's Cargo.lock.")
    if git("rev-parse", "HEAD").strip() == before:
        print("\nAlready up to date with %s: nothing merged, no sync-log row (record the check if you like)." % ref)
        return footprint(argparse.Namespace(pr=False, base=None, worktree=False, verbose=False, upstream=args.upstream,
                                            origin=ORIGIN_REF, budget=RUST_BUDGET, hot=HOT_COMMITS))
    status = 0
    if not args.no_check:
        cmds = [["cargo", "check", "--workspace"]]
        pkgs = our_packages()
        if pkgs:
            cmds.append(["cargo", "test", *[a for p in pkgs for a in ("-p", p)]])
        cmds.append(["cargo", "xtask", "layers"])
        for c in cmds:
            if run(c) != 0:
                status = 1
                break
    sha, date = git("log", "-1", "--format=%H %cs", ref + "^{commit}").split()
    print("\nUPSTREAM.md sync-log row:")
    print("| %s | `%s` | %s | Merge %s |" % (datetime.date.today().isoformat(), sha, date, ref))
    print()
    fp = footprint(argparse.Namespace(pr=False, base=None, worktree=False, verbose=False, upstream=args.upstream,
                                      origin=ORIGIN_REF, budget=RUST_BUDGET, hot=HOT_COMMITS))
    return status or fp


def main(argv=None):
    for stream in (sys.stdout, sys.stderr):  # Windows consoles default to cp1252; file names and hunks can be non-ASCII
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(prog="upstream.sh", description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd")
    fp = sub.add_parser("footprint", help="hunks we carry in upstream files, checked against UPSTREAM.md")
    fp.add_argument("--pr", action="store_true", help="only the upstream files this branch changes (since its merge-base with origin/main)")
    fp.add_argument("--base", help="diff from this revision instead")
    fp.add_argument("--worktree", action="store_true", help="include uncommitted changes")
    fp.add_argument("-v", "--verbose", action="store_true", help="print every hunk")
    fp.add_argument("--upstream", default=UPSTREAM_REF)
    fp.add_argument("--origin", default=ORIGIN_REF)
    fp.add_argument("--budget", type=int, default=RUST_BUDGET, help="max Rust lines changed in upstream files")
    fp.add_argument("--hot", type=int, default=HOT_COMMITS, help="upstream commits in 30 days that make a file HOT")
    mg = sub.add_parser("merge", help="merge an upstream tag with rerere, then check")
    mg.add_argument("ref", nargs="?", help="tag or commit (default: newest tag on upstream/main)")
    mg.add_argument("--no-check", action="store_true", help="skip cargo check / tests / layers")
    mg.add_argument("--no-fetch", action="store_true")
    mg.add_argument("--branch", help="default: upstream-sync/<date>")
    mg.add_argument("--upstream", default=UPSTREAM_REF)
    sub.add_parser("setup", help="upstream remote (push disabled) and rerere")
    args = ap.parse_args(argv)
    try:
        top = git("rev-parse", "--show-toplevel").strip()
        os.chdir(top)
        if args.cmd == "footprint":
            return footprint(args)
        if args.cmd == "merge":
            return merge(args)
        if args.cmd == "setup":
            return setup(args)
        ap.print_help()
        return 2
    except (Fail, OSError) as e:
        print("upstream.py: %s" % e, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
