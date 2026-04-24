---
project: openpulsehf
doc: docs/backlog.md
status: living
last_updated: 2026-04-24
---

# Backlog

- Design secure bandwidth optimization for signature sharing: agreed direction is out-of-band certificate distribution (internet DB / local cache, never over-air unless explicitly requested by peer) combined with per-session HMAC after an initial asymmetric key-exchange, reducing steady-state per-packet crypto overhead by ~50–75%. Must be refined alongside connection trust-level model and paranoid-mode spec (see pki-tooling-trust-policy.md § Connection trust levels and signing modes). Further discussion required before implementation.
