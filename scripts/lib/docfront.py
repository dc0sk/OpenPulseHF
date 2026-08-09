#!/usr/bin/env python3
"""Doc frontmatter validator + the anti-rot constitution.

Two jobs:
  1. Frontmatter hygiene, RECURSIVELY over docs/ (the old check saw only docs/*.md — 20 of ~180).
  2. The constitution: `status: living` claims a doc reflects the PRESENT state and is kept current.
     A `living` header on a doc no machine maintains is worse than a stale doc — it suppresses
     suspicion while it rots (this is how the 102 KB matrix rotted under a `living` header). So
     `living` is legal only for a doc named in docs/.living-manifest.txt (generated/checked docs).
     Historical prose uses a non-living status: review | resolved | archive | draft.

Grandfathered like every other ratchet: `--baseline` records today's offenders and today's living
docs; `check` fails only on NEW offenses. Shrink the baseline and the manifest over time.

    docfront.py check       # fail on NEW frontmatter offenses / illegal new `living` docs
    docfront.py baseline    # (re)write .frontmatter-baseline.txt and seed .living-manifest.txt
"""
from __future__ import annotations
import sys, os, re, glob, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
DOCS = ROOT / "docs"
MANIFEST = DOCS / ".living-manifest.txt"
BASELINE = DOCS / ".frontmatter-baseline.txt"
ALLOWED_STATUS = {"living", "review", "resolved", "archive", "draft"}
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def _docs():
    out = []
    for p in glob.glob(str(DOCS / "**/*.md"), recursive=True):
        rel = os.path.relpath(p, ROOT)
        if "/test-reports/" in rel:      # generated data dumps, not authored prose
            continue
        if rel.endswith(".generated.md"):  # generated artifacts carry a banner, not frontmatter
            continue
        out.append(rel)
    return sorted(out)


def _frontmatter(rel):
    """Return dict of frontmatter fields, or None if there is no opening --- block."""
    lines = (ROOT / rel).read_text(encoding="utf-8", errors="ignore").splitlines()
    if not lines or lines[0].strip() != "---":
        return None
    fm = {}
    for line in lines[1:]:
        if line.strip() == "---":
            return fm
        m = re.match(r"([a-z_]+):\s*(.*)$", line)
        if m:
            fm[m.group(1)] = m.group(2).strip()
    return None  # unterminated block


def _offenses(manifest):
    offenses = []  # "rel\treason"
    living = []
    for rel in _docs():
        fm = _frontmatter(rel)
        if fm is None:
            offenses.append(f"{rel}\tmissing or unterminated frontmatter block")
            continue
        if fm.get("project") != "openpulsehf":
            offenses.append(f"{rel}\tproject must be 'openpulsehf'")
        if fm.get("doc") != rel:
            offenses.append(f"{rel}\tdoc must equal the file path")
        st = fm.get("status", "")
        if st not in ALLOWED_STATUS:
            offenses.append(f"{rel}\tstatus '{st}' not in {sorted(ALLOWED_STATUS)}")
        elif st == "living" and rel not in manifest:
            offenses.append(f"{rel}\tstatus: living but not in .living-manifest.txt "
                            f"(a 'living' doc must be machine-maintained, or use review/resolved/archive)")
        if not DATE.match(fm.get("last_updated", "")):
            offenses.append(f"{rel}\tlast_updated must be YYYY-MM-DD")
        if st == "living":
            living.append(rel)
    return offenses, living


def _load(path):
    if not path.exists():
        return set()
    return {l.strip() for l in path.read_text().splitlines() if l.strip() and not l.startswith("#")}


def baseline():
    manifest = _load(MANIFEST)
    offenses, living = _offenses(manifest)
    # seed the manifest with today's living docs so existing ones are grandfathered
    MANIFEST.write_text(
        "# Docs allowed to carry `status: living` — each MUST be kept current by a regenerator or\n"
        "# checker. Historical prose uses review/resolved/archive instead. Shrink this list.\n"
        + "\n".join(sorted(set(living) | manifest)) + "\n", encoding="utf-8")
    manifest = _load(MANIFEST)
    offenses, _ = _offenses(manifest)   # recompute now that manifest is seeded
    BASELINE.write_text(
        "# Frontmatter offenses grandfathered at the bright line. A NEW offense (not listed) fails\n"
        "# the build. Fix an entry and delete its line. Format: <file><TAB><reason>\n"
        + "\n".join(sorted(offenses)) + "\n", encoding="utf-8")
    print(f"baseline: {len(offenses)} grandfathered offenses -> {BASELINE.relative_to(ROOT)}")
    print(f"manifest: {len(_load(MANIFEST))} living docs -> {MANIFEST.relative_to(ROOT)}")
    return 0


def check():
    docs = _docs()
    if not docs:
        sys.stderr.write("docfront: matched zero docs — glob is broken, refusing to pass\n"); return 2
    manifest = _load(MANIFEST)
    offenses, _ = _offenses(manifest)
    grandfathered = _load(BASELINE)
    new = [o for o in offenses if o not in grandfathered]
    print(f"docfront check: {len(docs)} docs, {len(offenses)} offenses "
          f"({len(new)} new, {len(offenses) - len(new)} grandfathered)")
    if new:
        print(f"\nFAIL — NEW frontmatter offenses ({len(new)}):")
        for o in sorted(new):
            rel, reason = o.split("\t", 1)
            print(f"  {rel}: {reason}")
        print("\nDOCFRONT: FAIL")
        return 1
    print("DOCFRONT: PASS")
    return 0


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "check"
    if cmd == "check":    return check()
    if cmd == "baseline": return baseline()
    sys.stderr.write("usage: docfront.py {check|baseline}\n"); return 2


if __name__ == "__main__":
    sys.exit(main())
