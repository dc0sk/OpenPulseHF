---
project: openpulsehf
doc: docs/releasenotes.md
status: living
last_updated: 2026-05-16
---

# Release Notes

## Unreleased

- Bandplan guardrails now recognize active `-RRC` waveform variants and
  `SCFDMA52-64QAM-P4` in occupied-bandwidth checks, preventing valid
  transmissions from being rejected as unknown operating modes.
- `BandplanPolicy::default()` now uses `HamIaruRegion1` instead of the
  deprecated `HamIaru` ruleset variant.
- Region 3 bandplan validation now exposes an explicit warning path when it
  uses Region 1 allocations as a conservative proxy.
- TX compliance logs now reject cross-station frame metadata instead of
  silently mixing different callsigns into a single session log.
- Session metrics now publish throughput as an explicit upper-bound proxy and
  include a dedicated note field to avoid interpreting it as exact payload
  throughput.

- Added BL-TP-7 SC-FDMA pilot-density Doppler review coverage in
  `plugins/scfdma/tests/pilot_density_review.rs`, validating dense-pilot
  (`SCFDMA52-64QAM-P4`) robustness against sparse-pilot
  (`SCFDMA52-64QAM`) under deterministic Watterson channels.
- On-air orchestration scripts now support `--help` and print usage text for
  unknown flags:
  `scripts/onair-preflight.sh`, `scripts/run-onair-tests.sh`,
  and `scripts/onair-bundle-evidence.sh`.
- Evidence bundles now include repository-state traceability:
  `metadata.json` carries `git_dirty`, and bundles include
  `git-status.short.txt`.

- `qpsk-plugin` demodulation now uses lower-overhead carrier/downmix loops
  (single-pass sin/cos evaluation and phase-step accumulation), reducing CPU
  cost in symbol extraction paths.
- `QPSK1000-HF` adaptive equalizer profile is now pinned to `mu=0.015` to match
  validated Watterson characterization and in-code documentation.
- Added `scripts/onair-preflight.sh` to validate on-air readiness locally
  (required tooling, callsign/config sanity, and expected release binaries).
- `scripts/run-onair-tests.sh` now executes local preflight by default,
  with `--no-preflight` available for explicitly pre-validated sessions.
- On-air report JSON now records preflight execution metadata
  (`preflight.ran` and `preflight.mode`) for compliance evidence trails.
- Added `scripts/onair-bundle-evidence.sh` to create structured evidence bundles
  for on-air validation runs (metadata + report + config snapshot + notes).
- Evidence bundles now support strict validation flags to require report, config,
  and preflight metadata for compliance capture.

- Adaptive-rate ACK-UP progression now skips unmapped reserved profile rungs,
  avoiding transitions into `None` mode mappings during active sessions.
- The SNR-gated admission path remains limited to HPX wideband-HD SL13 -> SL14;
  non-wideband profiles keep expected ACK-UP progression.

- Project docs are now organized under docs/ with a consistent format.
- Pull requests now run docs frontmatter validation checks.
- Docs touched in pull requests now receive automatic last_updated stamping.

## v0.1.0

- First public OpenPulseHF release.
- Introduced plugin-based modem architecture in a Cargo workspace.
- Included BPSK mode support and loopback-based testing path.
