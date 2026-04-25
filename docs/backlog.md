---
project: openpulsehf
doc: docs/backlog.md
status: living
last_updated: 2026-04-25
---

# Backlog

<<<<<<< HEAD
## Completed: HPX hardening & observability sprint

All sprint items (1-5) shipped in PR #49. Full test coverage (90+ tests) and CI integration verified on main.
=======
## Sprint: BPSK Hardening & Diagnostics & Observability

Sized for one sprint, focusing on modem robustness and CLI debugging experience.

### BPSK Hardening
- Add loopback fixture tests with SNR sweep (6dB, 9dB, 12dB, 15dB).
- Add multipath profile tests (fading, frequency offset, timing error).
- Implement error recovery scenarios (frame loss, timeout, retransmit).
- Harden real-device path (hardware detection, fallback, graceful degradation).
- Document BPSK behavior matrix (success/failure conditions).

### Diagnostics & Observability
- Add structured HPX event logging (JSON format with timestamps, state deltas).
- Improve CLI error messages (actionable recommendations, error codes).
- Add `--verbose` flag to CLI commands (emit debug logs to stderr).
- Add `--diagnostics` JSON output to `session state` (detailed HPX metrics).
- Add HPX event trace to benchmark report (transition sequence, timing).

## Upcoming Sprint: Session Management

Items for the next sprint after BPSK/Diagnostics.

### Session Management Extensions
- Add `session resume <session-id>` — re-attach to persisted session.
- Add `session list` — enumerate all saved sessions with HPX state.
- Add `session log` streaming (follow new events in real-time).

## Previous Sprint: HPX hardening & observability (PR #49 pending)
>>>>>>> 79318a4 (Foundation: Add diagnostics module and BPSK hardening test suite)

### HPX benchmark harness ✅
- Input signal corpus (SNR sweep, multipath profiles, silence gaps).
- Reproducible run procedure and output metrics (frame error rate, throughput, latency).
- Reduced CI benchmark suite in `openpulse-modem` with regression gate tests.
- `openpulse benchmark run` CLI subcommand with JSON results.
- **Status**: 10-scenario corpus implemented, regression gate validates 100% pass rate and mean_transitions ≤ 20.0, CI gate active.

### Signed transfer envelope ✅
- On-wire signed transfer envelope format (header, payload hash, signature block).
- `SignedEnvelope` type in `openpulse-core`.
- Envelope encode/decode in `openpulse-modem`.
- **Status**: Full round-trip codec with tamper detection and signature verification implemented and tested.

### HPX session persistence ✅
- Active session state saved to `~/.config/openpulse/session-state.json` on session start.
- Session state restored on CLI restart for `session state` and `session log`.
- **Status**: Snapshot-based persistence with safe metadata storage implemented. Integration tests passing.

### Trust-store CLI commands ✅
- `openpulse trust import <key-file>` — import peer public key.
- `openpulse trust list` — enumerate trusted peers.
- `openpulse trust revoke <peer-id>` — revoke peer key.
- **Status**: Local JSON storage with upsert/revoke semantics implemented. Full CLI integration tested.

### CI & cross-compile ✅
- `aarch64-unknown-linux-gnu` cross-compile step in CI.
- Pi 5 smoke-test profile (loopback only).
- Benchmark regression gate (fail on any failed scenario or mean_transitions > 20.0).
- CI auto-trigger on push to main/develop/feat/*, pull requests to main/develop.
- **Status**: All CI jobs active with automatic triggers enabled. Locally validated on ubuntu-latest.

## Icebox

Items acknowledged but not yet sprint-scheduled.

- QPSK mode plugin and spectral efficiency benchmarks.
- Optional Reed-Solomon forward error correction.
- Bandwidth-adaptive rate control hooks.
- ARDOP-compatible mode plugin skeleton.
- GPU offload candidate kernel list and CPU/GPU equivalence test design.
- Peer cache schema and signed descriptor query protocol.
- Multi-hop relay path selection and trust-policy enforcement.
