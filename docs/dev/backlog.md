---
project: openpulsehf
doc: docs/backlog.md
status: living
last_updated: 2026-05-20
---

# Backlog

All scheduled phases (1–9), far-future items (FF-1 through FF-13), FEC backlog items
(BL-FEC-1 through BL-FEC-6), and all previously documented daemon wiring gaps are
shipped and merged.  See `docs/dev/roadmap.md` for the full history with PR numbers.

---

## Open work items

Ordered by priority.  Items marked **[deferred]** have no target date.

### 1 — FreeDV frame signing (FF-11) ✅ Already shipped

`crates/openpulse-freedv-auth` is complete: `AuthBeacon` (Ed25519 sign/verify),
`FreeDvDataPort` (UDP to FreeDV Qt-GUI data port), `BeaconScheduler` (interval firing),
`TrustVerdict` + `VerdictServer` (Unix socket for UI polling).  5 integration tests pass.
No further work required; close this item.

---

### 2 — Peer deny-list enforcement

**Goal:** `openpulse-config` has `relay.deny_list: Vec<String>` (peer IDs as hex) that
parses correctly but is never read.  Wire it into `RelayForwarder` so frames from
deny-listed peers are dropped at the first hop with a `PolicyRejected` event.

**Acceptance criteria:**
- `RelayForwarder::new` accepts `deny_list: Vec<[u8; 32]>` (pre-parsed peer IDs).
- `RelayForwarder::try_forward` returns `RelayForwardError::PolicyRejected` when
  `src_peer_id` is in the deny list.
- Daemon wires config deny list into `RelayForwarder` at startup.
- Two unit tests: denied peer is rejected; allow-listed peer forwards normally.

---

### 3 — IQ output for OFDM and SC-FDMA plugins

**Goal:** `ModulationPlugin::modulate_iq()` already exists and BPSK uses it for direct
SDR upconversion.  OFDM and SC-FDMA plugins return the default empty-vec fallback.

**Acceptance criteria:**
- `ofdm_modulate_iq(payload, mode) -> Vec<f32>` — interleaved I/Q stereo output,
  identical carrier content to `ofdm_modulate`, at complex baseband (fc = 0 Hz).
- `scfdma_modulate_iq(payload, mode) -> Vec<f32>` — same convention.
- Both plugins override `ModulationPlugin::modulate_iq()` to call the new functions.
- Round-trip test for each: I/Q samples demodulate correctly when mixed to passband.

---

### 4 — GPU extensions: QPSK correlator + modulate-side RRC

**Goal:** GPU RRC FIR kernel exists for the receive path.  The modulate path still runs
CPU-side RRC for QPSK.  Low-priority given 1000 baud maximum, but worthwhile for
pipeline symmetry.

**Acceptance criteria:**
- `openpulse_gpu::gpu_rrc_fir` wired into `qpsk_modulate` via `#[cfg(feature = "gpu")]`
  dispatch, replacing the CPU convolution.
- `QpskPlugin::with_gpu()` constructor (mirrors `BpskPlugin::with_gpu`).
- CPU vs GPU modulate equivalence test (max sample delta < 1e-4).
- `cargo test --package qpsk-plugin --no-default-features` passes unchanged.

---

### 5 — SC-FDMA adaptive pilot density (PR #335, pending merge)

`AdaptivePilotState` (EMA α=0.3), `ScFdmaParams::with_pilot_density()`, and
`estimate_coh_bw_hz()` lag-1 pilot correlation estimator.  `ScFdmaPlugin::estimate_afc_hz`
feeds coherence BW into the adaptive state; `adaptive_params_for_mode()` returns adjusted
params.  Tests: flat → sparse, delay-26 2-tap (B_c ≈ 57 Hz) → dense, EMA reversion.

---

### 6 — On-device tuning/calibration wizard

**Goal:** New `openpulse-cli` subcommand `calibrate` that guides an operator through
audio level, PTT timing, and AFC offset setup without RF transmission.

**Acceptance criteria:**
- `openpulse calibrate audio` — plays a 1500 Hz tone at −18 dBFS, measures input
  level, reports clip headroom.
- `openpulse calibrate ptt` — asserts PTT, measures assert-to-audio latency via
  loopback, reports result vs 50 ms target.
- `openpulse calibrate afc` — plays a known BPSK250 burst into the audio loopback,
  runs `estimate_afc_hz`, reports measured offset.
- All subcommands write a machine-readable JSON summary to `--output <path>`.
- No RF transmission required; CPAL loopback backend sufficient.

