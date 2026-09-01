#!/usr/bin/env python3
"""Traceability as data — the checker.

The hand-maintained matrix rotted because the record (prose) and the reality (files, tests)
shared no join key, so no script could diff them. This tool makes the trace *data* and CHECKS
it against the tree, on every run, inside the gate.

`docs/dev/project/requirements.yaml` is the SOURCE OF TRUTH and is edited by hand. It was
originally imported from `requirements.md` + `traceability-matrix.md`, but that importer was
deleted in #1223: its sources went stale by construction at the bright line, so every re-run grew
strictly more destructive, while it advertised itself as "safe to re-run". If the yaml is missing,
restore it from git — it cannot be regenerated.

Subcommands:
  check    Verify requirements.yaml against the actual tree. WARNS on grandfathered `baseline`
           drift; FAILS (exit 1) on `enforced` entries and on NEW code orphans. This is the gate.
  evidence-self-test
           Probe the gate-log evidence channel that `check` reads for run-status (#1224).
  graph-self-test
           Probe the cargo dependency graph the dormancy join runs on (#1240) — crate resolution
           and edge filtering, the two things that were wrong in the instrument that produced
           #1237's evidence while reporting the correct headline numbers.

The join key is an in-code `// VERIFIES: REQ-x` comment (greppable, language-general); the baseline
REQ->test map was seeded from the matrix `tests` column at import time. An `enforced` requirement
must carry at least one `// VERIFIES:` binding, so promoting a requirement out of `baseline` forces
a real, checked link.

WHAT `enforced` MEANS, EXACTLY (#1237)
--------------------------------------
Written down because the gap between the code's meaning and the reader's meaning is what made the
`REQ-CTL-04` error easy to make. Until #1237, `enforced` asserted ONE thing — a `// VERIFIES:`
binding exists and its test passed in the last real gate run — while the registry was read as a
statement of product capability. Nothing joined the two, so a unit test of a module nothing
consumes satisfied the letter completely, and did: `REQ-CTL-04` shipped bound to a passing keystore
test while `openpulse-keystore` had zero consumers and the daemon read its PSK from an env var.

`enforced` now asserts both halves:

  1. a `// VERIFIES:` binding exists, and its test PASSED in the last real gate run; and
  2. at least one binding sits in a package that is NOT workspace-dormant.

Read (2) literally. It says *not workspace-dormant* — some chain of normal, non-optional
dependency edges reaches a package with a `bin` target — and NOTHING STRONGER. It is not
"production-reachable":

  * A dormant MODULE inside a live package passes it. `openpulse-core::pq_handshake` is exactly
    that today: every entry point sits in the orphan baseline and its only reference outside its
    own file is a `pub use` re-export, inside a crate that reaches every binary.
    `scripts/lib/reachability.py` is the finer, item-level instrument — and it is blind to the case
    THIS join catches, because a dead crate whose two files reference each other looks referenced
    to it. The two are complements; neither subsumes the other.

    The requirement sitting in that position is `REQ-PQ-05`, and it is deliberately left
    `enforced`. Its statement is a SIZE claim — ML-DSA-44 signatures and ML-KEM-768 keys exceed
    the 255-byte frame payload — and its binding (`sar_roundtrip.rs`) verifies exactly that, on
    SAR, which IS wired. The binding is germane and the join's verdict is right. What a reader
    may over-infer from it — "PQ artifacts cross the link today" — is not what the statement says,
    and no machine here checks the distance between a statement and what it evokes.
  * Only ONE binding has to be live. Whether a binding is *germane* to its requirement remains a
    manual norm, unchecked by anything here.
  * The 146 `baseline` entries are exempt from both halves by construction.

Believing "passed the reachability join" means "production-reachable" would be the original
over-read recursing one level up, which is why the claim is spelled out rather than named.

`unwired` is enforced-with-a-dormant-consumer: obligation (1) in full, (2) deliberately absent,
plus a tracking issue named in the statement. It FAILS the moment its package reaches a binary —
a demand to reconcile, never an automatic promotion, because at package granularity "reaches a
binary" does not imply "this requirement's capability is called".
"""
from __future__ import annotations
import sys, os, re, glob, json, pathlib, subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]
YAML = ROOT / "docs/dev/project/requirements.yaml"
# Frozen baselines. Nothing writes these any more (#1223 deleted `import`, their only writer), which
# is the intended end state: a baseline may SHRINK by hand as entries are paid down, and must never
# grow back by regeneration.
ORPHAN_BASELINE = ROOT / "docs/dev/project/trace-orphan-baseline.txt"
GRANDFATHERED = ROOT / "docs/dev/project/trace-grandfathered-ids.txt"

# Production source roots. A file here that no capability claims is an orphan.
SRC_GLOBS = [
    "crates/*/src/**/*.rs", "plugins/*/src/**/*.rs",
    "apps/*/src/**/*.rs", "tools/*/src/**/*.rs", "pki-tooling/src/**/*.rs",
]

