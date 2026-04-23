# Benchmark Baselines

This directory stores baseline aggregate benchmark artifacts used for regression checks.

Expected file type:

- JSON aggregate report objects compatible with docs/benchmark-harness.md.

Required key fields for matching:

- scenario_id
- mode_under_test

Suggested naming:

- <scenario_id>--<mode_under_test>.json

Example:

- HF500-NOM-01--HPX500.json

Notes:

- Regression checks compare candidate aggregate results under benchmark/results/aggregate against matching baseline objects in this directory.
- Missing candidate files are treated as skip.
- Missing baselines are warning-only by default, and can be made mandatory with REQUIRE_BASELINE=1.