---

### 7 — Turbo codes

**Goal:** Rate-1/3 Parallel Concatenated Convolutional Code (PCCC, 3GPP Turbo) as an
optional FEC mode offering higher coding gain than LDPC for the short block sizes
(≤ 256 bits) common in HF ARQ retransmissions.

**Acceptance criteria:**
- `crates/openpulse-core/src/turbo.rs`: `TurboCodec` with `encode(data: &[u8]) -> Vec<u8>`
  and `decode(llrs: &[f32]) -> Result<Vec<u8>, FecError>`.
- Rate-1/3, constituent RSC encoders G1={1,1,1}, G2={1,0,1} (generators 0x7, 0x5 octal),
  internal interleaver length 40–6144 bits (3GPP TS 36.212 table).
- Max-Log-MAP BCJR decoder, 8 iterations, early-exit on CRC pass.
- BER ≤ 0.01 at Eb/N0 = 2 dB for 256-bit blocks (AWGN loopback test).
- Wired into `transmit_with_fec_mode` / `receive_with_fec_mode` as `FecMode::Turbo`.
- `cargo test --package openpulse-core --no-default-features` passes; turbo BER test
  joins the FEC comparison suite.

---

### Deferred (no target date)

| Item | Reason |
|---|---|
| On-air regulatory validation (Phase 5.5-reg) | Requires licensed station and coordinated test schedule |

#### On-air regulatory validation execution checklist

When station access is available, run this checklist before marking Phase 5.5-reg complete.

1. Operator and station readiness
  - Confirm licensed control operator is assigned for each test window.
  - Confirm frequency plan uses IARU-aligned allocations for each target region.
  - Confirm station ID cadence meets local rules (10-minute interval and end-of-contact).
2. Hardware and software readiness
  - Verify audio/PTT path with `openpulse-kisstnc` or `openpulse-tnc` using CPAL backend.
  - Verify rig CAT/PTT control and fail-safe PTT release behavior.
  - Capture exact software revision (`git rev-parse HEAD`) and active config snapshot.
3. Required test matrix (minimum)
  - HF narrowband baseline: BPSK250 and QPSK500 on clean and typical live channel conditions.
  - Adaptive profile run: confirm ACK/NACK-driven transitions remain policy-safe on-air.
  - Gateway/interoperability run: one end-to-end message session with logs retained.
4. Evidence capture
  - Record timestamped logs, selected frequencies, mode transitions, and operator notes.
  - Export benchmark/test artifacts to `docs/dev/test-reports/on-air/` with scenario labels.
  - Build a per-run evidence bundle with `./scripts/onair-bundle-evidence.sh`.
  - Use `--require-report --require-config --require-preflight` for compliance runs.
  - Document any compliance exceptions and mitigations.
5. Completion criteria
  - No unresolved compliance exceptions.
  - Stable on-air sessions across the required matrix.
  - Follow-up docs updated: `docs/dev/roadmap.md`, `docs/releasenotes.md`, and compliance notes.

---

## Recently completed (summary)

- SC-FDMA adaptive pilot density: `AdaptivePilotState`, `estimate_coh_bw_hz()`, `ScFdmaParams::with_pilot_density()` (PR #335).
- OFDM16/52 GPU hard+soft demodulation via `gpu_fft256_batch`; `OfdmPlugin::with_gpu()` constructor (PR #330).
- README expanded with modulation/MAC/compression/ARQ/FEC/GPU feature tables; first-to-market table with 12 entries; PayPal sponsor badge restored (PRs #327–#329).
- QSY incoming event (`QsyIncoming` `ControlEvent`), 64-byte token length bound, e2e initiator→responder test, SC-FDMA GPU soft-demod (`scfdma_demodulate_soft_gpu`), `CHANGELOG.md` created (PR #326).
- GPU RRC FIR convolution kernel and 256-pt FFT/IFFT kernel wired into BPSK, QPSK, 8PSK, SC-FDMA, 64QAM plugins (PR #325).
- GPU soft-demod kernels for 64QAM and 8PSK via wgpu (PR #324).
- Daemon QSY RF wiring: `QsySession` wired into `AcceptQsy`; QSY_REQ/LIST frames transmitted; `process_received_bytes` drives responder role (PR #321).
- Daemon CrossBandRepeater wiring: `EnableRepeater`/`DisableRepeater` daemon commands (PR #321).

For full completion history (Phases 0-9, FF series, BL-FEC series), use `docs/dev/roadmap.md`.