# THE canonical id shapes, defined ONCE. Every consumer below is built from these by reference.
# #1229: `REQ_ID` and the VERIFIES scanner used to carry the same pattern independently, and both
# were blind to `REQ-SEC-CTL-01` (two category segments) and `REQ-DCD-ADAPT` (no numeric suffix) —
# so seven shipped requirements could not be registered at all, and nothing said so. A second
# hand-copy of an id pattern is the defect, not the typo in it.
REQ_SHAPE = r"REQ-[A-Z]+-\d+"
CAP_SHAPE = r"CAP-\d+"
REQ_ID = re.compile(REQ_SHAPE)
CANON_REQ = re.compile(REQ_SHAPE + r"$")
CANON_CAP = re.compile(CAP_SHAPE + r"$")

# Deliberately LOOSER than the canonical shapes: this is the tokenizer whose job is to over-collect
# so that non-conforming ids become RESIDUE rather than silently falling outside the checked set.
# That inversion is the whole fix — the old pattern was a SELECTOR (anything it missed vanished);
# this one feeds a VALIDATOR (anything it collects and cannot validate FAILS). You cannot write a
# regex with no blind spot; you can choose whether the blind spot fails loud or silent.
ID_TOKEN = re.compile(r"\b(?:REQ|CAP)-[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*(?:/\d+)*")

# Notation, not ids: table headers and placeholders that legitimately look id-shaped. A MISSING
# entry here is a loud false failure, fixed in one line — never a silent pass.
ID_VOCABULARY = {
    "REQ-ID", "REQ-IDs", "REQ-NN", "REQ-x", "REQ-y", "REQ-GAP",
    "CAP-ID", "CAP-IDs", "CAP-NN", "CAP-x",
    "REQ-DOES-NOT-EXIST-99",  # deliberate negative fixture in scripts/check-trailer.sh
    "CAP-ORPHAN",             # a CHECK name, not an id
    "CAP-SELFTEST-SABOTAGE",  # the self-test's planted fixture in scripts/trace.sh
}

# Ids that were RENAMED or RETIRED and are deliberately still named in living text: changelog
# mapping tables, and comments explaining why the rename happened.
#
# SCOPE, stated exactly because the wording used to overclaim: these are allowed ANYWHERE, in any
# living file, not only in a mapping table. A brand-new doc citing `REQ-FT-03` tomorrow passes
# silently. That is the same permissiveness the unregistered baseline gave them, so moving ids here
# weakens nothing — but it is a per-id allowlist, not a per-site one, and the only thing keeping it
# honest is that it is bounded and shrink-only: a NEW off-convention id is not on this list and
# fails.
# The legal `traceability:` values. `enforced` and `unwired` differ ONLY in whether the bound
# capability is workspace-dormant, and that difference is decided by the machine (#1237), not by
# the author — the author's one degree of freedom is naming the tracking issue.
TRACEABILITY_VALUES = ("enforced", "unwired", "baseline")

RENAMED_IDS = {
    "REQ-SEC-CTL", "REQ-SEC-CTL-01", "REQ-SEC-CTL-02", "REQ-SEC-CTL-03",
    "REQ-SEC-CTL-04", "REQ-SEC-CTL-05", "REQ-SEC-CTL-06", "REQ-DCD-ADAPT",
    # the draft file-transfer scheme, reconciled to REQ-FX-* in #1235; the plan doc keeps a
    # mapping table so an old id still leads somewhere.
    "REQ-FT-01", "REQ-FT-02", "REQ-FT-03", "REQ-FT-04", "REQ-FT-05", "REQ-FT-06", "REQ-FT-07",
}
ID_VOCABULARY |= RENAMED_IDS

# Corpora that record what was true at a past date. Excluded as a CLASS, by path — not per id — so
# the exclusion cannot grow one incident at a time: rewriting a dated audit would change what the
# auditor wrote. The old ids stay there, and the changelog carries the old->new mapping so one grep
# finds both.
FROZEN_PREFIXES = (
    "docs/dev/reviews/",
    "docs/dev/project/traceability.md",
    "CHANGELOG.md",
    "SBOM.spdx.json",
)
UNREGISTERED_BASELINE = ROOT / "docs/dev/project/trace-unregistered-ids.txt"

try:
    import yaml
except ImportError:
    sys.stderr.write("trace: pyyaml is required (pip install pyyaml)\n"); sys.exit(2)


# ----------------------------------------------------------------------------- import
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
    pat = re.compile(rf"//\s*VERIFIES:\s*({REQ_SHAPE}(?:\s*,\s*{REQ_SHAPE})*)")
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


class CargoUnavailable(Exception):
    """`cargo metadata` could not be run or parsed.

    A LOUD error, never a skip. A checker that silently drops its newest rule on the one machine
    where cargo is missing is the self-consistent-checker archetype this file exists to avoid.
    """


