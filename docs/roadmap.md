---
project: openpulsehf
doc: docs/roadmap.md
status: living
last_updated: 2026-05-04
---

# Roadmap

## Scope policy

- Primary goal: build an independent and competing OpenPulse protocol from scratch.
- External/proprietary compatibility modes (for example VARA and PACTOR-4) are secondary and must not proceed without legal review and explicit approval.
- Regulatory compliance (FCC Part 97, CEPT/EU, UK Ofcom) is a hard requirement before any on-air transmission use. See docs/regulatory.md.

---

## Phase 0 — Foundation (Completed)

All Phase 0 work has shipped and merged.

### Shipped in PR #49
- ✅ HPX benchmark harness: inputs, scenarios, reproducible run procedure, JSON result schema.
- ✅ Signed transfer envelope format (header, payload_hash, signature_block).
- ✅ CI benchmark regression gates (100% pass rate, mean_transitions ≤ 20.0).
- ✅ HPX session persistence to `~/.config/openpulse/session-state.json`.
- ✅ Trust-store CLI commands: import, list, revoke.
- ✅ ARM64 cross-compile validation (aarch64-unknown-linux-gnu).
- ✅ Raspberry Pi 5 smoke-test profile (loopback + benchmark).
- ✅ CI auto-trigger on push and pull requests.

### Shipped in PR #50
- ✅ Hardened BPSK TX/RX under loopback with 56-scenario fixture matrix.
- ✅ Structured session diagnostics with transition event capture.
- ✅ `session state --diagnostics` JSON output.

### Shipped (multithreaded pipeline + session management)
- ✅ Explicit pipeline stage boundaries and bounded-channel scheduler.
- ✅ Per-stage scheduler metrics in diagnostics.
- ✅ `session list` and `session resume` commands.
- ✅ `session log --follow` streaming mode.

### Shipped (FEC phase 1)
- ✅ `FecCodec` in `openpulse-core` with GF(2^8) RS codec (ECC_LEN=32, corrects up to 16 byte errors/block).
- ✅ `ModemEngine::transmit_with_fec` and `receive_with_fec`.
- ✅ BER-injection correctness tests and 20-scenario loopback fixture matrix.

