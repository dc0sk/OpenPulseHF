---
project: openpulsehf
doc: docs/changelog.md
status: living
last_updated: 2026-04-25
---

# Changelog

## Unreleased

- Added documentation framework with standardized frontmatter.
- Added docs CI checks and automated last_updated stamping for pull requests.
- Expanded `openpulse-modem` BPSK hardening coverage with a deterministic
  loopback fixture matrix executing 56 scenarios across supported modes and
  payload profiles.
- Strengthened `openpulse-modem` structured HPX event logging so diagnostic
  entries preserve `event_source`, `session_id`, and `reason_string`, and
  transition events are counted consistently in session diagnostics.

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
