#!/usr/bin/env python3
"""Traceability as data — importer, checker, renderer.

The hand-maintained matrix rotted because the record (prose) and the reality (files, tests)
shared no join key, so no script could diff them. This tool makes the trace *data* and CHECKS
it against the tree, on every run, inside the gate.

Subcommands:
  import   Build requirements.yaml from the existing requirements.md + traceability-matrix.md,
           plus a baseline orphan allowlist. One-shot migration; safe to re-run.
  check    Verify requirements.yaml against the actual tree. WARNS on grandfathered `baseline`
           drift; FAILS (exit 1) on `enforced` entries and on NEW code orphans. This is the gate.
  render   Regenerate the matrix from requirements.yaml (a generated artifact, never hand-typed).

The join key going forward is an in-code `// VERIFIES: REQ-x` comment (greppable, language-general);
the imported matrix `tests` column seeds the baseline REQ->test map. An `enforced` requirement must
carry at least one `// VERIFIES:` binding, so promoting a requirement out of `baseline` forces a real,
checked link.
"""
from __future__ import annotations
import sys, os, re, glob, json, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
REQ_MD = ROOT / "docs/dev/requirements.md"
MATRIX_MD = ROOT / "docs/dev/project/traceability-matrix.md"
YAML = ROOT / "docs/dev/project/requirements.yaml"
ORPHAN_BASELINE = ROOT / "docs/dev/project/trace-orphan-baseline.txt"
GRANDFATHERED = ROOT / "docs/dev/project/trace-grandfathered-ids.txt"
RENDER_OUT = ROOT / "docs/dev/project/traceability-matrix.generated.md"

# Production source roots. A file here that no capability claims is an orphan.
SRC_GLOBS = [
    "crates/*/src/**/*.rs", "plugins/*/src/**/*.rs",
    "apps/*/src/**/*.rs", "tools/*/src/**/*.rs", "pki-tooling/src/**/*.rs",
]

REQ_ID = re.compile(r"REQ-[A-Z]+-\d+")
CAP_ID = re.compile(r"CAP-\d+")
PATH_RS = re.compile(r"[\w./-]+\.rs")

try:
    import yaml
except ImportError:
    sys.stderr.write("trace: pyyaml is required (pip install pyyaml)\n"); sys.exit(2)


# ----------------------------------------------------------------------------- import
def _table_rows(text, first_col):
    """Yield split cells for markdown rows whose first data cell matches first_col."""
    for line in text.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if cells and first_col.match(cells[0]):
            yield cells


def do_import():
    reqmd = REQ_MD.read_text(encoding="utf-8")
    matrix = MATRIX_MD.read_text(encoding="utf-8")

    # statements from requirements.md bullets: - **REQ-X** — statement.
    statements = {}
    for m in re.finditer(r"^-\s+\*\*(REQ-[A-Z]+-\d+)\*\*\s*[—-]+\s*(.+?)\s*$", reqmd, re.M):
        statements[m.group(1)] = m.group(2)

    requirements = {}
    for cells in _table_rows(matrix, REQ_ID):  # REQ-ID | Category | Requirement | Covered by | Status
        rid = cells[0]
        cat = cells[1] if len(cells) > 1 else ""
        covered = CAP_ID.findall(cells[3]) if len(cells) > 3 else []
        requirements[rid] = {
            "statement": statements.get(rid, cells[2] if len(cells) > 2 else ""),
            "category": cat,
            "status": "ratified",          # the existing spec is the ratified intent
            "traceability": "baseline",    # grandfathered; checker warns, does not fail
            "covered_by": covered,
        }
    # requirements present in the prose spec but absent from the matrix table
    for rid, st in statements.items():
        requirements.setdefault(rid, {
            "statement": st, "category": "", "status": "ratified",
            "traceability": "baseline", "covered_by": [],
        })

    capabilities = {}
    for cells in _table_rows(matrix, CAP_ID):
        cid = cells[0]
        satisfies = REQ_ID.findall(cells[2]) if len(cells) > 2 else []
        code_cell = cells[4] if len(cells) > 4 else ""
        test_cell = cells[5] if len(cells) > 5 else ""
        # keep only real paths (contain a '/'); bare filenames in prose are not locations
        code = sorted({p for p in PATH_RS.findall(code_cell) if "/" in p})
        tests = sorted({p for p in PATH_RS.findall(test_cell) if "/" in p})
        capabilities[cid] = {
            "name": cells[1] if len(cells) > 1 else "",
            "satisfies": satisfies,
            "code": code,
            "tests": tests,
            "traceability": "baseline",
        }

    doc = {
        "meta": {
            "generated_by": "scripts/trace.sh import",
            "sources": ["docs/dev/requirements.md", "docs/dev/project/traceability-matrix.md"],
            "bright_line": "2026-08-09",
            "note": ("baseline entries are grandfathered (checker WARNS on drift). Set "
                     "traceability: enforced to make an entry FAIL the build on drift; an enforced "
                     "requirement must carry an in-code // VERIFIES: REQ-x binding. New requirements "
                     "and new code default to enforced."),
        },
        "requirements": requirements,
        "capabilities": capabilities,
    }
    YAML.write_text(yaml.safe_dump(doc, sort_keys=True, allow_unicode=True, width=100), encoding="utf-8")

    # baseline orphan allowlist: every production file no capability claims, TODAY. New orphans
    # (files not in this list) fail the check — the ratchet.
    claimed = _claimed_files(capabilities)
    orphans = sorted(f for f in _src_files() if f not in claimed)
    ORPHAN_BASELINE.write_text(
        "# Production source files unclaimed by any capability at the bright line.\n"
        "# Grandfathered: the checker warns on these. A file NOT listed here that is also\n"
        "# unclaimed is a NEW orphan and fails the build. Shrink this list over time.\n"
        + "\n".join(orphans) + "\n", encoding="utf-8")

    print(f"import: {len(requirements)} requirements, {len(capabilities)} capabilities "
          f"-> {YAML.relative_to(ROOT)}")
    print(f"import: {len(orphans)} baseline orphans -> {ORPHAN_BASELINE.relative_to(ROOT)}")