def _workspace_graph():
    """Return `(pkg_of_dir, prod_rdeps, bin_pkgs)` from `cargo metadata`.

    Two things here were wrong in the throwaway script that produced this rule's evidence, and
    both were found by review rather than by the numbers, which reproduced either way:

    1. **Package names are not directory names.** `plugins/bpsk` is package `bpsk-plugin` and
       `plugins/64qam` is `qam64-plugin` — ten packages diverge. Resolving a file's crate by path
       segment makes every plugin binding look unreachable: a false-FAIL machine for a whole
       layer, invisible today only because all current bindings live where dir == package name.
       Resolve by `manifest_path` instead, longest-prefix.
    2. **Edge kind and `optional` must be filtered.** `openpulse-gpu` is depended on ONLY through
       optional feature-gated edges; counting those as production reach would let a GPU
       requirement bound to a CPU-fallback test pass this join while the GPU path never executes
       in any gated build — the very laundering the rule exists to stop.
    """
    try:
        out = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            capture_output=True, text=True, cwd=str(ROOT), timeout=300,
        )
    except (OSError, subprocess.SubprocessError) as e:
        raise CargoUnavailable(f"could not run `cargo metadata`: {e}")
    if out.returncode != 0:
        raise CargoUnavailable(
            f"`cargo metadata` exited {out.returncode}: {out.stderr.strip()[:400]}")
    try:
        meta = json.loads(out.stdout)
    except ValueError as e:
        raise CargoUnavailable(f"could not parse `cargo metadata` output: {e}")

    pkgs = meta.get("packages", [])
    if not pkgs:
        raise CargoUnavailable("`cargo metadata` reported no packages")

    pkg_of_dir, bin_pkgs, rdeps = {}, set(), {}
    for pkg in pkgs:
        pkg_of_dir[os.path.dirname(pkg["manifest_path"])] = pkg["name"]
        if any("bin" in t.get("kind", []) for t in pkg.get("targets", [])):
            bin_pkgs.add(pkg["name"])
    names = set(pkg_of_dir.values())
    for pkg in pkgs:
        for dep in pkg.get("dependencies", []):
            # kind None == a normal dependency; "dev" and "build" edges do not ship.
            if dep.get("kind") is not None or dep.get("optional"):
                continue
            if dep["name"] in names:
                rdeps.setdefault(dep["name"], set()).add(pkg["name"])
    return pkg_of_dir, rdeps, bin_pkgs


def _optional_only_packages():
    """Workspace packages every one of whose incoming normal edges is `optional = true`.

    Kept separate from `_workspace_graph` because it is the *discriminating population* for the
    edge-filtering probe, not an input to the join. A package here is reachable only when a feature
    is enabled, so it must never confer production reach.
    """
    try:
        out = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            capture_output=True, text=True, cwd=str(ROOT), timeout=300,
        )
        meta = json.loads(out.stdout) if out.returncode == 0 else {}
    except (OSError, subprocess.SubprocessError, ValueError):
        return set()
    pkgs = meta.get("packages", [])
    names = {p["name"] for p in pkgs}
    incoming, optional_incoming = {}, {}
    for pkg in pkgs:
        for dep in pkg.get("dependencies", []):
            if dep.get("kind") is not None or dep["name"] not in names:
                continue
            incoming[dep["name"]] = incoming.get(dep["name"], 0) + 1
            if dep.get("optional"):
                optional_incoming[dep["name"]] = optional_incoming.get(dep["name"], 0) + 1
    return {n for n, total in incoming.items()
            if total > 0 and optional_incoming.get(n, 0) == total}


def _package_of(rel, pkg_of_dir):
    """The package owning source file `rel`, by longest manifest-directory prefix."""
    full = os.path.abspath(str(ROOT / rel))
    best = None
    for d, name in pkg_of_dir.items():
        if full.startswith(d + os.sep) and (best is None or len(d) > len(best[0])):
            best = (d, name)
    return best[1] if best else None


def _dormant_packages(rdeps, bin_pkgs, all_pkgs):
    """Packages no chain of normal, non-optional edges connects to a package with a `bin` target.

    This is the ONLY claim the join makes, and the docstring says it in those words on purpose.
    It is deliberately weaker than "production-reachable": a dormant MODULE inside a live package
    passes it — `openpulse-core::pq_handshake` is exactly that today, every entry point sitting in
    the orphan baseline inside a crate that reaches every binary. Reading this join as
    reachability would be the original over-read recursing one level up.
    """
    dormant = set()
    for name in all_pkgs:
        seen, stack, live = set(), [name], False
        while stack:
            cur = stack.pop()
            if cur in bin_pkgs:
                live = True
                break
            if cur in seen:
                continue
            seen.add(cur)
            stack.extend(rdeps.get(cur, ()))
        if not live:
            dormant.add(name)
    return dormant


class EvidenceChannelBroken(Exception):
    """The gate handed us a log it says is complete and it is not — a mechanism failure, not a
    missing run. Raised so the caller can fail LOUDLY; degrading to the NOTE path here would be a
    permanently-printed, never-failing message (#1224)."""


# Positional, deliberately NOT the step's display name. Matching `=== end cargo test (workspace)`
# would hand-transcribe a constant across bash and python, which cannot be shared by reference: a
# step rename would silently return None forever and the check would degrade to a NOTE that can
# never fail. Instead: require an end-marker AFTER the last `test result:` line, which says "the
# step that produced the test output finished" without naming it.
_END_MARKER = re.compile(r"^=== end .*: exit -?\d+ ===$")
_TEST_RESULT = re.compile(r"^test result:")


