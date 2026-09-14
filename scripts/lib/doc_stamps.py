#!/usr/bin/env python3
"""`last_updated` diff ratchet (#1349).

A doc CHANGED in this branch must carry a `last_updated` at or after the merge-base date. That is
what the retired stamper did — `docs-last-updated-pr.yml` pushed `docs: stamp last_updated` commits
to PR branches as a bot until the maintainer removed its trigger and its push body in `5c93ca29` —
without a bot writing to anyone's branch.

**Why a ratchet and not a validator.** `docfront.py` checks `last_updated` against
`^\\d{4}-\\d{2}-\\d{2}$` and nothing else: format, never currency. Measured 2026-09-13, 92 of the 143
stamped docs had their last commit more than a week after their stamp, so the field was decorative.
This makes it mean something for docs people actually touch, and says nothing about the rest.

**Scope, deliberately narrow.** Only docs that ALREADY carry a `last_updated` are checked. A doc
without one is grandfathered exactly as `docfront.py` grandfathers it — otherwise editing
`docs/dev/project/traceability.md`, which has no frontmatter and is touched by nearly every PR,
would demand a frontmatter block before any ledger entry could land, and a check that blocks
everything is a check people route around.

It judges HEAD, not the working tree: what would land, not what is staged. A stamp fixed but not
yet committed will still be reported, which is confusing exactly once.

Usage:  scripts/check-doc-stamps.sh [--base REF]   # REF defaults to origin/main
        scripts/check-doc-stamps.sh --self-test
Exit:   0 clean, 1 a changed doc carries a stale stamp, 2 the check could not run.
"""
import os
import re
import subprocess
import sys
import tempfile

DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
STAMP = re.compile(r"^last_updated:\s*(\S+)\s*$")


def git(repo, *args):
    return subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True, check=False)


def stamp_of(text):
    """The `last_updated` value from a frontmatter block, or None if the doc has no block/field."""
    lines = text.split("\n")
    if not lines or lines[0].strip() != "---":
        return None
    for line in lines[1:]:
        if line.strip() == "---":
            return None
        m = STAMP.match(line)
        if m:
            return m.group(1)
    return None


def changed_docs(repo, base, head):
    """Markdown files under docs/ added or modified in base..head (deletions excluded)."""
    r = git(repo, "diff", "--name-only", "--diff-filter=AMR", f"{base}...{head}", "--", "docs/**/*.md", "docs/*.md")
    if r.returncode != 0:
        raise RuntimeError(f"git diff failed: {r.stderr.strip()}")
    return [p for p in r.stdout.split() if p.endswith(".md")]


def check(repo, base, head):
    mb = git(repo, "merge-base", base, head)
    if mb.returncode != 0:
        raise RuntimeError(f"no merge base between {base} and {head}")
    mb = mb.stdout.strip()
    d = git(repo, "show", "-s", "--format=%cs", mb)
    if d.returncode != 0:
        raise RuntimeError("could not read the merge-base date")
    floor = d.stdout.strip()
    if not DATE.match(floor):
        raise RuntimeError(f"merge-base date {floor!r} is not YYYY-MM-DD")

    stale, checked, exempt = [], 0, 0
    for rel in changed_docs(repo, base, head):
        blob = git(repo, "show", f"{head}:{rel}")
        if blob.returncode != 0:
            continue
        s = stamp_of(blob.stdout)
        if s is None:
            exempt += 1
            continue
        checked += 1
        if not DATE.match(s):
            stale.append((rel, s, "not YYYY-MM-DD"))
        elif s < floor:
            stale.append((rel, s, f"older than the merge-base date {floor}"))
    return floor, checked, exempt, stale


def run(repo, base, head):
    floor, checked, exempt, stale = check(repo, base, head)
    print(f"doc-stamps: merge-base date {floor}; {checked} stamped doc(s) changed, {exempt} without a stamp (exempt)")
    if stale:
        print(f"DOC-STAMPS: FAIL — {len(stale)} changed doc(s) carry a stamp older than this branch:")
        for rel, s, why in stale:
            print(f"  {rel}: last_updated {s} — {why}")
        print(f"  Fix: set `last_updated: {floor}` (or later) in each, since you changed them.")
        return 1
    print("DOC-STAMPS: PASS")
    return 0


# ---- self-test -----------------------------------------------------------------------------
# Three fixtures. A stale stamp must FAIL, a current one must PASS, and a doc with no stamp must be
# EXEMPT rather than failed — that last one is the whole reason this check does not block the ledger.
def self_test():
    failures = []
    with tempfile.TemporaryDirectory(prefix="doc-stamps-selftest-") as repo:
        env = ["-c", "user.name=t", "-c", "user.email=t@l", "-c", "commit.gpgsign=false"]
        git(repo, "init", "-q")
        os.makedirs(os.path.join(repo, "docs"))
        base_file = os.path.join(repo, "docs", "seed.md")
        with open(base_file, "w") as fh:
            fh.write("seed\n")
        git(repo, "add", "-A")
        git(repo, *env, "-c", "commit.date=2026-09-10T00:00:00", "commit", "-q", "-m", "seed")
        base = git(repo, "rev-parse", "HEAD").stdout.strip()

        def commit_doc(name, body):
            with open(os.path.join(repo, "docs", name), "w") as fh:
                fh.write(body)
            git(repo, "add", "-A")
            git(repo, *env, "commit", "-q", "-m", name)

        fm = lambda d: f"---\nproject: p\ndoc: docs/x.md\nstatus: living\nlast_updated: {d}\n---\n\nbody\n"
        for name, body, want_fail, label in [
            ("stale.md", fm("2026-01-01"), True, "a stale stamp must FAIL"),
            ("current.md", fm("2026-09-30"), False, "a current stamp must PASS"),
            ("nostamp.md", "# no frontmatter\n\nbody\n", False, "a doc with no stamp must be EXEMPT"),
        ]:
            git(repo, "checkout", "-q", base)
            git(repo, "checkout", "-q", "-B", "probe")
            commit_doc(name, body)
            head = git(repo, "rev-parse", "HEAD").stdout.strip()
            rc = 1 if check(repo, base, head)[3] else 0
            ok = (rc == 1) == want_fail
            print(f"  {'ok  ' if ok else 'FAIL'} {label}")
            if not ok:
                failures.append(label)
    if failures:
        print(f"SELF-TEST: FAIL ({len(failures)}): {', '.join(failures)}")
        return 1
    print("SELF-TEST: PASS — stale rejected, current accepted, unstamped exempt")
    return 0


def main(argv):
    if argv[1:2] == ["--self-test"]:
        return self_test()
    repo = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True).stdout.strip()
    base = argv[1] if len(argv) > 1 else "origin/main"
    try:
        return run(repo, base, "HEAD")
    except RuntimeError as e:
        print(f"doc-stamps: {e}", file=sys.stderr)
        print("DOC-STAMPS: FAIL")
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