# ----------------------------------------------------------------------------- helpers
def _src_files():
    out = set()
    for g in SRC_GLOBS:
        for p in glob.glob(str(ROOT / g), recursive=True):
            rel = os.path.relpath(p, ROOT)
            if "/tests/" in rel or rel.endswith("/build.rs"):
                continue
            out.add(rel)
    return out


def _matches(pathspec):
    """A code/test entry may be a file, a directory, or a glob. Return matched real files."""
    hits = glob.glob(str(ROOT / pathspec), recursive=True)
    if not hits and any(ch in pathspec for ch in "*?["):
        hits = glob.glob(str(ROOT / pathspec))
    return [os.path.relpath(h, ROOT) for h in hits if os.path.exists(h)]


def _claimed_files(capabilities):
    claimed = set()
    for cap in capabilities.values():
        for c in cap.get("code", []):
            claimed.update(_matches(c))
    return claimed


def _scan_verifies():
    """Grep the tree for in-code `// VERIFIES: REQ-x` bindings.

    Returns {req: [{"file": rel, "fn": test_fn_or_None}, ...]}. The fn is the next `fn NAME`
    after the comment — the test whose run status the checker can then confirm.
    """
    binds = {}
    pat = re.compile(r"//\s*VERIFIES:\s*(REQ-[A-Z]+-\d+(?:\s*,\s*REQ-[A-Z]+-\d+)*)")
    fnpat = re.compile(r"\bfn\s+([a-zA-Z0-9_]+)\s*[(<]")
    for base in ("crates", "plugins", "apps", "tools", "pki-tooling"):
        for p in glob.glob(str(ROOT / base / "**/*.rs"), recursive=True):
            try:
                txt = pathlib.Path(p).read_text(encoding="utf-8", errors="ignore")
            except OSError:
                continue
            for m in pat.finditer(txt):
                fn = fnpat.search(txt, m.end())
                fname = fn.group(1) if fn else None
                for rid in REQ_ID.findall(m.group(1)):
                    binds.setdefault(rid, []).append(
                        {"file": os.path.relpath(p, ROOT), "fn": fname})
    return binds


def _passed_tests():
    """Names of tests that PASSED in the most recent real gate run.

    Reads $GATE_LOG if set (gate.sh exports it), else the newest target/gate-*.log. `cargo test`
    prints `test <path::name> ... ok` per test; we key on the final segment (the fn name). Returns
    None when no run is available — the caller then cannot confirm run status and says so, rather
    than failing a check it has no evidence for.
    """
    log = os.environ.get("GATE_LOG")
    if not log or not os.path.exists(log):
        logs = sorted(glob.glob(str(ROOT / "target" / "gate-*.log")), key=os.path.getmtime)
        log = logs[-1] if logs else None
    if not log or not os.path.exists(log):
        return None
    passed = set()
    rx = re.compile(r"^test ([\w:]+) \.\.\. ok$")
    for line in pathlib.Path(log).read_text(encoding="utf-8", errors="ignore").splitlines():
        m = rx.match(line.strip())
        if m:
            passed.add(m.group(1).split("::")[-1])
    return passed


