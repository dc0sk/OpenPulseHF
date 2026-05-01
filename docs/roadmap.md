---
project: openpulsehf
doc: docs/roadmap.md
status: living
last_updated: 2026-05-01
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

## Phase 1 — Protocol Foundation (Near term)

**Gate to exit Phase 1:** all items complete, multi-platform CI green, real-device validation done, SAR implemented, interleaver integrated with FEC, regulatory checklist signed off.

Phase 1 addresses structural gaps that block everything downstream: multi-platform CI confidence, the frame size constraint that blocks PQ crypto, FEC correctness on burst channels, and the radio interface layer.

### 1.1 — CI and codebase health
- Re-enable the multi-platform Ubuntu/macOS test matrix (currently `if: false` in CI). Failing tests are an advisory failure until fixed; blocking failure once stable.
- Split `openpulse-cli/src/main.rs` into per-subcommand modules. Current monolith is ~2500 lines and is not unit-testable.
- Add acceptance criteria to each functional requirement in docs/requirements.md, linking each requirement to a specific test or benchmark scenario.
- Publish a regulatory compliance sign-off checklist (docs/regulatory.md) as a required gate for any release enabling on-air transmission.

### 1.2 — Segmentation and reassembly (SAR)
SAR is the prerequisite for in-band PQ handshakes, large object transfer, and clean multi-frame session flows.
- Define SAR wire format: segment_id (u16, session-scoped), fragment_index (u8), fragment_total (u8), payload slice.
- Implement SAR encoder and decoder in `openpulse-core`.
- Integration tests: round-trip of objects from 256 bytes to 64 KB; error injection for missing fragments; reassembly timeout handling.
- Update frame format documentation in docs/architecture.md with the SAR sub-layer definition.
- Decision point: evaluate extending the `length` field from `u8` to `u16` as an alternative to SAR for objects up to 65535 bytes; document the decision and rationale.

### 1.3 — Interleaver integration
FEC phase 1 shipped RS codes but no interleaver. HF burst errors make RS codes alone insufficient.
- Implement block interleaver in `openpulse-core` paired with the existing `FecCodec`.
- Interleaver depth must be a configurable parameter per mode profile, documented in the mode definition.
- Default depth: minimum 5× the expected maximum burst duration in symbols at the mode's baud rate.
- Add Gilbert-Elliott burst-error channel simulation to the loopback fixture harness.
- Benchmark FEC+interleaver effectiveness under Gilbert-Elliott moderate and heavy burst profiles.
- Confirm FEC+interleaver is a gate in the reduced CI suite for FEC-enabled modes.

### 1.4 — Channel model implementation in benchmarks
Current benchmark harness specifies scenarios by vague name only; channel model parameters are not codified.
- Implement Watterson two-ray channel simulator in the benchmark harness with configurable Doppler spread and delay spread.
- Implement Gilbert-Elliott burst error model in the benchmark harness with configurable state transition probabilities.
- Update all existing scenario YAML files to include explicit `channel_model` and `channel_profile_parameters` fields.
- Add HF500-BURST-03 and HF2300-FADE-02 scenarios (as defined in docs/benchmark-harness.md) to the CI reduced suite.
- Document Watterson and Gilbert-Elliott parameter sets in docs/benchmark-harness.md (done).

### 1.5 — Radio interface layer
OpenPulseHF currently has no PTT control or radio integration beyond the audio path.
- Define and implement `PttController` trait with: no-op (loopback), serial port RTS/DTR, VOX, and CAT/rigctld.
- Implement serial RTS/DTR PTT for Linux (primary target) using the `serialport` crate.
- Implement CAT PTT via Hamlib `rigctld` daemon over TCP.
- Add AFC (automatic frequency control) loop to the BPSK demodulator tracking ±50 Hz offset.
- Expose AFC state (estimated offset in Hz) and audio input level in session diagnostics.
- Add `--ptt` and `--rig` CLI options.
- Integration test: PTT assert/release timing verified within 50 ms budget under loopback.

### 1.6 — Real-device validation
- Complete on-air loopback validation (TX→RX via SDR or second transceiver) for BPSK31 and BPSK250.
- Validate QPSK500 compliance with FCC §97.307(f) on a per-band basis; document restricted bands in CLI output and docs/regulatory.md.
- Publish first real-device benchmark report under docs/raspberry-pi-4-5-tuning-and-benchmarks.md.

---

## Phase 2 — HPX Protocol Completion (Mid term)

**Gate to exit Phase 2:** HPX500 and HPX2300 profiles implemented with adaptive rate control, ACK taxonomy complete, signed handshake verified end-to-end, peer cache and relay operational, benchmarks published against ARDOP on matched bandwidth profiles.

Phase 2 completes the core HPX protocol to a usable and benchmarkable state.

### 2.1 — ACK frame taxonomy and rate adaptation
- Implement the ACK frame taxonomy defined in docs/architecture.md (ACK-OK, ACK-UP, ACK-DOWN, NACK, BREAK, REQ, QRT, ABORT).
- ACK frames must use FSK modulation independent of the current data modulation for robustness at low SNR.
- Implement mid-session rate adaptation: ACK-UP triggers rate increase; ACK-DOWN triggers rate decrease; NACK triggers retransmit at current rate.
- Add rate adaptation integration tests: verify rate stepping under simulated SNR changes.

### 2.2 — HPX500 and HPX2300 adaptive profiles
- Implement HPX500 adaptive profile: rate ladder BPSK31 → BPSK63 → BPSK250 → QPSK250 → QPSK500, selected by ACK-UP/ACK-DOWN feedback.
- Implement HPX2300 adaptive profile: rate ladder QPSK500 → QPSK1000 → 8PSK (TBD) within 2300 Hz occupied bandwidth.
- Implement HPX2300 bandwidth design decision: evaluate OFDM vs single-carrier at this bandwidth class; document decision in docs/architecture.md.
- Benchmark HPX500 against ARDOP 500 and VARA 500 at matched Watterson M1 and M2 channel conditions.
- Benchmark HPX2300 against ARDOP 2000 and VARA 2300 at matched channel conditions.