### Shipped (QPSK phase 1)
- ✅ `qpsk-plugin` with Gray-mapped QPSK modulate/demodulate (PR #56).
- ✅ CLI plugin registration for QPSK125, QPSK250, QPSK500.
- ✅ Loopback fixture matrix and spectral efficiency benchmarks.

---

## Phase 1 — Protocol Foundation (Completed)

All Phase 1 items shipped. See CLAUDE.md for per-PR completion records.

### 1.1 — CI and codebase health ✅ Done (PR #67, #68)
### 1.2 — Segmentation and reassembly (SAR) ✅ Done
### 1.3 — Interleaver integration ✅ Done (PR #70)
### 1.4 — Channel model implementation ✅ Done (PR #71)
### 1.5 — Radio interface layer ✅ Done (PTT, AFC, CLI wiring)
### 1.6 — Real-device validation *(deferred — replaced by Phase 3.5-substitute)*

---

## Phase 2 — HPX Protocol Completion (Completed)

All Phase 2 items shipped.

### 2.1 — ACK frame taxonomy and rate adaptation ✅ Done
- `AckType` (8 variants), `AckFrame` (5-byte codec, CRC-8/SMBUS, FNV-1a session hash).
- `SpeedLevel` (SL1–SL11), `RateAdapter` state machine; ACK-DOWN floors at SL2; ChirpFallback via 3 consecutive NACKs.
- `Fsk4Plugin` for FSK4-ACK: 4 tones, 100 baud, Hann-windowed, Goertzel demodulator.
- 11 rate adaptation tests; 3 FSK4 loopback tests.

### 2.2 — HPX500 and HPX2300 adaptive profiles ✅ Done
- `QPSK1000` mode added; `Psk8Plugin` for `8PSK500` and `8PSK1000` (Gray-coded, single-carrier).
- `SessionProfile::hpx500()` (SL2=BPSK31 → SL6=QPSK500) and `SessionProfile::hpx2300()` (SL8=QPSK500 → SL11=8PSK1000).
- HPX2300 waveform decision: single-carrier over OFDM (lower PAPR, simpler AFC — see docs/architecture.md).
- 4 session profile tests; 8 adaptive profile integration tests.

### 2.3 — Signed handshake and manifest verification ✅ Done
- `ConReq`/`ConAck` wire frames with Ed25519 sign/verify; trust evaluation via `evaluate_handshake()`.
- `TransferManifest` with SHA-256 payload hash, sender ID, Ed25519 signature; `verify_manifest()`.
- 12 handshake integration tests; 6 manifest integration tests.

### 2.4 — Channel access and shared-channel operation ✅ Done
- `DcdState`: RMS energy threshold, 100 ms hold window, `update()`/`is_busy()`/`energy()`.
- 0.3-persistence CSMA in `stage_emit_output()`; `ModemError::ChannelBusy` on blocked TX.
- 4 CSMA loopback integration tests.

### 2.5 — Peer cache and query subsystem ✅ Done
- `PeerDescriptor`: self-authenticating signed identity; `peer_id` IS the Ed25519 verifying-key bytes.
- `PeerCache::query()` with `TrustFilter` and capability mask; `PeerQueryRequest`/`PeerQueryResponse` wire envelopes.
- 9 peer descriptor tests; 9 wire query tests.

### 2.6 — Multi-hop relay ✅ Done
- `RelayForwarder`: hop-limit enforcement, duplicate suppression, trust-policy check.
- Trust-weighted path scoring; `score_route`/`select_best_scored_route`.
- `RelayDataChunk` (msg_type 0x05) and `RelayHopAck` (msg_type 0x06) wire codecs.
- 12 relay integration tests (path scoring, 3-hop chain, event drain, wire round-trips).

### 2.7 — Compression (session layer) ✅ Done
- `lz4_flex 0.11`; `compress_if_smaller()` — compress-then-compare, returns original if LZ4 not smaller.
- `ConReq`/`ConAck` carry `supported_compression`/`selected_compression`; covered by Ed25519 signature.
- 9 compression integration tests.

---

## Phase 3 — Advanced Signal Processing and Compliance (Partial)

Most Phase 3 items shipped. Remaining: 3.2 (Turbo FEC evaluation, deferred) and 3.5 on-air validation.

### 3.1 — Post-quantum in-band handshake ✅ Done
- `ml-dsa 0.1.0-rc.9` (ML-DSA-44) and `ml-kem 0.3` (ML-KEM-768) added to `openpulse-core`.
- `PqConReq`/`PqConAck` wire frames with classical + PQ pubkeys, KEM pubkey/ciphertext, dual signatures.
- Hybrid mode: Ed25519 + ML-DSA-44 dual signature; X25519 + ML-KEM-768 dual KEM.
- SAR encode→fragment→reassemble→decode round-trip validated for full PQ payload sizes.
- 12 PQ handshake integration tests.

### 3.2 — Turbo code FEC evaluation *(deferred)*
- Benchmark Turbo code decoder on Raspberry Pi 4 against equivalent RS+interleaver at matched code rate.
- If Turbo codes deliver ≥ 2 dB gain at acceptable CPU cost, add as an optional FEC codec for HPX high-rate profiles.
- Document decision in docs/vara-research.md FEC comparison section.

### 3.3 — GPU acceleration ✅ Done (PR #90)
- `crates/openpulse-gpu`: `GpuContext` (wgpu device + pre-compiled pipelines), WGSL kernels for BPSK modulation, IQ demodulation, and timing offset search.
- `bpsk-plugin` gains optional `gpu` feature; CPU fallback when GPU returns `None`.
- All GPU functions return `Option<T>` to surface failures rather than silently returning empty/zero data.
- GPU-vs-CPU equivalence tests under `#[cfg(feature = "gpu")]`.

### 3.4 — ARDOP-compatible TCP interface ✅ Done (PR #91)
- ARDOP-compatible TCP command/data port interface (`crates/openpulse-ardop`).
- Command port (default 8515): ASCII line protocol — VERSION, MYID, LISTEN, CONNECT, DISCONNECT, ABORT, STATE, BUFFER, PTT, CLOSE.
- Data port (default 8516): `u16 BE` length-prefixed binary framing.
- `openpulse-tnc` binary; loopback mode for protocol-level integration tests.
- 8 integration tests covering all major ARDOP commands and data port framing.
- Legal review still required before labelling as "ARDOP-compatible" in public documentation.

### 3.5 — Regulatory on-air validation *(Phase 3.5-substitute shipped)*
Phase 3.5-substitute (sound-loopback channel simulation) is done (PR #89):
- `ChannelSimHarness` wires two `ModemEngine` instances through `openpulse-channel` models.
- 6 channel-loopback integration tests (clean, AWGN 20 dB, Watterson F1/F2, Gilbert-Elliott with/without FEC).
- These replace on-air validation as the CI gate for loopback correctness.

Remaining on-air items:
- Conduct on-air tests on IARU-aligned frequencies for each supported bandwidth class.
- Verify station identification behaviour at 10-minute intervals under long sessions.
- Test relay node automatic control point interface.
- Publish compliance test report as a release artefact.

---

## Phase 4 — Ecosystem and Long-term (Active)

**Active phase.** Phase 4.2, 4.1, and 4.3 shipped. Phase 4.4 is next.

### 4.2 — Observability and automation ✅ Done (PR #92)
- `EngineEvent` broadcast channel in `ModemEngine` — 8 event variants (AFC update, rate change, DCD flip, HPX transition, frame TX/RX, session start/end).
- `engine.subscribe()` returns a `broadcast::Receiver<EngineEvent>` for real-time event consumption.
- `HpxState`, `HpxEvent`, `RateEvent` all derive `Serialize/Deserialize`.
- `openpulse monitor --mode <MODE>` CLI subcommand streams all engine events as NDJSON to stdout; pipeable to `jq` or scripts.
- 7 integration tests in `engine_events.rs` (including DcdChange and AfcUpdate coverage).

### 4.1 — TUI frontend ✅ Done
- New `crates/openpulse-tui` binary crate using ratatui 0.27 + crossterm.
- Background worker thread drives `ModemEngine::receive()` and forwards events to TUI via `std::sync::mpsc`.
- Live panels: HPX state (colour-coded), AFC offset + rate/mode, DCD energy bar (Gauge), scrollable transitions log (last 50).
- Keyboard: `q`/Ctrl+C to quit, `p` to pause updates, `↑↓` to scroll transition log.

### 4.3 — KISS and AX.25 interface ✅ Done
- New `crates/openpulse-kiss` crate with `openpulse-kisstnc` binary.
- KISS frame encode/decode with full byte stuffing (FEND/FESC).
- AX.25 UI frame encode/decode (callsign, SSID, Control=0x03, PID=0xF0).
- TCP listener (default port 8100) using the same `broadcast` + `std::sync::mpsc` bridge pattern as `openpulse-ardop`.
- 0.3-persistence CSMA (Phase 2.4) already in `ModemEngine`; honoured on TX path.
- Target: APRS clients (APRSdroid, PinPoint, Xastir) via KISS over TCP.
- 8 integration tests (KISS codec round-trips, byte stuffing, AX.25 frame round-trip, TCP loopback with multi-frame and special bytes).

### 4.4 — B2F protocol and Winlink gateway integration
- Study B2F (Binary over HTTP) protocol as used by Winlink over PACTOR/VARA/ARDOP connections.
- B2F is the application-layer protocol for Winlink email forwarding; it runs on top of the TNC data connection.
- Assess feasibility of B2F integration for full Winlink gateway compatibility.
- This requires legal review of Winlink protocol documentation before implementation begins.

---

## Dependency ordering summary

The following dependencies constrain the execution sequence:

```
Phase 1.2 (SAR)
    └─> Phase 3.1 (PQ in-band handshake)

Phase 1.3 (Interleaver)
    └─> Phase 2 (HPX profiles can claim burst-error resilience)

Phase 1.5 (Radio interface / PTT)
    └─> Phase 1.6 (Real-device validation)
    └─> Phase 3.5 (Regulatory on-air validation)

Phase 2.1 (ACK taxonomy)
    └─> Phase 2.2 (HPX500/HPX2300 adaptive profiles)

Phase 2.5 (Peer cache)
    └─> Phase 2.6 (Multi-hop relay)

Phase 2.4 (CSMA / DCD)
    └─> Phase 4.3 (KISS broadcast mode)
```

Items within the same phase may proceed in parallel unless a dependency within the phase is listed above.