# ----------------------------------------------------------------------------- check
def do_check(release=False):
    if not YAML.exists():
        sys.stderr.write("trace: no requirements.yaml — run `scripts/trace.sh import` first\n"); return 2
    doc = yaml.safe_load(YAML.read_text(encoding="utf-8")) or {}
    reqs = doc.get("requirements", {})
    caps = doc.get("capabilities", {})
    # positive control: a checker that silently matched nothing is the flattering-direction failure.
    if not reqs or not caps:
        sys.stderr.write("trace: requirements.yaml has no requirements/capabilities — refusing to pass\n")
        return 2
    src = _src_files()
    if not src:
        sys.stderr.write("trace: matched zero production source files — filter is broken, refusing to pass\n")
        return 2
    baseline_orphans = set()
    if ORPHAN_BASELINE.exists():
        baseline_orphans = {l.strip() for l in ORPHAN_BASELINE.read_text().splitlines()
                            if l.strip() and not l.startswith("#")}
    binds = _scan_verifies()

    # The grandfathered set. `traceability: baseline` is legal ONLY for an id in this file, which
    # is what makes requirements.yaml's "new requirements and new code default to enforced" a
    # mechanism rather than a promise (#1158). The list only shrinks: removing an id enforces it,
    # and nothing can add one, so a new entry cannot be grandfathered by copying a neighbour's
    # `baseline` — which is how it would happen, since 221 of 232 entries say baseline.
    grandfathered = set()
    if GRANDFATHERED.exists():
        grandfathered = {l.strip() for l in GRANDFATHERED.read_text().splitlines()
                         if l.strip() and not l.startswith("#")}

    fails, warns = [], []

    # Enforcement is decided HERE and nowhere else. It used to be decided by an inline
    # `== "enforced"` at two separate sites, which is the fix-two-of-five-arms shape: a change at
    # one site silently leaves the other on the old semantics.
    def is_enforced(entry):
        return entry.get("traceability") == "enforced"

    def flag(entry, msg):
        (fails if is_enforced(entry) else warns).append(msg)

    # An absent or misspelled field is a HARD ERROR, not a silent downgrade to warn-only (#1158).
    # The old code compared against the literal "enforced", so `traceability:` absent, `baseline`,
    # `enforcd` and `Enforced` all took the same warn-only path — three of those four silently, and
    # the note in requirements.yaml promised the opposite for exactly the entries it was about.
    vocab_errors = []
    for kind, table in (("requirement", reqs), ("capability", caps)):
        for eid, entry in table.items():
            if not isinstance(entry, dict):
                continue
            tr = entry.get("traceability")
            if tr is None:
                vocab_errors.append(
                    f"{eid}: NO-TRACEABILITY — {kind} has no `traceability:` field. Say which you "
                    f"mean: `enforced` (drift FAILS) or `baseline` (drift warns, grandfathered "
                    f"only)."
                )
            elif tr not in ("enforced", "baseline"):
                vocab_errors.append(
                    f"{eid}: BAD-TRACEABILITY — `traceability: {tr}` is not one of "
                    f"enforced|baseline."
                )
            elif tr == "baseline" and eid not in grandfathered:
                vocab_errors.append(
                    f"{eid}: NOT-GRANDFATHERED — `traceability: baseline` is reserved for ids in "
                    f"{GRANDFATHERED.name}. A new {kind} is enforced; do not inherit `baseline` "
                    f"by copying a neighbour."
                )
    if vocab_errors:
        print("trace: traceability field errors — these are not warnings\n")
        for m in vocab_errors:
            print(f"  {m}")
        print(f"\n  {len(vocab_errors)} error(s)")
        print("\nTRACE: FAIL")
        return 1

    # dangling code / tests, and CAP<->REQ bidirectional agreement
    for cid, cap in caps.items():
        for c in cap.get("code", []):
            if not _matches(c):
                flag(cap, f"{cid}: DANGLING-CODE — `{c}` matches no file")
        for t in cap.get("tests", []):
            if not _matches(t):
                flag(cap, f"{cid}: DANGLING-TEST — cited test `{t}` does not exist")
        if not cap.get("satisfies"):
            flag(cap, f"{cid}: CAP-ORPHAN — capability satisfies no requirement")
        for rid in cap.get("satisfies", []):
            if rid in reqs and cid not in reqs[rid].get("covered_by", []):
                flag(cap, f"{cid}: BIDIR-DRIFT — claims to satisfy {rid}, but {rid} does not list {cid}")

    passed = _passed_tests()  # None => no gate run available to confirm against
    for rid, req in reqs.items():
        cov = req.get("covered_by", [])
        if not cov:
            flag(req, f"{rid}: REQ-GAP — no capability covers it")
        for cid in cov:
            if cid in caps and rid not in caps[cid].get("satisfies", []):
                flag(req, f"{rid}: BIDIR-DRIFT — covered_by {cid}, but {cid} does not satisfy {rid}")
        enforced = is_enforced(req)
        if enforced and rid not in binds:
            fails.append(f"{rid}: MISSING-BINDING — enforced requirement has no `// VERIFIES: {rid}` in code")
        # CITED-BUT-DIDN'T-RUN: an enforced binding's test must have PASSED in the last real run.
        if enforced and passed is not None:
            for b in binds.get(rid, []):
                fn = b.get("fn")
                if fn and fn not in passed:
                    fails.append(f"{rid}: CITED-BUT-DIDN'T-RUN — bound test `{fn}` "
                                 f"({b['file']}) did not pass in the last gate run")

    # release: no shipped code may trace only to a draft requirement
    if release:
        for rid, req in reqs.items():
            if req.get("status") == "draft" and (req.get("covered_by") or rid in binds):
                fails.append(f"{rid}: DRAFT-SHIPPED — draft requirement has code/bindings; "
                             f"ratify it (record who + when) or pull the code before release")

    # dangling in-code bindings
    for rid in binds:
        if rid not in reqs:
            fails.append(f"binding: DANGLING-BINDING — `// VERIFIES: {rid}` names a requirement not in requirements.yaml")

    # code orphans: production files no capability claims. Baseline grandfathered; NEW ones fail.
    claimed = _claimed_files(caps)
    orphans = sorted(f for f in src if f not in claimed)
    new_orphans = [f for f in orphans if f not in baseline_orphans]
    for f in new_orphans:
        fails.append(f"orphan: NEW-ORPHAN — `{f}` is claimed by no capability and is not in the baseline allowlist")

    # ---- report (failure list never truncated; matches gate.sh discipline) ----
    print(f"trace check: {len(reqs)} requirements, {len(caps)} capabilities, "
          f"{len(binds)} in-code bindings, {len(orphans)} code orphans "
          f"({len(new_orphans)} new)")
    if passed is None:
        print("  NOTE: no gate run found — run-status of enforced bindings unverified "
              "(runs inside scripts/gate.sh, which exports GATE_LOG)")
    if warns:
        print(f"\nWARN (grandfathered baseline drift — {len(warns)}; shrink over time):")
        for w in sorted(warns)[:40]:
            print(f"  {w}")
        if len(warns) > 40:
            print(f"  ... and {len(warns) - 40} more (see requirements.yaml)")
    if fails:
        print(f"\nFAIL (enforced or new — {len(fails)}):")
        for f in sorted(fails):
            print(f"  {f}")
        print("\nTRACE: FAIL")
        return 1
    print("\nTRACE: PASS")
    return 0