### 2.3 — Signed handshake and manifest verification
- Implement full HPX session handshake: CONREQ signed with sender Ed25519 key; CONACK signed by responder; trust verification gates session admission.
- Implement signed transfer manifest: file hash, size, sender identity, signature block; verified by receiver before final delivery acknowledgement.
- Integration test: verify handshake rejection on tampered or revoked-key sessions.
- Integration test: verify manifest rejection on payload modification.

### 2.4 — Channel access and shared-channel operation
- Implement DCD (Data Carrier Detect) from demodulated signal energy.
- Implement 0.3-persistence CSMA for broadcast and relay channel access modes.
- Integration test: verify that two simultaneous transmitters in loopback both correctly defer on DCD.

### 2.5 — Peer cache and query subsystem
- Define peer cache schema: signed identity descriptor, capability field, link quality history, age/TTL policy.
- Implement local peer cache store with signed descriptor handling and expiry.
- Implement query engine: local filter queries and bounded network query propagation with hop limit.
- Implement wire-level peer query and response envelope per docs/peer-query-relay-wire.md.

### 2.6 — Multi-hop relay
- Implement relay path planner: trust-scored and link-quality-scored path selection.
- Implement relay forwarding: hop-limited, duplicate-suppressed, loop-prevented.
- Implement relay trust-policy enforcement: each hop verifies trust of the originating station.
- Implement relay observability: relay events logged to session diagnostics.
- Integration test: 3-hop relay path in loopback with trust verification at each hop.

### 2.7 — Compression (session layer)
- Implement lossless payload compression (lz4 recommended: fast, deterministic, low memory).
- Compression negotiated in handshake; not assumed.
- Compress-then-compare: only send compressed frame if compressed size < uncompressed.
- Integration test: compression round-trip with known payload; decompression failure treated as frame error.

---

## Phase 3 — Advanced Signal Processing and Compliance (Mid-to-long term)

**Gate to exit Phase 3:** PQ in-band handshake operational (requires Phase 1 SAR), regulatory compliance validated on-air, GPU acceleration path benchmarked, ARDOP-compatible interface documented.

### 3.1 — Post-quantum in-band handshake
Depends on Phase 1 SAR being complete.
- Implement ML-DSA-44 (FIPS 204) signing and verification for HPX session handshake.
- Implement ML-KEM-768 (FIPS 203) key encapsulation for session key establishment.
- Implement hybrid mode: Ed25519 + ML-DSA-44 dual signature; X25519 + ML-KEM-768 dual KEM.
- Benchmark PQ signature operations on Raspberry Pi 4 (target: signing < 5 ms, verification < 3 ms).
- Note: ML-DSA-44 signatures (2420 bytes) and ML-KEM-768 public keys (1184 bytes) require SAR for in-band transport. Phase 3.1 is blocked on Phase 1.2 (SAR) completion.

### 3.2 — Turbo code FEC evaluation
- Benchmark Turbo code decoder on Raspberry Pi 4 against equivalent RS+interleaver at matched code rate.
- If Turbo codes deliver ≥ 2 dB gain at acceptable CPU cost, add as an optional FEC codec for HPX high-rate profiles.
- Document decision in docs/vara-research.md FEC comparison section.

### 3.3 — GPU acceleration
- Identify DSP operations with sufficient arithmetic intensity for GPU offload: FFT (future OFDM), matched filter banks, Viterbi/Turbo decoder inner loop.
- Implement GPU path using wgpu (Vulkan backend) with CPU fallback for all accelerated operations.
- Benchmark GPU vs CPU on Raspberry Pi 5 (Vulkan support via V3D); publish results.
- GPU path must not introduce latency regressions on CPU-only systems.

### 3.4 — ARDOP-compatible TCP interface
- Define an ARDOP-compatible TCP command/data port interface (command port + data port model).
- This enables existing Winlink and peer-to-peer applications written for ARDOP to work with OpenPulseHF without modification.
- Note: this is an interface compatibility layer, not a protocol compatibility layer. OpenPulseHF transmissions remain HPX on air.
- Legal review required before labelling as "ARDOP-compatible" in public documentation.

### 3.5 — Regulatory on-air validation
- Conduct on-air tests on IARU-aligned frequencies for each supported bandwidth class.
- Verify station identification behaviour at 10-minute intervals under long sessions.
- Test relay node automatic control point interface.
- Publish compliance test report as a release artefact.

---

## Phase 4 — Ecosystem and Long-term (Long term)

Phase 4 is planned but not sprint-scheduled. Items may be promoted earlier based on demand.

### 4.1 — TUI and GUI frontends
- Terminal UI (TUI) using ratatui or similar, implementing the same stable core API as the CLI.
- Optional GUI using a cross-platform toolkit; design goals TBD.
- Both frontends must expose AFC state, audio level, and session diagnostics.

### 4.2 — Observability and automation
- Live link quality display: SNR estimate, AFC offset, retry rate, current rate level.
- Automation hooks: structured JSON event stream on stdout or Unix socket for scripting.
- Periodic HPX performance report publication against maintained benchmark profiles.

### 4.3 — KISS and AX.25 interface
- AX.25/APRS KISS interface for compatibility with APRS applications (PinPoint, APRSISCE/32).
- Three KISS frame modes: standard AX.25, 7-char callsign AX.25, generic data (matching VARA KISS model).
- 0.3-persistence CSMA required for KISS broadcast mode (see Phase 2.4).

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
