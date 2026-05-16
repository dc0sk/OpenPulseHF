---
project: openpulsehf
doc: docs/changelog.md
status: living
last_updated: 2026-05-17

# Changelog

## Unreleased

- **Performance**: Cached standard benchmark corpus using `LazyLock` to eliminate repeated Vec allocation on every benchmark run. Result: cleaner command invocations without changing regression criteria (PR #275).
- **Quality**: Resolved Clippy `needless_borrow` warning in benchmark CLI command and replaced deprecated `BandplanMode::HamIaru` with region-specific `HamIaruRegion1` in tests (PR #276).

- Optimized `qpsk-plugin` demodulation hot paths by reducing duplicate trigonometric
  work in passband/RRC downmix loops and using phase-step accumulation in symbol
  integration.
- Aligned the `QPSK1000-HF` LMS profile to the validated characterization value
  `mu=0.015` (from `0.012`) and kept deterministic Watterson guard tests green.
- Added `scripts/onair-preflight.sh` for repeatable station-readiness checks
  before live RF sessions (`--strict` mode enforces built binaries and config).
- `scripts/run-onair-tests.sh` now runs local preflight by default
  (override with `--no-preflight` when already validated).
- On-air JSON reports now include preflight execution metadata
  (`preflight.ran`, `preflight.mode`) for audit traceability.
- Added `scripts/onair-bundle-evidence.sh` to package on-air run artifacts
  (report JSON, metadata, config snapshot, notes) for Phase 5.5-reg evidence capture.
- Added strict validation flags to the evidence-bundle tool so compliance runs
  can fail fast when report, config, or preflight metadata are missing.

- Updated adaptive-rate ACK-UP behavior in `openpulse-modem` so active session
  profiles skip unmapped reserved speed levels instead of landing on
  `None` mode mappings (for example, HPX wideband now advances SL9 -> SL11).
- Clarified and regression-tested that SNR-gated admission remains specific to
  HPX wideband-HD SL13 -> SL14 and does not affect non-wideband-HD profiles.

- Added `FecCodec` to `openpulse-core`: Reed-Solomon GF(2^8) codec (ECC_LEN=32, corrects up to 16 byte errors per 255-byte block).
- Added `ModemError::Fec` variant for FEC-specific error propagation.
- Added `ModemEngine::transmit_with_fec` and `receive_with_fec` for transparent FEC-protected transmission.
- Added FEC loopback hardening tests: 20-scenario fixture matrix (2 modes × 10 payloads) plus BER-injection correctness and capacity-exceeded failure tests.

- Added `qpsk-plugin` crate with Gray-mapped QPSK modulation and demodulation.
- Registered QPSK plugin in CLI engine, exposing modes `QPSK125`, `QPSK250`, and `QPSK500` via `openpulse modes`.
- Added QPSK loopback fixture matrix (3 modes × 14 payload profiles = 42 scenarios).
- Added spectral efficiency benchmarks confirming QPSK250 carries more bits per sample than BPSK250 at equal baud rate.

- Added documentation framework with standardized frontmatter.
- Added docs CI checks and automated last_updated stamping for pull requests.
- Expanded `openpulse-modem` BPSK hardening coverage with a deterministic
  loopback fixture matrix executing 56 scenarios across supported modes and
  payload profiles.
- Strengthened `openpulse-modem` structured HPX event logging so diagnostic
  entries preserve `event_source`, `session_id`, and `reason_string`, and
  transition events are counted consistently in session diagnostics.
- Improved `openpulse session state --diagnostics` output so text mode renders
  a readable summary plus event lines while JSON mode keeps the raw structured
  diagnostics payload and uses persisted peer context when available.

### HPX conformance & session audit (2026-04-25)

- Added 10 HPX spec conformance integration tests in `openpulse-modem` covering
  all major state-machine paths (happy path, timeouts, signature rejection,
  quality recovery, ARQ exhaustion, local/remote teardown, relay activation).
- Fixed missing `RelayActive + TrainingOk → ActiveTransfer` state-machine transition
  in `openpulse-core::hpx` required by the relay conformance scenario.
- Added `hpx_session_id()` and `hpx_transitions()` public accessors to `ModemEngine`.
- Added `POST /api/v1/session-audit-events` endpoint to `pki-tooling` that validates
  and persists HPX transition logs to the `audit_events` table.
- Added `PkiClient::create_session_audit_event` and `record_handshake_session_audit`
  to the CLI, wiring `diagnose handshake` to post audit events on every execution.
- Added `openpulse session` CLI subcommand group with four commands:
  `start`, `state`, `end`, and `log`, exposing the full HPX lifecycle through the CLI.
- Added 5 integration tests for the `session` command group using mockito.
- Added `live_pki_integration.rs` test suite that spins up the real `pki-tooling`
  axum router on a random TCP port and validates CLI commands end-to-end against
  a live Postgres database (skips gracefully when `PKI_TEST_DATABASE_URL` is unset).

## 0.1.0

- Initial OpenPulseHF workspace with core modem architecture.
- Added BPSK plugin and CLI-based transmit/receive operations.
- Added audio backend support for loopback and CPAL-based systems.
