---
project: openpulsehf
doc: docs/roadmap.md
status: living
last_updated: 2026-04-25
---

# Roadmap

## Completed (shipped in PR #49)

- ✅ HPX benchmark harness inputs, metrics, and reproducible run procedure.
- ✅ Signed transfer envelope format (header, payload_hash, signature_block).
- ✅ Reduced CI benchmark suite with regression gates (100% pass rate, mean_transitions ≤ 20.0).
- ✅ HPX session persistence to `~/.config/openpulse/session-state.json`.
- ✅ Trust-store CLI commands: import, list, revoke.
- ✅ ARM64 cross-compile validation (aarch64-unknown-linux-gnu).
- ✅ Pi 5 smoke-test profile (loopback + benchmark).
- ✅ CI auto-trigger on push and pull requests.

## Completed (shipped in PR #50)

- ✅ Hardened BPSK TX/RX behavior under loopback with expanded scenario tests.
- ✅ Improved modem diagnostics output with structured session diagnostics.
- ✅ Added `session state --diagnostics` for detailed JSON metrics.

## Near term

- Expand integration tests around frame boundaries and timing assumptions.
- Complete real-device validation path for BPSK hardening.
- Define GPU offload candidate kernels and CPU/GPU equivalence test strategy.
- Define peer cache schema and query protocol envelope.
- Define relay route scoring and maximum-hop policy defaults.
- Add streaming/follow mode for `session log`.

## In progress (current branch)

- Add multithreaded modem pipeline boundaries and scheduling policy.
- Add scheduler queue metrics to session diagnostics.
- Add `session resume` and `session list` persistence commands.

## Mid term

- Add QPSK modes with higher spectral efficiency.
- Introduce optional forward error correction (for example Reed-Solomon and convolutional coding).
- Add bandwidth-adaptive rate control hooks in the modem engine.
- Add ARDOP-compatible mode plugin support.
- Implement HPX500 and HPX2300 adaptive profiles as plugin modes.
- Implement signed handshake and signed transfer manifest verification for HPX sessions.
- Implement full benchmark suite execution and artifact publishing for release readiness.
- Implement multithreaded HPX pipeline execution in production mode.
- Implement optional GPU acceleration path using open frameworks with CPU fallback.
- Implement peer cache and query subsystem with signed descriptor handling.
- Implement multi-hop relay path selection and forwarding controls.
- Implement relay trust-policy enforcement and route observability events.

## Long term

- Add optional TUI and GUI frontends on top of stable core APIs.
- Add stronger observability for live link quality and tuning.
- Support richer automation flows for repeated test and deployment scenarios.
- Publish periodic HPX performance reports against maintained benchmark profiles.

