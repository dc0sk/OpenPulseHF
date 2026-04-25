---
project: openpulsehf
doc: docs/backlog.md
status: living
last_updated: 2026-04-25
---

# Backlog

## Sprint: HPX hardening & observability

Items drawn from near-term and mid-term roadmap, sized for one sprint.

### HPX benchmark harness
- Define input signal corpus (SNR sweep, multipath profiles, silence gaps).
- Define reproducible run procedure and output metrics (frame error rate, throughput, latency).
- Implement reduced CI benchmark suite in `openpulse-modem` with regression gate tests.
- Add `openpulse benchmark run` CLI subcommand that drives the harness and emits JSON results.

### Signed transfer envelope
- Define the on-wire signed transfer envelope format (header, payload hash, signature block).
- Add `SignedEnvelope` type to `openpulse-core`.
- Implement envelope encode/decode in `openpulse-modem`.
- Add CLI `session transmit --signed` flag that wraps outbound frames in the envelope.
- Add verification step in `session receive` that checks the signature against PKI trust store.

### HPX session persistence
- Save active session state to `~/.config/openpulse/session-state.json` on session start.
- Restore session state on CLI restart so `session state` and `session log` survive process exit.
- Add `session resume` subcommand to re-attach a modem engine to a persisted session.
- Add `session list` subcommand to enumerate saved sessions with their final HPX state.

### Trust-store CLI commands
- `openpulse trust import <key-file>` — import a peer public key into the local trust store.
- `openpulse trust list` — enumerate trusted peers and their key fingerprints.
- `openpulse trust revoke <peer-id>` — mark a peer's key as locally revoked.
- Persist trust-store entries to `~/.config/openpulse/trust-store.json`.

### CI & cross-compile
- Add `aarch64-unknown-linux-gnu` cross-compile step to CI pipeline.
- Add Pi 5 smoke-test profile (loopback only, no audio device required) to CI matrix.
- Add benchmark regression gate that fails the CI run on >10 % FER increase vs baseline.

## Icebox

Items acknowledged but not yet sprint-scheduled.

- QPSK mode plugin and spectral efficiency benchmarks.
- Optional Reed-Solomon forward error correction.
- Bandwidth-adaptive rate control hooks.
- ARDOP-compatible mode plugin skeleton.
- GPU offload candidate kernel list and CPU/GPU equivalence test design.
- Peer cache schema and signed descriptor query protocol.
- Multi-hop relay path selection and trust-policy enforcement.