# ----------------------------------------------------------------------------- render
def do_render():
    doc = yaml.safe_load(YAML.read_text(encoding="utf-8")) or {}
    reqs, caps = doc.get("requirements", {}), doc.get("capabilities", {})
    binds = _scan_verifies()
    lines = [
        "<!-- GENERATED by scripts/trace.sh render — do not edit; edit requirements.yaml. -->",
        "# Traceability matrix (generated)", "",
        f"{len(reqs)} requirements, {len(caps)} capabilities. Regenerate with `scripts/trace.sh render`;",
        "CI requires this file to be in sync (`git diff --exit-code`).", "",
        "## Requirements → capabilities → bindings", "",
        "| REQ-ID | Status | Trace | Covered by | In-code bindings |",
        "|---|---|---|---|---|",
    ]
    for rid in sorted(reqs):
        r = reqs[rid]
        bfiles = sorted({b["file"] for b in binds.get(rid, [])})
        lines.append(f"| {rid} | {r.get('status','')} | {r.get('traceability','')} | "
                     f"{', '.join(r.get('covered_by',[])) or '—'} | "
                     f"{', '.join(bfiles) or '—'} |")
    RENDER_OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"render: {RENDER_OUT.relative_to(ROOT)}")
    return 0


def do_scope(rid):
    """Emit a requirement's CAP code files and bound test fns, for requirement-scoped mutation."""
    doc = yaml.safe_load(YAML.read_text(encoding="utf-8")) or {}
    reqs, caps = doc.get("requirements", {}), doc.get("capabilities", {})
    if rid not in reqs:
        sys.stderr.write(f"trace scope: {rid} not in requirements.yaml\n"); return 2
    files = set()
    for cid in reqs[rid].get("covered_by", []):
        for c in caps.get(cid, {}).get("code", []):
            files.update(_matches(c))
    for f in sorted(files):
        print(f"CODE\t{f}")
    for b in _scan_verifies().get(rid, []):
        if b.get("fn"):
            print(f"TEST\t{b['fn']}")
    return 0


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "scope":
        return do_scope(sys.argv[2]) if len(sys.argv) > 2 else 2
    if cmd == "import":
        do_import(); return 0
    if cmd == "check":
        return do_check(release="--release" in sys.argv[2:])
    if cmd == "render":
        return do_render()
    sys.stderr.write(f"usage: trace.py {{import|check|render}}\n"); return 2


if __name__ == "__main__":
    sys.exit(main())