def _log_is_complete(path):
    """True when `path` holds a FINISHED cargo-test step.

    Two failures this separates, both of which produced the same false CITED-BUT-DIDN'T-RUN:
      * a truncated log (gate still running, or killed) — has test output, no end marker after it;
      * a `--quick` or self-test log — no `test result:` lines at all, which used to yield an
        EMPTY SET rather than None, so every enforced binding reported "did not run".
    """
    saw_result = False
    complete = False
    for line in pathlib.Path(path).read_text(encoding="utf-8", errors="ignore").splitlines():
        line = line.strip()
        if _TEST_RESULT.match(line):
            saw_result = True
            complete = False          # more test output: any earlier marker was a previous step's
        elif saw_result and _END_MARKER.match(line):
            complete = True
    return saw_result and complete


def _current_toolchain():
    """`rustc -V`, or None when it cannot be read.

    Returns None rather than raising: this is used to EXPIRE evidence, and a host without rustc
    should not be told its stored verdict is stale for that reason.
    """
    try:
        out = subprocess.run(["rustc", "-V"], capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.SubprocessError):
        return None
    return out.stdout.strip() if out.returncode == 0 else None


def _evidence_log():
    """The log whose test results may be trusted, or None.

    `GATE_LOG` (set by gate.sh) is authoritative: control flow guarantees the test step finished
    before the trace step runs, so a log without a marker there means the MECHANISM broke — the
    steps were reordered, or the marker format drifted. That is a hard error, not a NOTE.

    Otherwise prefer the log named by `target/gate-verdict.json`, which is written only at the end
    of a full gate and so is complete by construction — this is what lets a manual check during an
    in-flight gate answer from the PREVIOUS completed run instead of throwing the evidence away.
    Falling back to the newest COMPLETE `gate-*.log` also skips `--quick` logs and self-test logs
    (a self-test is a full run of a deliberately sabotaged tree, and its log matches the glob).
    """
    env_log = os.environ.get("GATE_LOG")
    if env_log and os.path.exists(env_log):
        if not _log_is_complete(env_log):
            raise EvidenceChannelBroken(
                f"GATE_LOG={env_log} has no completion marker after its test output. Inside the "
                f"gate the test step has finished by construction, so this means the marker "
                f"mechanism itself is broken (steps reordered, or the `=== end <step>: exit N ===` "
                f"format drifted away from trace.py's matcher)."
            )
        return env_log, None

    verdict = ROOT / "target" / "gate-verdict.json"
    if verdict.exists():
        try:
            v = json.loads(verdict.read_text(encoding="utf-8"))
        except (ValueError, OSError):
            v = {}
        # A run the gate marked INVALID (#1151) had the tree or HEAD move under it, so its log is
        # not attributable to any single state of the repo — completeness is not enough to make it
        # evidence. Measured before the check existed: an INVALID verdict naming a COMPLETE log was
        # accepted as proof the cited tests ran.
        if v.get("result") == "INVALID":
            return None, ("the last gate run was INVALID (tree or HEAD moved during it), so its "
                          "log is not attributable evidence")
        # A verdict is attributable to (tree, HEAD, TOOLCHAIN). The third member drifts without
        # anyone performing an act — a distro package upgrade — and it re-derives differently:
        # rustc 1.98.0 landed on this host and `main` went red on a lint that did not exist when
        # the standing PASS was taken. Refuse here rather than falling through to the mtime scan,
        # which would find the SAME stale log. Returning None degrades to "no usable run", which
        # the callers report rather than mistaking for "the cited test did not run".
        want = _current_toolchain()
        got_tc = v.get("toolchain")
        if got_tc != want:
            was = got_tc or "an unrecorded toolchain"
            return None, (f"the last gate verdict was produced by {was} and this is {want} — a "
                          f"verdict does not survive a toolchain change; run a full gate")
        named = v.get("log")
        if named and os.path.exists(named) and _log_is_complete(named):
            return named, None

    candidates = sorted(glob.glob(str(ROOT / "target" / "gate-*.log")), key=os.path.getmtime)
    for path in reversed(candidates):
        if _log_is_complete(path):
            return path, None
    if candidates:
        return None, (f"{len(candidates)} gate log(s) in target/, none with a completed test step "
                      f"(pre-marker, in flight, --quick, or self-test)")
    return None, "no gate log in target/"


def _passed_tests():
    """Names of tests that PASSED in the most recent COMPLETE gate run, or None.

    `cargo test` prints `test <path::name> ... ok`; we key on the final segment (the fn name), so a
    same-named test in another binary vouches for this one — a pre-existing weakness of the check,
    noted here because it also weakens any sabotage of it.

    Returns None when no usable run exists — the caller then says so rather than failing a check it
    has no evidence for. It must NEVER return an empty set for that case: an empty set reports every
    enforced binding as "did not run", which is the #1224 false FAIL.
    """
    log, _why = _evidence_log()
    if not log:
        return None
    passed = set()
    rx = re.compile(r"^test ([\w:]+) \.\.\. ok$")
    for line in pathlib.Path(log).read_text(encoding="utf-8", errors="ignore").splitlines():
        m = rx.match(line.strip())
        if m:
            passed.add(m.group(1).split("::")[-1])
    return passed


