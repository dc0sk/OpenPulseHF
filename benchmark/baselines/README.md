---
project: openpulsehf
doc: benchmark/baselines/README.md
status: living
last_updated: 2026-10-15
---

# Benchmark Baselines

This directory stores baseline aggregate benchmark artifacts used for regression checks.

Expected file type:

- JSON aggregate report objects compatible with `docs/dev/benchmark-harness.md`.

Required key fields for matching:

- scenario_id
- mode_under_test

Suggested naming:

- <scenario_id>--<mode_under_test>.json

Example (the only baseline committed today):

- HF2300-WATTERSON-F1-ITEM6--VARA-HF-REFERENCE.json

Item 6 direct parity reference:

- `HF2300-WATTERSON-F1-ITEM6--VARA-HF-REFERENCE.json` captures the VARA reference point used by Item 6 parity tracking (7536 bps, 1.25 s cycle envelope).
- The matching scenario descriptor is `benchmark/scenarios/HF2300-WATTERSON-F1-ITEM6.yaml`.

Notes:

- `scripts/check-benchmark-regressions.sh benchmark/baselines benchmark/results/aggregate` compares
  each candidate aggregate in `benchmark/results/aggregate/` against the baseline with the same
  `scenario_id` and `mode_under_test`. It fails on `success_rate` below 0.95, a `median_goodput_bps`
  drop of more than 5 %, or a `p95_completion_time_ms` rise of more than 10 %.
- The "Benchmark gates" workflow (`.github/workflows/benchmark.yml`) runs that script and
  `scripts/validate-benchmark-artifacts.sh` on every pull request. Both read **committed** JSON;
  nothing is measured in CI, so the check moves only when someone commits a new aggregate. The
  committed VARA-reference candidate (median goodput 7300 bps, p95 1300 ms) sits within the 5 % / 10 %
  bounds of its baseline.
- The CLI benchmark gate (`openpulse benchmark run`, `passed == total` and `mean_transitions <= 20`)
  is a separate job in `.github/workflows/ci.yml` and does not use this directory.
- Missing candidate files are treated as skip.
- Missing baselines are warning-only by default, and can be made mandatory with `REQUIRE_BASELINE=1`.
  `HF2300-AWGN30-ITEM6--SCFDMA52-64QAM-P4.json` in `results/aggregate/` has no baseline today, so it
  produces a warning.
