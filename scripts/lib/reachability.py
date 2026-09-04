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

# Bare `pub` ONLY — not `pub(crate)`/`pub(super)`/`pub(in ...)`. The orphan test excludes an item's
# own declaring file, which is the right rule for CROSS-CRATE reachability and the wrong one for a
# restricted item, whose entire legitimate scope may be that file. The case that needs this is
# `FX_CONTROL_SEGMENT_ID` (openpulse-daemon/src/filexfer.rs): production use in its own file plus
# test use in other files of the same crate, so it MUST be `pub(crate)` and would be reported as
# having no production caller however it is written.
#
# What is lost, stated rather than waved away: rustc's `dead_code` lint covers an unused restricted
# item under the workspace's `-D warnings` — verified by planting one, including the case where its
# only use is a `#[cfg(test)]` module in another file, because `--all-targets` still checks the
# plain lib where no test module exists. It does NOT cover an item silenced by `#[allow(dead_code)]`
# or one in code this gate's `--no-default-features` build never compiles. That set is EMPTY today
# (9 allow sites in production src, none on a restricted item; all 41 packages under SRC_GLOBS are
# workspace members), which is a fact about now, not a property — re-check it if allow sites grow.
PUB_ITEM = re.compile(r"^\s*pub\s+(?:fn|struct|enum|trait|const|static|type)\s+([A-Za-z_][A-Za-z0-9_]+)", re.M)
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]+")


def _strip_rust(text):
    r"""Blank comments and string literals, preserving length so offsets survive.

    WHY: `IDENT.findall` cannot tell a call from an English sentence, so before this a doc-comment
    mention made an item look reachable and rewording that comment made it look like a NEW orphan
    (#1192 — that is exactly how it surfaced, on a comment rewrite during #1147). Two evasion
    classes followed: a new item named in any sibling's doc comment passed silently, and an item
    whose name is an ordinary English word (`frequency`, `invalid`, `fast`) was effectively
    immune — it could fail only if NO comment or string anywhere else in the tree contained the
    word, which for a common word is vanishingly unlikely though not impossible.

    Regex cannot do this. Rust has NESTED block comments, raw strings with arbitrary hash counts,
    and lifetimes that are indistinguishable from a char literal at the opening quote. Removed
    bytes become spaces (newlines kept) so PUB_ITEM's `^\s*pub` anchor still works.
    """
    out = list(text)
    i, n = 0, len(text)

    def blank(a, b):
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = text[i]

        if c == "/" and i + 1 < n and text[i + 1] == "/":          # line comment
            j = text.find("\n", i)
            j = n if j == -1 else j
            blank(i, j); i = j; continue

        if c == "/" and i + 1 < n and text[i + 1] == "*":          # block comment, NESTED
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j): depth += 1; j += 2
                elif text.startswith("*/", j): depth -= 1; j += 2
                else: j += 1
            blank(i, j); i = j; continue

        if c in "rb":                                              # raw string r"" r#""# br##""##
            k = i
            if text[k] == "b" and k + 1 < n and text[k + 1] == "r":
                k += 1
            if text[k] == "r":
                h = k + 1
                while h < n and text[h] == "#":
                    h += 1
                if h < n and text[h] == '"':
                    close = '"' + "#" * (h - (k + 1))
                    j = text.find(close, h + 1)
                    j = n if j == -1 else j + len(close)
                    blank(i, j); i = j; continue

        if c == '"' or (c == "b" and i + 1 < n and text[i + 1] == '"'):   # string / byte string
            j = i + (2 if c == "b" else 1)
            while j < n:
                if text[j] == "\\": j += 2; continue
                if text[j] == '"': j += 1; break
                j += 1
            blank(i, j); i = j; continue

        if c == "'" or (c == "b" and i + 1 < n and text[i + 1] == "'"):   # char vs LIFETIME
            q = i + 1 if c == "b" else i
            if text.startswith("\\", q + 1):
                # Escaped char. Search from q+3: the escape consumes two characters, so the closing
                # quote can never sit at q+2 — searching from q+2 makes `'\''` match its own
                # escaped quote and leaves a stray `'` that can blank real code.
                j = text.find("'", q + 3)
                j = n if j == -1 else j + 1
                blank(i, j); i = j; continue
            if q + 2 < n and text[q + 2] == "'":                   # 'x'  — a real char literal
                blank(i, q + 3); i = q + 3; continue
            i += 1; continue                                       # 'a   — a lifetime; leave it

        i += 1
    return "".join(out)


