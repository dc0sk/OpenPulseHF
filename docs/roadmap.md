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

## Mid term

- Add QPSK modes with higher spectral efficiency.
- Introduce optional forward error correction (for example Reed-Solomon and convolutional coding).
- Add bandwidth-adaptive rate control hooks in the modem engine.
- Add ARDOP-compatible mode plugin support.

## Long term

- Add optional TUI and GUI frontends on top of stable core APIs.
- Add stronger observability for live link quality and tuning.
- Support richer automation flows for repeated test and deployment scenarios.

