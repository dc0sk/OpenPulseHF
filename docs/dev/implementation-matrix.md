---
project: openpulsehf
doc: docs/dev/implementation-matrix.md
status: living
last_updated: 2026-06-29
---

# Implementation Matrix

This matrix maps major user-facing and protocol features to the primary implementation
surface and the strongest integration-test evidence currently in-tree.

> For the full numbered, cross-linked traceability (REQ-IDs ↔ CAP-IDs, with design decisions,
> tests, results, supporting assets, and PRs), see
> [traceability-matrix.md](steering/traceability-matrix.md). This page is the quick
> feature → implementation → test summary.

## Core protocol and security

| Feature | Primary implementation | Validation evidence | Status |
|---|---|---|---|
| Signed classical handshake (Ed25519) | `crates/openpulse-core/src/handshake.rs` | `crates/openpulse-core/tests/handshake_integration.rs` | Implemented |
| Post-quantum handshake (ML-DSA-44 / ML-KEM-768) | `crates/openpulse-core/src/pq_handshake.rs` | `crates/openpulse-core/tests/pq_handshake_integration.rs` | Implemented |
| Transfer manifest signing/verification | `crates/openpulse-core/src/manifest.rs` | `crates/openpulse-core/tests/manifest_integration.rs` | Implemented |
| Segmentation and reassembly (SAR) | `crates/openpulse-core/src/sar.rs` | `crates/openpulse-core/tests/sar_roundtrip.rs` | Implemented |
| Adaptive rate control and ACK taxonomy | `crates/openpulse-core/src/rate.rs`, `crates/openpulse-core/src/ack.rs` | `crates/openpulse-core/tests/rate_adaptation.rs`, `crates/openpulse-modem/tests/bidir_rate_adaptation.rs` | Implemented |

## Waveforms, DSP, and FEC

| Feature | Primary implementation | Validation evidence | Status |
|---|---|---|---|
| BPSK/QPSK/8PSK/64QAM plugin families | `plugins/bpsk`, `plugins/qpsk`, `plugins/psk8`, `plugins/64qam` | `crates/openpulse-modem/tests/bpsk_hardening.rs`, `crates/openpulse-modem/tests/qpsk_hardening.rs`, plugin unit tests | Implemented |
| OFDM and SC-FDMA plugins | `plugins/ofdm`, `plugins/scfdma` | `crates/openpulse-modem/tests/ofdm_simulation.rs`, `plugins/scfdma/tests/loopback.rs` | Implemented |
| AFC loop and correction | `plugins/bpsk/src/demodulate.rs`, `crates/openpulse-modem/src/engine.rs` | `crates/openpulse-modem/tests/afc_correction.rs` | Implemented |
| DCD + CSMA channel access | `crates/openpulse-core/src/dcd.rs`, `crates/openpulse-modem/src/engine.rs` | `crates/openpulse-modem/tests/csma_loopback.rs` | Implemented |
| RS/Interleaver/StrongRS/ShortRS/Concat/SoftConcat | `crates/openpulse-core/src/fec.rs`, `crates/openpulse-core/src/soft_viterbi.rs` | `crates/openpulse-modem/tests/fec_loopback.rs` | Implemented |
| Window-ARQ selective retry + range-limited combine | `crates/openpulse-core/src/fec.rs`, `crates/openpulse-modem/src/engine.rs` | `crates/openpulse-modem/tests/window_arq_watterson.rs`, `crates/openpulse-modem/tests/window_arq_multimode.rs`, `crates/openpulse-modem/tests/window_arq_selective_engine.rs` | Implemented |
| LDPC iterative decoder path | `crates/openpulse-core/src/ldpc.rs`, `crates/openpulse-modem/src/engine.rs` | `crates/openpulse-modem/tests/ldpc_engine_loopback.rs` | Implemented |

## Services, interop, and tooling

| Feature | Primary implementation | Validation evidence | Status |
|---|---|---|---|
| ARDOP TCP TNC bridge | `crates/openpulse-ardop` | `crates/openpulse-ardop/tests/ardop_integration.rs` | Implemented |
| KISS/AX.25 TCP TNC bridge | `crates/openpulse-kiss` | `crates/openpulse-kiss/tests/kiss_integration.rs` | Implemented |
| B2F protocol + Winlink gateway | `crates/openpulse-b2f`, `crates/openpulse-gateway` | `crates/openpulse-b2f/tests/b2f_integration.rs`, `crates/openpulse-gateway` unit tests | Implemented |
| B2F driver end-to-end loopback | `crates/openpulse-b2f-driver` | `crates/openpulse-b2f-driver/tests/e2e_loopback.rs` | Implemented |
| QSY frequency agility | `crates/openpulse-qsy` | `crates/openpulse-qsy/tests/qsy_session.rs` | Implemented |
| Mesh relay and query propagation | `crates/openpulse-mesh`, `crates/openpulse-core/src/relay.rs`, `crates/openpulse-core/src/query_propagation.rs` | relay/query integration tests in `crates/openpulse-core/tests/` and mesh integration tests | Implemented |
| Daemon control server + panel/TUI control surfaces | `crates/openpulse-daemon`, `apps/openpulse-panel`, `crates/openpulse-tui` | `crates/openpulse-daemon/tests/control_port.rs`, command-apply unit tests in `crates/openpulse-daemon/src/lib.rs`, TUI/panel unit coverage | Implemented |
| FreeDV authenticated voice shim | `crates/openpulse-freedv-auth` | `crates/openpulse-freedv-auth/tests/freedv_auth_integration.rs` | Implemented |

## Non-code release/compliance gates

| Feature | Primary implementation | Validation evidence | Status |
|---|---|---|---|
| On-air regulatory validation report (Phase 5.5-reg) | Process/documentation workflow | `docs/on-air_testplan.md` defines procedure; `scripts/onair-generate-report.sh` and `scripts/onair-bundle-evidence.sh` provide the report scaffold | In progress (scaffold shipped; on-air field validation in active execution as of 2026-06 — see `docs/dev/onair-status.md`) |