# `#[cfg(test)]` and the compound forms — `#[cfg(all(test, unix))]`,
# `#[cfg(all(test, not(target_arch = "wasm32")))]`, `#[cfg(all(test, feature = "serial"))]`.
# Matching only the bare form treated five test modules as PRODUCTION, which is the false-PASS
# direction: their contents counted as production callers, and their own `pub(super)` helpers were
# collected as public items with no caller. Found when the ratchet flagged three such helpers in
# `openpulse-daemon` during #1252. `not(` is excluded so a `#[cfg(not(test))]` block — production
# code that compiles only outside tests — is never stripped; there are none today, and stripping one
# would delete real production text.
_CFG_OPEN_RE = re.compile(r"#\[cfg\(")


def _cfg_expr_is_test(expr):
    """True when a `cfg(...)` expression selects TEST builds — `test`, or `all(...)` containing it.

    A predicate rather than one regex, because "a bare `test` atom anywhere inside `all(...)` except
    under `not(...)`" is not something a Python regex expresses (the `not(` guard needs a
    variable-width lookbehind). Attempting it produced a pattern that matched the bare form and
    silently stopped matching every compound one — the failure this function exists to prevent.

    Deliberately FALSE for `any(test, ...)`: that compiles in production whenever another arm holds,
    so stripping it would delete real production declarations and callers. Also false for
    `not(test)`, which is production-only code.
    """
    expr = expr.strip()
    if expr == "test":
        return True
    if not expr.startswith("all(") or not expr.endswith(")"):
        return False
    inner, depth, out = expr[4:-1], 0, []
    for ch in inner:                       # blank out nested groups, keeping top-level atoms
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif depth == 0:
            out.append(ch)
        if depth < 0:
            return False
    return "test" in [a.strip() for a in "".join(out).split(",")]


def _cfg_span(text, start):
    """Given the index of `#[cfg(`, return (expr, index just past the closing `]`) or None."""
    open_paren = text.index("(", start)
    depth, i, n = 0, open_paren, len(text)
    while i < n:
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                close = text.find("]", i)
                return (None if close == -1 else (text[open_paren + 1 : i], close + 1))
        i += 1
    return None


def _strip_cfg_test(text):
    """Remove `#[cfg(test)] mod ... { ... }` blocks so test-only use is not a production caller."""
    out, i, n = [], 0, len(text)
    while i < n:
        m = _CFG_OPEN_RE.search(text[i:])
        if not m:
            out.append(text[i:]); break
        start = i + m.start()
        span = _cfg_span(text, start)
        if span is None or not _cfg_expr_is_test(span[0]):
            # A cfg that is not a test cfg: keep it and resume PAST it, so its own parentheses
            # cannot be mistaken for the next candidate's.
            end = start + len("#[cfg(") if span is None else span[1]
            out.append(text[i:end]); i = end; continue
        out.append(text[i:start])
        brace = text.find("{", start)
        semi = text.find(";", start)
        # `#[cfg(test)] use path::to::thing;` and `#[cfg(test)] mod x;` have no brace block. Both
        # occur here (scfdma/demodulate.rs, mfsk16/lib.rs). Cutting at the brace instead swallows
        # everything up to the next `{` in the file — and once comments are stripped that brace is
        # no longer the doc-comment brace that happened to balance it, so the matcher eats the next
        # real item. Measured: it deleted `mix_to_nominal` from the survey entirely.
        # LIVE EDGE, not hypothetical: `#[cfg(test)] const RESERVED_MAGICS`/`FOREIGN_MAGICS` in
        # signing_domain.rs cut early at the `;` inside `&[(&[u8; TAG_LEN]…`, leaving the const's
        # tail in production text. It leaks only `TAG_LEN`/`u8`/`str` today — but the leak is in
        # the FALSE-PASS direction, which is the one that matters here, so it is a known cost
        # rather than a non-issue. Fixing it needs brace/bracket-aware scanning, not a wider regex.
        if semi != -1 and (brace == -1 or semi < brace):
            i = semi + 1
            continue
        if brace == -1:
            break
        depth, j = 1, brace + 1
        while j < n and depth:
            if text[j] == "{": depth += 1
            elif text[j] == "}": depth -= 1
            j += 1
        i = j  # skip the whole block
    return "".join(out)


