#!/usr/bin/env python3
"""Turn cargo-nextest's JUnit report into the libtest-shaped lines the gate's readers consume.

Why this exists. `scripts/gate.sh` counts tests from `test result:` lines and `trace.py` reads which
tests passed from `test <path> ... ok` lines — both the format `cargo test` prints. nextest prints a
different, human-oriented format whose captured failure output ALSO contains libtest lines (each
test runs as `<binary> --exact <name>`, and a failing test's stdout is replayed into the log). So
the counts must not be scraped from nextest's terminal output: they come from its JUnit report,
which is the stable machine-readable record, and are re-emitted here in exactly libtest's shape.

Usage:
  nextest_junit.py report <junit.xml> [N]  # libtest-shaped lines on stdout; exit 1 if any failed,
                                           # or if N is given and the report holds != N tests
  nextest_junit.py failures <junit.xml>    # one `<binary-id> <test>` per failed test, complete

Exit 2 when the report is missing or unparseable, or holds a SKIPPED testcase — the gate must not
read "no report" as "no failures", nor a test that did not run as one that passed.
"""

import sys
import xml.etree.ElementTree as ET

# A testcase is failed when it carries either element. nextest writes `<failure>` for a test that
# ran and failed and `<error>` for one that could not be run properly (crash, exec failure); both
# are failures for the gate. `flakyFailure`/`rerunFailure` are retry records and cannot appear with
# `retries = 0`, but a final verdict is still the absence of `failure`/`error`, never their count.
_FAILED_TAGS = ("failure", "error")


class SkippedInReport(ValueError):
    """The report lists a testcase that did not run.

    nextest writes `<skipped/>` cases only when `junit.report-skipped` asks for them (default
    "none"; the gate profile does not set it). If that ever changes, a skipped case has neither
    `failure` nor `error`, and treating it like the others would render every `#[ignore]`d test —
    the held-out REQ-QRM-01 suite included — as `test X ... ok`, which trace.py accepts as evidence.
    Refused rather than dropped, so the change is noticed (review 2026-11-08, finding 1).
    """


def parse(path):
    """[(binary_id, [(test_name, passed_bool), ...]), ...] in report order."""
    root = ET.parse(path).getroot()
    suites = root.iter("testsuite") if root.tag != "testsuite" else [root]
    out = []
    for s in suites:
        cases = []
        for c in s.iter("testcase"):
            if c.find("skipped") is not None:
                raise SkippedInReport(
                    f"{s.get('name')} {c.get('name')} is a SKIPPED testcase in the JUnit report; "
                    f"the gate profile must not set junit.report-skipped")
            failed = any(c.find(t) is not None for t in _FAILED_TAGS)
            cases.append((c.get("name"), not failed))
        out.append((s.get("name"), cases))
    return out


def render(suites):
    """libtest-shaped text for `suites` (the format `cargo test` prints, one block per binary)."""
    lines = []
    for binary, cases in suites:
        passed = sum(1 for _, ok in cases if ok)
        failed = len(cases) - passed
        lines.append(f"     Running nextest {binary}")
        lines.append("")
        lines.append(f"running {len(cases)} test{'' if len(cases) == 1 else 's'}")
        for name, ok in cases:
            lines.append(f"test {name} ... {'ok' if ok else 'FAILED'}")
        lines.append("")
        if failed:
            lines.append("failures:")
            for name, ok in cases:
                if not ok:
                    lines.append(f"    {name}")
            lines.append("")
        verdict = "FAILED" if failed else "ok"
        # No `ignored`/`filtered out` fields: JUnit does not carry them, and inventing a 0 would be
        # a false number. The gate reads only the `passed;` and `failed;` tokens.
        lines.append(f"test result: {verdict}. {passed} passed; {failed} failed; (nextest JUnit)")
        lines.append("")
    return "\n".join(lines) + "\n"


def main(argv):
    if not (len(argv) == 3 or (len(argv) == 4 and argv[1] == "report")) \
            or argv[1] not in ("report", "failures"):
        print(__doc__, file=sys.stderr)
        return 2
    try:
        suites = parse(argv[2])
    except (OSError, ET.ParseError, SkippedInReport) as e:
        print(f"nextest_junit: cannot read the JUnit report {argv[2]}: {e}", file=sys.stderr)
        return 2
    if argv[1] == "failures":
        for binary, cases in suites:
            for name, ok in cases:
                if not ok:
                    print(f"{binary} {name}")
        return 0
    sys.stdout.write(render(suites))
    total_failed = sum(1 for _, cases in suites for _, ok in cases if not ok)
    total = sum(len(cases) for _, cases in suites)
    print(f"nextest JUnit: {len(suites)} binaries, {total} tests, {total_failed} failed")
    if len(argv) == 4:
        # The run must have EXECUTED what was selected. A report with fewer tests (a cancelled
        # run, a scheduler that stopped) or none at all must not reach `GATE: PASS` on a zero.
        if not argv[3].isdigit():
            print(f"nextest_junit: expected count {argv[3]!r} is not a number", file=sys.stderr)
            return 2
        if total != int(argv[3]):
            print(f"nextest JUnit: FAIL — {total} tests in the report, {argv[3]} selected")
            return 1
        print(f"nextest JUnit: all {total} selected tests are in the report")
    return 1 if total_failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