# ------------------------------------------------------------------- evidence-channel self-test
def do_evidence_selftest():
    """Probe the gate-log evidence channel (#1224). Asserts BEHAVIOUR, never an exit code.

    Every probe below passed before the fix for a different reason, so each names the specific
    outcome it requires rather than 'something went wrong'.
    """
    import tempfile, io
    rc = 0

    def ok(msg):
        print(f"  ok: {msg}")

    def bad(msg):
        nonlocal rc
        print(f"  EVIDENCE-SELF-TEST FAIL: {msg}")
        rc = 1

    # A. TWO-ANCHOR FORMAT AGREEMENT. The marker is emitted by gate.sh (bash) and matched here
    #    (python); the two cannot share the constant by reference, which is exactly the
    #    hand-transcribed-constant shape that rots. So take the format from gate.sh's own source and
    #    require this module's matcher to accept it. If either side drifts, this fails — where the
    #    alternative is _passed_tests() silently returning None forever behind a NOTE that can never
    #    fail.
    gate_src = (ROOT / "scripts" / "gate.sh").read_text(encoding="utf-8")
    m = re.search(r'echo "(=== end [^"]*)" >> "\$LOG"', gate_src)
    if not m:
        bad("gate.sh emits no `=== end ...` marker — the evidence channel has no completeness signal")
        sample = None
    else:
        sample = m.group(1).replace("$step_name", "cargo test (workspace)").replace("$rc", "0")
        if _END_MARKER.match(sample):
            ok(f"gate.sh's marker format is accepted by trace.py's matcher ({sample!r})")
        else:
            bad(f"gate.sh emits {sample!r}, which trace.py's _END_MARKER does not match")

    if sample is None:
        return 1

    body = "test some_module::a_cited_test ... ok\ntest other::t ... ok\ntest result: ok. 2 passed\n"

    with tempfile.TemporaryDirectory() as td:
        def run_with(text, env_log=True):
            path = os.path.join(td, "gate-probe.log")
            io.open(path, "w", encoding="utf-8").write(text)
            os.environ["GATE_LOG"] = path
            try:
                return _passed_tests(), None
            except EvidenceChannelBroken as e:
                return "RAISED", e
            finally:
                os.environ.pop("GATE_LOG", None)

        # B. TRUNCATED log: test output present, no marker after it. Cut BEFORE the cited test's
        #    `ok` line, or the probe passes for the wrong reason (the name would be in the partial
        #    set anyway) and proves nothing.
        res, _ = run_with("test other::t ... ok\ntest result: ok. 1 passed\n")
        if res == "RAISED":
            ok("a truncated log under GATE_LOG is a HARD ERROR, not silent evidence")
        else:
            bad(f"a truncated log yielded {res!r} instead of raising — this is the #1224 false FAIL")

        # C. --quick / self-test shaped log: NO `test result:` lines at all. This used to return an
        #    EMPTY SET, which reports every enforced binding as 'did not run' with no race needed.
        os.environ.pop("GATE_LOG", None)
        path = os.path.join(td, "gate-quick.log")
        io.open(path, "w", encoding="utf-8").write("=== cargo fmt --check: ok ===\n" + sample + "\n")
        if _log_is_complete(path):
            bad("a log with no test output was judged a completed test run")
        else:
            ok("a --quick/self-test shaped log is not mistaken for test evidence")

        # D. COMPLETE log is accepted, and the cited name is actually found. Without this every
        #    assertion above is satisfied by a checker that rejects everything.
        res, _ = run_with(body + sample + "\n")
        if isinstance(res, set) and "a_cited_test" in res:
            ok("a complete log is accepted and its passed tests are read (positive control)")
        else:
            bad(f"a complete log yielded {res!r} — the checks above prove nothing")

        # E. COMPLETE log whose cited test is ABSENT must still yield a set WITHOUT it, so
        #    CITED-BUT-DIDN'T-RUN can still fire. A fix that turned the check into a no-op would
        #    pass every probe above and fail this one.
        res, _ = run_with("test other::t ... ok\ntest result: ok. 1 passed\n" + sample + "\n")
        if isinstance(res, set) and "a_cited_test" not in res and "t" in res:
            ok("a completed run that did not include the cited test still reports it as not-run")
        else:
            bad(f"a completed run missing the cited test yielded {res!r} — the check is a no-op")

    # F. An INVALID gate run (#1151: tree or HEAD moved under it) must not be evidence, even when
    #    its log is complete. Measured before the check existed: it WAS accepted. The PASS control
    #    is what stops this from being satisfied by refusing every verdict.
    import json as _json, tempfile as _tf, os as _os
    verdict = ROOT / "target" / "gate-verdict.json"
    saved = verdict.read_text(encoding="utf-8") if verdict.exists() else None
    complete = None
    for cand in sorted(glob.glob(str(ROOT / "target" / "gate-*.log")), key=_os.path.getmtime):
        if _log_is_complete(cand):
            complete = cand
    if complete is None:
        bad("no complete gate log in target/ — run a full gate before trusting this probe")
    else:
        try:
            here = _current_toolchain()
            for label, fields, want_log in (
                ("INVALID", {"result": "INVALID", "toolchain": here}, False),
                ("PASS", {"result": "PASS", "toolchain": here}, True),
                # A verdict is attributable to (tree, HEAD, TOOLCHAIN). These two probe the third
                # member, which drifts with no act by anyone — a distro upgrade. The unrecorded
                # case is not hypothetical: every verdict written before gate.sh recorded the
                # toolchain looks exactly like this, and must not be trusted by default.
                ("PASS from another toolchain",
                 {"result": "PASS", "toolchain": "rustc 0.0.0 (not this host)"}, False),
                ("PASS with no toolchain recorded", {"result": "PASS"}, False),
            ):
                fields["log"] = complete
                verdict.write_text(_json.dumps(fields), encoding="utf-8")
                _os.environ.pop("GATE_LOG", None)
                got, why = _evidence_log()
                if (got == complete) is want_log:
                    ok(f"a {label} verdict is {'accepted' if want_log else 'refused'} as evidence")
                else:
                    bad(f"a {label} verdict yielded {got or why!r}")
        finally:
            if saved is not None:
                verdict.write_text(saved, encoding="utf-8")
            elif verdict.exists():
                verdict.unlink()

    print("EVIDENCE-SELF-TEST: " + ("PASS" if rc == 0 else "FAIL"))
    return rc


