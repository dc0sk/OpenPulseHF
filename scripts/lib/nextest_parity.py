#!/usr/bin/env python3
"""Prove nextest runs every test `cargo test` would run — per binary, per test name.

Why this exists. `scripts/gate.sh` runs the workspace tests through cargo-nextest, which selects
tests by its own rules (a `default-filter`, test-group overrides, how it lists a binary), while the
contract the gate inherited is `cargo test --workspace --no-default-features`. A test nextest does
not run is a coverage loss that NO pass/fail count can show: the run is green and smaller. So the
gate compares the two selections directly, from the binaries themselves:

  * cargo's selection: every test executable `cargo test --no-run` builds, each asked
    `--list --format terse` (all tests) minus `--list --format terse --ignored` (the `#[ignore]`d
    ones, which `cargo test` does not run either);
  * nextest's: `cargo nextest list --profile gate`, tests whose filter-match status is `matches`.

Keyed by binary PATH (both tools run the same file from the same build), so two binaries with a
same-named test cannot vouch for each other. Doctests are outside both sides — nextest does not run
them, and the gate runs them with `cargo test --doc`.

Usage:
  nextest_parity.py check            # exit 0 when the selections are equal, 1 with the diff
  nextest_parity.py nextest-count    # print how many tests nextest would run (for the self-test)
"""

import json
import os
import subprocess
import sys

CARGO_ARGS = ["--workspace", "--no-default-features"]
PROFILE = "gate"


def _run(cmd, cwd=None):
    p = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    if p.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} exited {p.returncode}:\n{p.stderr[-4000:]}")
    return p.stdout


def nextest_selection():
    """{binary_path: set(test names nextest would run)}."""
    out = _run(["cargo", "nextest", "list", *CARGO_ARGS, "--profile", PROFILE,
                "--message-format", "json"])
    data = json.loads(out)
    sel = {}
    for _bid, suite in data["rust-suites"].items():
        names = sel.setdefault(suite["binary-path"], set())
        for name, case in suite["testcases"].items():
            if case["filter-match"]["status"] == "matches":
                names.add(name)
    return sel


def _listed(exe, cwd, ignored):
    args = [exe, "--list", "--format", "terse"] + (["--ignored"] if ignored else [])
    names = set()
    for line in _run(args, cwd=cwd).splitlines():
        # `<name>: test` — `<name>: bench` is a benchmark, which `cargo test` runs only as a
        # smoke test on nightly; there are none on stable, and if one appears it shows up as a
        # parity difference rather than being silently dropped here.
        if line.endswith(": test"):
            names.add(line[: -len(": test")])
        elif line.endswith(": bench"):
            names.add(line[: -len(": bench")] + " [bench]")
    return names


def cargo_selection():
    """{binary_path: set(test names `cargo test` would run)}."""
    out = _run(["cargo", "test", *CARGO_ARGS, "--no-run", "--message-format", "json"])
    sel = {}
    for line in out.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") != "compiler-artifact" or not msg.get("executable"):
            continue
        if not msg.get("profile", {}).get("test"):
            continue  # a bin built for an integration test to spawn, not a test harness
        exe = msg["executable"]
        cwd = os.path.dirname(msg["manifest_path"])
        sel[exe] = _listed(exe, cwd, False) - _listed(exe, cwd, True)
    return sel


def check():
    cargo, nextest = cargo_selection(), nextest_selection()
    diffs = []
    for exe in sorted(set(cargo) | set(nextest)):
        c, n = cargo.get(exe, set()), nextest.get(exe, set())
        for name in sorted(c - n):
            diffs.append(f"  cargo test only (NOT run by nextest): {os.path.basename(exe)} {name}")
        for name in sorted(n - c):
            diffs.append(f"  nextest only (not run by cargo test): {os.path.basename(exe)} {name}")
    total_c = sum(len(v) for v in cargo.values())
    total_n = sum(len(v) for v in nextest.values())
    print(f"cargo test would run {total_c} tests in {len(cargo)} binaries; "
          f"nextest would run {total_n} tests in {len(nextest)} binaries")
    if diffs:
        print(f"PARITY: FAIL — {len(diffs)} difference(s), complete list:")
        print("\n".join(diffs))
        return 1
    print("PARITY: PASS — identical per-binary test selections")
    return 0


def main(argv):
    if len(argv) != 2 or argv[1] not in ("check", "nextest-count"):
        print(__doc__, file=sys.stderr)
        return 2
    try:
        if argv[1] == "nextest-count":
            print(sum(len(v) for v in nextest_selection().values()))
            return 0
        return check()
    except (RuntimeError, ValueError, KeyError) as e:
        print(f"nextest_parity: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