def _self_check():
    r"""Prove the lexer strips what it must and keeps what it must, then prove it is WIRED.

    Two halves, and the second is the one that matters. The first asserts the stripper's behaviour
    in isolation. The second re-runs the real analysis with the stripper disabled and requires the
    orphan count to CHANGE — so a refactor that silently stops calling `_strip_rust` fails here
    instead of quietly restoring the false-PASS class it was written to close.
    """
    strip_cases = [
        ("let x = 5; // mentions verify_pq_conreq", "verify_pq_conreq"),
        ("/* block naming symbol_name */ let y = 1;", "symbol_name"),
        ("/* outer /* nested inner_sym */ still */ ok", "inner_sym"),
        ('let s = "string_ident";', "string_ident"),
        ('let r = r#"raw_ident"#;', "raw_ident"),
        ("/// doc comment naming doc_sym", "doc_sym"),
        ("//! module doc naming mod_sym", "mod_sym"),
        ('#[doc = "attr_sym"]', "attr_sym"),
    ]
    keep_cases = [
        ("fn f<'a>(x: &'a Foo) -> Bar { baz() }", ["Foo", "Bar", "baz"]),
        ("let c = 'x'; let v = real_call();", ["real_call"]),
        ("let b = b'\"'; then_call();", ["then_call"]),
        (r"let e = '\''; after_call();", ["after_call"]),
        ("'outer: loop { labelled_call(); break 'outer; }", ["labelled_call"]),
    ]
    # The CFG stripper is a SECOND stripper with a separate failure mode, and until #1270 it had no
    # probe at all: the "stripper is WIRED" line below disables `_strip_rust`, so a regression of the
    # cfg matcher back to the literal `#[cfg(test)]` form passed both self-tests. That regression had
    # already shipped — five compound-cfg test modules were treated as production for the life of the
    # ratchet. `any(test, ...)` is in the KEEP list on purpose: it compiles in production whenever
    # another arm holds, so stripping it would delete real declarations.
    cfg_strip_cases = [
        "test",
        "all(test, unix)",
        'all(test, not(target_arch = "wasm32"))',
        'all(test, feature = "serial")',
        'all(feature = "x", test)',
        'all(unix, test, feature = "y")',
    ]
    cfg_keep_cases = [
        "not(test)",
        "all(not(test), unix)",
        'feature = "test-util"',
        'any(test, feature = "x")',
        'all(not(test), feature = "z")',
        "test_util",
        'all(feature = "test-util", unix)',
    ]

    bad = []
    for expr in cfg_strip_cases:
        if not _cfg_expr_is_test(expr):
            bad.append(f"cfg NOT recognised as test-only: {expr!r}")
    for expr in cfg_keep_cases:
        if _cfg_expr_is_test(expr):
            bad.append(f"cfg WRONGLY recognised as test-only: {expr!r}")
    for src, sym in strip_cases:
        if sym in _strip_rust(src):
            bad.append(f"NOT stripped: {sym!r} survives {src!r}")
    for src, syms in keep_cases:
        got = _strip_rust(src)
        for sym in syms:
            if sym not in got:
                bad.append(f"WRONGLY stripped: {sym!r} lost from {src!r}")

    # Structural invariant over the real tree: in valid Rust every quote belongs to a string and
    # delimiters balance outside comments/strings, so a lexer that enters or leaves a mode at the
    # wrong place trips one of these.
    for rel in _src_files():
        try:
            txt = _strip_rust((ROOT / rel).read_text(encoding="utf-8", errors="ignore"))
        except OSError:
            continue
        if '"' in txt or "//" in txt or "/*" in txt:
            bad.append(f"leftover comment/string delimiter after stripping: {rel}")
            break

    if bad:
        print("SELF-CHECK: FAIL")
        for b in bad:
            print("  " + b)
        return 1

    # The wiring half: disabling the stripper must move the number.
    real = globals()["_strip_rust"]
    globals()["_strip_rust"] = lambda t: t
    try:
        _, items_off, orphans_off, _ = _analyze()
    finally:
        globals()["_strip_rust"] = real
    _, items_on, orphans_on, _ = _analyze()

    if len(orphans_off) == len(orphans_on):
        print("SELF-CHECK: FAIL")
        print(f"  disabling the stripper did not change the orphan count ({len(orphans_on)}).")
        print("  Either it is no longer wired into _analyze, or the tree has no prose references")
        print("  left — check which before trusting this.")
        return 1
    if len(items_off) != len(items_on):
        print("SELF-CHECK: FAIL")
        print(f"  the stripper changed the PUBLIC ITEM count ({len(items_off)} -> {len(items_on)}).")
        print("  It must never manufacture or destroy a declaration, only references.")
        return 1

    # Same wiring probe for the cfg stripper. Unlike `_strip_rust` this one legitimately changes the
    # ITEM count too — a `#[cfg(test)] pub fn` is a declaration that must not be surveyed — so only
    # the orphan count is required to move.
    real_cfg = globals()["_strip_cfg_test"]
    globals()["_strip_cfg_test"] = lambda t: t
    try:
        _, _, orphans_cfg_off, _ = _analyze()
    finally:
        globals()["_strip_cfg_test"] = real_cfg
    if len(orphans_cfg_off) == len(orphans_on):
        print("SELF-CHECK: FAIL")
        print(f"  disabling the CFG stripper did not change the orphan count ({len(orphans_on)}).")
        print("  Either it is no longer wired into _analyze, or no test module in the tree")
        print("  references a public item — check which before trusting this.")
        return 1

    print(f"  ok: {len(strip_cases)} stripped, {sum(len(k[1]) for k in keep_cases)} preserved, "
          f"no leftover delimiters in {len(_src_files())} files")
    print(f"  ok: {len(cfg_strip_cases)} cfg exprs recognised, {len(cfg_keep_cases)} refused")
    print(f"  ok: lexer is WIRED — orphans {len(orphans_off)} (off) vs {len(orphans_on)} (on); "
          f"public items unchanged at {len(items_on)}")
    print(f"  ok: cfg stripper is WIRED — orphans {len(orphans_cfg_off)} (off) vs "
          f"{len(orphans_on)} (on)")
    print("SELF-CHECK: PASS")
    return 0


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
        prod = _strip_cfg_test(_strip_rust(raw))
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
    """Regenerate the baseline, PRESERVING the comment lines already in it.

    Regeneration used to overwrite the whole file with a fixed three-line header, silently
    destroying every annotation somebody had written — including a `DORMANT(#1118)` block recording
    WHY three items are deliberately unreferenced. Losing that turns a documented decision back into
    an unexplained entry, and nothing would have reported the loss.
    """
    _, _, orphans, _ = _analyze()

    header = [
        "# Public items with no production caller at the bright line — grandfathered.",
        "# A NEW unreferenced public item (not listed here) fails the build. Shrink this list.",
        "# Format: <item-name><TAB><declaring-file>",
    ]

    # Drop the previous header by FULL-LINE EQUALITY against the list emitted above — shared by
    # reference, never by substring. An earlier version matched on the substrings "Format:",
    # "grandfathered" and "fails the build", which silently destroyed any annotation using the
    # ratchet's own vocabulary: an annotation reading "grandfathered pending #9999" vanished with
    # no report. Equality cannot do that — a lost line would have to be byte-identical to a header —
    # and if the header text ever changes, stale headers ACCUMULATE VISIBLY instead of eating
    # annotations. Anything dropped is printed, so no loss is ever silent.
    kept, dropped = [], []
    if BASELINE.exists():
        for line in BASELINE.read_text(encoding="utf-8").splitlines():
            if not line.startswith("#"):
                continue
            (dropped if line in header else kept).append(line)
    BASELINE.write_text("\n".join(header + kept + orphans) + "\n", encoding="utf-8")
    print(f"baseline: {len(orphans)} entries ({len(kept)} annotation line(s) preserved, "
          f"{len(dropped)} previous header line(s) replaced) -> {BASELINE.relative_to(ROOT)}")
    for d in dropped:
        if d not in header:  # unreachable today; prints if the header text ever drifts
            print(f"  dropped non-header comment: {d}")
    return 0


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "report"
    if cmd == "report":   report(); return 0
    if cmd == "check":    return check()
    if cmd == "baseline": return write_baseline()
    if cmd == "self-check": return _self_check()
    sys.stderr.write("usage: reachability.py {report|check|baseline|self-check}\n"); return 2


if __name__ == "__main__":
    sys.exit(main())