# ----------------------------------------------------------------------------- check
def do_check(release=False):
    if not YAML.exists():
        sys.stderr.write("trace: no requirements.yaml — it is the hand-maintained source of\n"
                         "truth and cannot be regenerated (#1223); restore it from git.\n"); return 2
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
        # `unwired` is enforced-with-a-dormant-consumer: the binding must still exist and pass, so
        # drift on it FAILS exactly as `enforced` does. Routing it to warns would have made the
        # new state a way to soften an existing requirement, which is the opposite of the point.
        return entry.get("traceability") in ("enforced", "unwired")

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
                    f"{eid}: NO-TRACEABILITY — {kind} has no `traceability:` field. Say which "
                    f"you mean: `enforced` (drift FAILS; the bound capability must not be "
                    f"workspace-dormant), `unwired` (bound and passing, but nothing consumes the "
                    f"capability yet — must name a tracking issue), or `baseline` (drift warns, "
                    f"grandfathered only)."
                )
            elif tr not in TRACEABILITY_VALUES:
                vocab_errors.append(
                    f"{eid}: BAD-TRACEABILITY — `traceability: {tr}` is not one of "
                    f"{'|'.join(TRACEABILITY_VALUES)}."
                )
            elif tr == "unwired" and kind != "requirement":
                vocab_errors.append(
                    f"{eid}: UNWIRED-NOT-FOR-CAPABILITIES — `unwired` is a per-requirement state, "
                    f"decided by the requirement's own binding. A capability's `code:` list mixes "
                    f"wired and unwired files (CAP-68 does), so the state is not well-defined here."
                )
            elif tr == "unwired" and not re.search(r"#\d+", str(entry.get("statement", ""))):
                vocab_errors.append(
                    f"{eid}: UNWIRED-WITHOUT-ISSUE — `traceability: unwired` must name the issue "
                    f"tracking the wiring in its own statement, so the state cannot be a quiet "
                    f"resting place."
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

    # ---- workspace-dormancy join (#1237) -----------------------------------------------------
    # `enforced` used to mean exactly "a binding exists and its test passed". Nothing joined that
    # to whether anything CONSUMES the capability, so a unit test of a module with no callers
    # satisfied the letter completely while the registry was read as a claim about the product.
    # REQ-CTL-04 shipped that way. The rule bans the construct:
    #
    #     an `enforced` requirement's binding must not sit in a workspace-dormant package
    #
    # and the reverse direction keeps `unwired` from becoming a resting place. The reverse is a
    # demand for RECONCILIATION, never an automatic promotion: at package granularity "the package
    # reaches a binary" does NOT imply "this requirement's capability is wired", and wiring one
    # function must not silently upgrade every sibling requirement's claim.
    try:
        pkg_of_dir, prod_rdeps, bin_pkgs = _workspace_graph()
    except CargoUnavailable as e:
        print(f"trace: {e}")
        print("\n  The dormancy join cannot run without cargo metadata, and skipping it would")
        print("  silently drop the newest rule on whichever machine lacks cargo.")
        print("\nTRACE: FAIL")
        return 1
    dormant = _dormant_packages(prod_rdeps, bin_pkgs, set(pkg_of_dir.values()))

    dormancy_errors, unwired_roster = [], []
    for rid, entry in sorted(reqs.items()):
        if not isinstance(entry, dict):
            continue
        tr = entry.get("traceability")
        if tr not in ("enforced", "unwired"):
            continue
        pkgs_for = {}
        for b in binds.get(rid, []):
            pkgs_for[b["file"]] = _package_of(b["file"], pkg_of_dir)
        if not pkgs_for:
            continue  # a missing binding is already reported by the binding checks
        live = {f: p for f, p in pkgs_for.items() if p is not None and p not in dormant}
        if tr == "enforced" and not live:
            where = "; ".join(f"{f} -> {p or 'UNRESOLVED'}" for f, p in sorted(pkgs_for.items()))
            dormancy_errors.append(
                f"{rid}: DORMANT-ENFORCED — every binding sits in a package no chain of normal, "
                f"non-optional dependencies connects to a binary ({where}). A passing test on a "
                f"capability nothing consumes is not enforcement. Wire it, rebind it to the "
                f"capability that IS consumed, or mark it `unwired` naming the tracking issue."
            )
        elif tr == "unwired" and live:
            where = "; ".join(f"{f} -> {p}" for f, p in sorted(live.items()))
            dormancy_errors.append(
                f"{rid}: UNWIRED-BUT-REACHED — marked `unwired`, but a binding's package now "
                f"reaches a binary ({where}). Reconcile deliberately: promote to `enforced` if "
                f"THIS requirement's capability is genuinely wired, or rebind/restate if the "
                f"package went live for an unrelated reason. Reaching a binary is not proof that "
                f"this capability is called."
            )
        elif tr == "unwired":
            unwired_roster.append(
                f"{rid}: {' '.join(str(entry.get('statement', '')).split())[:88]}")

    if dormancy_errors:
        print("trace: workspace-dormancy errors (#1237) — these are not warnings\n")
        for m in dormancy_errors:
            print(f"  {m}")
        print(f"\n  {len(dormancy_errors)} error(s)")
        print("\nTRACE: FAIL")
        return 1

    # Printed on EVERY run, passing included. The state is legal, so the only thing keeping it
    # from being a quiet parking space is that it is loud.
    if unwired_roster:
        print(f"trace: {len(unwired_roster)} requirement(s) `unwired` — bound and passing, but "
              f"nothing consumes the capability yet:")
        for m in unwired_roster:
            print(f"  {m}")
        print()

    # ---- id conformance (#1229) -------------------------------------------------------------
    # Three layers, all by NEGATION: collect loosely, validate strictly, fail on the residue.
    #   1. the yaml's own keys and cross-references
    #   2. `// VERIFIES:` bindings in source — an id the scanner cannot tokenize is unregisterable
    #   3. living docs — shape AND membership, because an id that exists in prose but not in the
    #      source of truth IS the defect, independent of whether its shape happens to conform
    unregistered = set()
    if UNREGISTERED_BASELINE.exists():
        unregistered = {l.strip() for l in UNREGISTERED_BASELINE.read_text().splitlines()
                        if l.strip() and not l.startswith("#")}

    def bad_shape(tok):
        rx = CANON_REQ if tok.startswith("REQ") else CANON_CAP
        return not rx.match(tok)

    id_errors = []
    for kind, table, rx in (("requirement", reqs, CANON_REQ), ("capability", caps, CANON_CAP)):
        for eid, entry in table.items():
            if eid in ID_VOCABULARY:
                continue  # a self-test's planted fixture, not a real entry
            if not rx.match(eid):
                id_errors.append(f"{eid}: BAD-ID-SHAPE — {kind} key does not match the convention "
                                 f"in docs/dev/requirements.md")
            if not isinstance(entry, dict):
                continue
            for field in ("covered_by", "satisfies"):
                for ref in entry.get(field) or []:
                    if bad_shape(ref):
                        id_errors.append(f"{eid}: BAD-ID-SHAPE — `{field}` references `{ref}`")

    for root in ("crates", "plugins", "apps", "tools", "pki-tooling", "docs", "scripts"):
        for path in glob.glob(str(ROOT / root / "**/*"), recursive=True):
            rel = os.path.relpath(path, ROOT)
            if not os.path.isfile(path) or os.path.splitext(path)[1] not in (".rs", ".md", ".py", ".sh", ".toml"):
                continue
            if any(rel.startswith(f) for f in FROZEN_PREFIXES):
                continue
            try:
                txt = pathlib.Path(path).read_text(encoding="utf-8", errors="ignore")
            except OSError:
                continue
            for m in ID_TOKEN.finditer(txt):
                raw = m.group(0)
                if raw in ID_VOCABULARY:
                    continue
                # Expand the `/NN` shorthand (`REQ-CTL-01/02`) against its own prefix.
                # KNOWN BLIND SPOT: this validates each expanded id's EXISTENCE, never whether it
                # is the id the sentence means. `REQ-FX-02/05/05` — a real slip in #1235, where a
                # rename mapping was not 1:1 — expands to registered ids and passes. Membership
                # cannot catch a wrong-but-registered citation; only reading can.
                head, *rest = raw.split("/")
                toks = [head] + [head.rsplit("-", 1)[0] + "-" + r for r in rest]
                for tok in toks:
                    if tok in ID_VOCABULARY:
                        continue
                    if tok.startswith("REQ") and tok.count("-") < 2:
                        continue  # a bare category word ("REQ-SEC requirements")
                    if bad_shape(tok):
                        id_errors.append(f"{rel}: BAD-ID-SHAPE — `{tok}` is not `REQ-<CAT>-NN` / `CAP-NN`")
                    elif tok not in reqs and tok not in caps and tok not in unregistered:
                        id_errors.append(f"{rel}: UNREGISTERED-ID — `{tok}` is not in requirements.yaml")
    if id_errors:
        print("trace: requirement/capability id errors — these are not warnings\n")
        for m in sorted(set(id_errors))[:40]:
            print(f"  {m}")
        extra = len(set(id_errors)) - 40
        if extra > 0:
            print(f"  ... and {extra} more")
        print(f"\n  {len(set(id_errors))} error(s)")
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

    try:
        passed = _passed_tests()  # None => no usable gate run to confirm against
        _, evidence_note = _evidence_log()
    except EvidenceChannelBroken as e:
        sys.stderr.write(f"trace: {e}\n")
        sys.stderr.write("trace: refusing to report run-status against a log the gate says is "
                         "complete and is not.\n")
        print("TRACE: FAIL")
        return 2
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
        # Say WHICH absence this is. The old text claimed "no gate run found" while several logs sat
        # in target/, so a reader could take it as "nothing to see" when the real cause was that
        # every candidate was truncated or was a --quick/self-test log (#1224).
        print(f"  NOTE: {evidence_note} — run-status of enforced bindings unverified "
              f"(confirmed inside scripts/gate.sh, which exports GATE_LOG)")
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


def do_graph_selftest():
    """Probe the two resolution defects the dormancy join's evidence was produced WITH (#1240).

    The join's other probes (`DORMANT-ENFORCED`, `UNWIRED-BUT-REACHED`) exercise its VERDICTS. These
    two exercise the graph underneath them — the pair that was wrong in the throwaway script that
    produced #1237's evidence, and that adversarial review caught by reading rather than running,
    because the headline numbers were identical before and after the fix.

    Both are written as PROPERTIES over the workspace, and each asserts its own discriminating
    population is non-empty first. A probe that silently stops discriminating is the failure mode
    here: if every plugin were renamed to match its directory, a fixed-example probe would keep
    passing while testing nothing.
    """
    rc = 0

    def ok(msg):
        print(f"  ok: {msg}")

    def bad(msg):
        nonlocal rc
        print(f"  GRAPH-SELF-TEST FAIL: {msg}")
        rc = 1

    try:
        pkg_of_dir, rdeps, bin_pkgs = _workspace_graph()
    except CargoUnavailable as e:
        print(f"  GRAPH-SELF-TEST FAIL: {e}")
        print("GRAPH-SELF-TEST: FAIL")
        return 1

    # ---- defect 1: a crate resolved by DIRECTORY name -----------------------------------------
    # `plugins/64qam` is package `qam64-plugin`. Resolving by path segment made every plugin
    # binding look unreachable — a false-FAIL machine for the whole layer, invisible only because
    # all current bindings live where directory and package name coincide.
    divergent = [(d, name) for d, name in pkg_of_dir.items()
                 if os.path.basename(d) != name]
    if not divergent:
        bad("no package's directory differs from its name — this probe can no longer discriminate; "
            "re-derive it or delete it rather than leaving it green")
    else:
        wrong = [(d, name) for d, name in divergent
                 if _package_of(os.path.join(os.path.relpath(d, str(ROOT)), "src", "lib.rs"),
                                pkg_of_dir) != name]
        if wrong:
            bad(f"{len(wrong)} package(s) resolved to the wrong name by path, e.g. {wrong[0]}")
        else:
            ok(f"{len(divergent)} package(s) whose directory != name still resolve correctly "
               f"(e.g. {os.path.basename(divergent[0][0])} -> {divergent[0][1]})")

    # ---- defect 2: optional / dev edges counted as production reach ----------------------------
    # `openpulse-gpu` is depended on ONLY through `optional = true` edges. Counting those as reach
    # would let a requirement bound to a CPU-fallback test pass this join while the real path never
    # executes in any gated build.
    dormant = _dormant_packages(rdeps, bin_pkgs, set(pkg_of_dir.values()))
    optional_only = _optional_only_packages()
    if not optional_only:
        bad("no package is depended on only through optional edges — this probe can no longer "
            "discriminate; re-derive it or delete it rather than leaving it green")
    else:
        leaked = sorted(p for p in optional_only if p not in dormant)
        if leaked:
            bad(f"optional-only edges conferred production reach on {leaked}")
        else:
            ok(f"{len(optional_only)} optional-only package(s) are dormant "
               f"(e.g. {sorted(optional_only)[0]})")

    print("GRAPH-SELF-TEST: " + ("PASS" if rc == 0 else "FAIL"))
    return rc


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "scope":
        return do_scope(sys.argv[2]) if len(sys.argv) > 2 else 2
    if cmd == "check":
        return do_check(release="--release" in sys.argv[2:])
    if cmd == "evidence-self-test":
        return do_evidence_selftest()
    if cmd == "graph-self-test":
        return do_graph_selftest()
    sys.stderr.write(
        f"usage: trace.py {{check|evidence-self-test|graph-self-test}}\n"); return 2


if __name__ == "__main__":
    sys.exit(main())
