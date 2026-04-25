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

## Near term

- Harden BPSK TX/RX behavior under loopback and real-device paths.
- Improve modem diagnostics and error clarity in CLI output.
- Expand integration tests around frame boundaries and timing assumptions.
- Define multithreaded modem pipeline boundaries and scheduling policy.
- Define GPU offload candidate kernels and CPU/GPU equivalence test strategy.
- Define peer cache schema and query protocol envelope.
- Define relay route scoring and maximum-hop policy defaults.
- Add `session resume` subcommand to re-attach to persisted sessions.
- Add `session list` subcommand to enumerate saved sessions with HPX state.

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

