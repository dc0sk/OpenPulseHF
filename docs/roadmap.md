---
project: openpulse
doc: docs/roadmap.md
status: living
last_updated: 2026-04-23
---

# Roadmap

## Near term

- Harden BPSK TX/RX behavior under loopback and real-device paths.
- Improve modem diagnostics and error clarity in CLI output.
- Expand integration tests around frame boundaries and timing assumptions.
- Define HPX benchmark harness inputs, metrics, and reproducible run procedure.
- Define signed transfer envelope format and trust-store schema.
- Implement reduced CI benchmark suite with regression gates for HPX.
- Define multithreaded modem pipeline boundaries and scheduling policy.
- Define GPU offload candidate kernels and CPU/GPU equivalence test strategy.
- Add Raspberry Pi 4/5 benchmark profile definitions.

## Mid term

- Add QPSK modes with higher spectral efficiency.
- Introduce optional forward error correction (for example Reed-Solomon and convolutional coding).
- Add bandwidth-adaptive rate control hooks in the modem engine.
- Add ARDOP-compatible mode plugin support.
- Implement HPX500 and HPX2300 adaptive profiles as plugin modes.
- Implement signed handshake and signed transfer manifest verification for HPX sessions.
- Ship CLI trust-store management commands for key import, list, and revoke markers.
- Implement full benchmark suite execution and artifact publishing for release readiness.
- Implement multithreaded HPX pipeline execution in production mode.
- Implement optional GPU acceleration path using open frameworks with CPU fallback.
- Add ARM64 CI or scheduled validation runs for Raspberry Pi 4/5 compatibility.

## Long term

- Add optional TUI and GUI frontends on top of stable core APIs.
- Add stronger observability for live link quality and tuning.
- Support richer automation flows for repeated test and deployment scenarios.
- Publish periodic HPX performance reports against maintained benchmark profiles.

