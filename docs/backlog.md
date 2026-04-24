---
project: openpulsehf
doc: docs/backlog.md
status: living
last_updated: 2026-04-24
---

# Backlog

- Add CI baseline-comparison tool for HPX performance regression detection.
- Add trust-store file format and migration policy.
- Add conformance tests for signed handshake and manifest verification.
- Add CLI UX spec for identity and trust diagnostics.
- Add multithreaded pipeline test harness with deterministic timing assertions.
- Add CPU vs GPU equivalence tests for selected DSP kernels.
- Add Raspberry Pi 4/5 tuning guide and benchmark result appendix.
- Add peer cache eviction and conflict-resolution tests.
- Add query propagation duplicate-suppression tests.
- Add relay loop-prevention and multi-hop conformance tests.
- Add relay trust-policy failure-path integration tests.
- Design secure bandwidth optimization for signature sharing: agreed direction is out-of-band certificate distribution (internet DB / local cache, never over-air unless explicitly requested by peer) combined with per-session HMAC after an initial asymmetric key-exchange, reducing steady-state per-packet crypto overhead by ~50–75%. Must be refined alongside connection trust-level model and paranoid-mode spec (see pki-tooling-trust-policy.md § Connection trust levels and signing modes). Further discussion required before implementation.
