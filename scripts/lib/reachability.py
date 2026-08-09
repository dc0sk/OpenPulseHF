#!/usr/bin/env python3
"""Reachability ratchet — find public items no PRODUCTION code references.

Coverage cannot see this: an item exercised only by tests reads as *covered*, so coverage does not
merely miss a test-only orphan, it vouches for it (the audit found 139 such items against 16 true
gaps). This sweep is the complement: for every public item, is there any reference in production
code OTHER than its own declaring file, with `#[cfg(test)]` modules stripped so test-only use does
not count as a production caller?

Heuristic by nature (a grep cannot resolve trait dispatch, macro/serde generation, or re-exports
perfectly), so it is a RATCHET, not an absolute: today's set is grandfathered in a baseline file;
a NEW item with no production caller fails the build. Shrink the baseline over time.

    reachability.py report     # print the two numbers + the orphan list
    reachability.py check      # fail (exit 1) on NEW orphans vs the baseline
    reachability.py baseline   # (re)write the baseline allowlist = today's orphan set
"""
from __future__ import annotations
import sys, os, re, glob, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
BASELINE = ROOT / "docs/dev/project/reachability-baseline.txt"
SRC_GLOBS = ["crates/*/src/**/*.rs", "plugins/*/src/**/*.rs",
             "apps/*/src/**/*.rs", "tools/*/src/**/*.rs", "pki-tooling/src/**/*.rs"]

PUB_ITEM = re.compile(r"^\s*pub(?:\([^)]*\))?\s+(?:fn|struct|enum|trait|const|static|type)\s+([A-Za-z_][A-Za-z0-9_]+)", re.M)
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]+")


def _strip_cfg_test(text):
    """Remove `#[cfg(test)] mod ... { ... }` blocks so test-only use is not a production caller."""
    out, i, n = [], 0, len(text)
    while i < n:
        m = re.search(r"#\[cfg\(test\)\]", text[i:])
        if not m:
            out.append(text[i:]); break
        start = i + m.start()
        out.append(text[i:start])
        brace = text.find("{", start)
        if brace == -1:
            break
        depth, j = 1, brace + 1
        while j < n and depth:
            if text[j] == "{": depth += 1
            elif text[j] == "}": depth -= 1
            j += 1
        i = j  # skip the whole block
    return "".join(out)


def _src_files():
    seen = set()
    for g in SRC_GLOBS:
        for p in glob.glob(str(ROOT / g), recursive=True):
            rel = os.path.relpath(p, ROOT)
            if "/tests/" in rel or rel.endswith("/build.rs"):
                continue
            seen.add(rel)
    return sorted(seen)


def _analyze():
    files = _src_files()
    ident_files: dict[str, set] = {}
    pub_items = []  # (name, file)
    for rel in files:
        try:
            raw = (ROOT / rel).read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        prod = _strip_cfg_test(raw)
        for tok in set(IDENT.findall(prod)):
            ident_files.setdefault(tok, set()).add(rel)
        for m in PUB_ITEM.finditer(prod):
            pub_items.append((m.group(1), rel))
    orphans = sorted({f"{name}\t{rel}" for name, rel in pub_items
                      if ident_files.get(name, set()) - {rel} == set()})
    return files, pub_items, orphans, ident_files


def report():
    files, pub_items, orphans, _ = _analyze()
    total, reach = len(pub_items), len(pub_items) - len(orphans)
    print(f"reachability: {len(files)} production files, {total} public items, "
          f"{reach} production-reachable, {len(orphans)} unreferenced")
    for o in orphans:
        name, rel = o.split("\t")
        print(f"  ORPHAN  {name}  ({rel})")
    return orphans


def _load_baseline():
    if not BASELINE.exists():
        return set()
    return {l.strip() for l in BASELINE.read_text().splitlines()
            if l.strip() and not l.startswith("#")}


def check():
    files, pub_items, orphans, ident_files = _analyze()
    # positive control: if the reference index is empty or NOTHING is reachable, the sweep is broken
    # and must not pass in the flattering direction.
    if not pub_items or len(orphans) == len(pub_items):
        sys.stderr.write("reachability: sweep found no reachable items — filter is broken, refusing to pass\n")
        return 2
    baseline = _load_baseline()
    keys = {o.replace("\t", "  (") + ")" for o in orphans}  # display form for messages
    new = sorted(o for o in orphans if o not in baseline)
    total, reach = len(pub_items), len(pub_items) - len(orphans)
    print(f"reachability check: {total} public items, {reach} production-reachable, "
          f"{len(orphans)} unreferenced ({len(new)} new)")
    if new:
        print(f"\nFAIL — NEW public items with no production caller ({len(new)}):")
        for o in new:
            name, rel = o.split("\t")
            print(f"  {name}  ({rel})")
        print("\n  Fix: wire it into a production path, make it pub(crate)/private, or — if it is "
              "deliberately dormant — record it (DORMANT(#issue) + rationale) and add it to")
        print(f"  {BASELINE.relative_to(ROOT)}.")
        print("\nREACH: FAIL")
        return 1
    print("\nREACH: PASS")
    return 0


def write_baseline():
    _, _, orphans, _ = _analyze()
    BASELINE.write_text(
        "# Public items with no production caller at the bright line — grandfathered.\n"
        "# A NEW unreferenced public item (not listed here) fails the build. Shrink this list.\n"
        "# Format: <item-name><TAB><declaring-file>\n"
        + "\n".join(orphans) + "\n", encoding="utf-8")
    print(f"baseline: {len(orphans)} entries -> {BASELINE.relative_to(ROOT)}")
    return 0


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "report"
    if cmd == "report":   report(); return 0
    if cmd == "check":    return check()
    if cmd == "baseline": return write_baseline()
    sys.stderr.write("usage: reachability.py {report|check|baseline}\n"); return 2


if __name__ == "__main__":
    sys.exit(main())
