---
project: openpulsehf
doc: docs/dev/steering/changelog.md
status: living
last_updated: 2026-06-29
---

# Changelog

> Phase/roadmap history lives in [roadmap.md](roadmap.md); this file tracks
> user-visible changes. "Unreleased" = merged to `main`, not yet in a tagged release.

## Unreleased

- **Security/Identity**: The daemon now performs the Ed25519 signed handshake over RF on connect — the initiator sends a signed `ConReq`, the responder verifies it and replies with a signed `ConAck`, and the initiator verifies that (both SAR-fragmented, since the frames exceed one modem frame). The verified peer callsign + Maidenhead grid are stored, a `PeerVerified` event is emitted, and the verified grid is written to the ADIF logbook (ahead of the `[logbook.peer_grids]` fallback). New `[station] identity_key_path`; 30 s handshake timeout (PR #584).
- **ARDOP TNC**: Opt-in adaptive ARQ session via `[ardop] enable_adaptive_arq` / `adaptive_profile`. With it on, the host `ARQBW` hint now caps the adaptive rate ladder by occupied bandwidth and `ARQTIMEOUT` drops an idle connection (both were accepted-and-echoed no-ops before). New rate-policy bandwidth-cap API (`set_arq_max_tx_level`), distinct from the OTA bounds (PR #585).
- **Radio/CAT**: The generic serial CAT backend is now selectable from the daemon for rigs Hamlib/rigctld doesn't support — `[radio] cat_backend = "generic"` with `serial_port` + `rig_file`, built with `--features generic-serial` (Unix). `RigctldController` gained its `CatController` impl (PR #586).
- **Operator Panel**: AGC on/off toggle in the controls column (receiver streaming AGC), completing panel control-surface parity (PR #583).
- **CLI**: Added `openpulse daemon set-tx-attenuation <db> [--band]`, closing the last daemon control-surface parity gap (PR #587).
- **Docs**: Sorted `docs/dev/` into topic subfolders (`design/`, `pki/`, `research/`, `steering/`) with all references updated (PR #582); user manual updated for the above config options and CLI command (PR #588).

- **Bandplan Guardrails**: Added missing occupied-bandwidth coverage for active `-RRC` waveform variants and `SCFDMA52-64QAM-P4`, preventing valid region-aware transmissions from being rejected as `UnknownOperatingMode`.
- **Bandplan Guardrails**: `BandplanPolicy::default()` now uses `HamIaruRegion1` instead of the deprecated `HamIaru` variant.
- **Bandplan Guardrails**: Region 3 validation now exposes an explicit warning when Region 1 allocations are being used as a conservative proxy.
- **Regulatory Logging**: `TxSessionLog::log_frame` now rejects cross-station metadata instead of silently mixing frames from different callsigns into the same compliance log.
- **Session Metrics**: Session-metrics JSON now labels throughput as an upper-bound proxy and emits a dedicated `throughput_bps_note` field so downstream consumers do not misread the value as exact payload throughput.

- **Waveform Validation**: Added BL-TP-7 pilot-density Doppler review test (`plugins/scfdma/tests/pilot_density_review.rs`) comparing sparse (`SCFDMA52-64QAM`) and dense (`SCFDMA52-64QAM-P4`) pilot profiles under deterministic Watterson low/high Doppler conditions.
- **CLI/UX**: Added `--help` support to on-air orchestration scripts (`onair-preflight.sh`, `run-onair-tests.sh`, and `onair-bundle-evidence.sh`) plus usage output on unknown flags.
- **Operational Tooling**: Evidence bundles now capture repository state context (`git_dirty` in `metadata.json` and `git-status.short.txt` snapshot) for stronger compliance traceability.
- **Testing**: Extended benchmark integration tests with cached-corpus stability assertions (`standard_corpus` static-slice identity).

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
