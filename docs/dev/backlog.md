---
project: openpulsehf
doc: docs/backlog.md
status: living
last_updated: 2026-05-22
---

# Backlog

All scheduled phases (1–9), far-future items (FF-1 through FF-13), FEC backlog items
(BL-FEC-1 through BL-FEC-6), and all previously documented daemon wiring gaps are
shipped and merged.  See `docs/dev/roadmap.md` for the full history with PR numbers.

---

## Open work items

Ordered by priority.  Items marked **[deferred]** have no target date.

### 8 — Operator transmit-settings auto-tune and restore

Add an OpenPulse helper that snapshots the current rig transmit settings before a test window, applies the known-good data-mode settings for the session, and restores the original values when the run ends.

Scope:

- query and store per-rig `freq`, `mode`, `RFPOWER`, `MICGAIN`, and `COMP` state when available
- apply the test profile settings before TX begins
- restore the saved baseline during cleanup even if a test fails early
- pair the helper with RF/ALC readback so operators can see whether the radio is actually producing output

Why:

- the June 4 session showed that PTT can succeed while RF output remains absent
- compression and mic gain are easy to leave in a bad state between operator sessions
- restoring the original state lowers operator risk and keeps the radios ready for the next task

### 9 — Integrated tuner on high SWR (explicit opt-in)

Add an explicit operator-controlled setting to allow integrated tuner operation when SWR exceeds a configured threshold during on-air runs.

Scope:

- add a runtime/frontend-visible flag for "allow integrated tuner on high SWR"
- gate behavior behind explicit opt-in (default disabled)
- execute tuner attempt only when SWR is above threshold
- apply the same policy after QSY/tune transitions when QSY mode is enabled
- record tuner attempts and outcomes in run logs for operator auditability

Why:

- protects operators from automatic tuner actions unless they explicitly allow it
- avoids repeated manual interventions during high-SWR conditions
- keeps QSY workflows consistent with preflight SWR safety policy

### 1 — FreeDV frame signing (FF-11) ✅ Already shipped

`crates/openpulse-freedv-auth` is complete: `AuthBeacon` (Ed25519 sign/verify),
`FreeDvDataPort` (UDP to FreeDV Qt-GUI data port), `BeaconScheduler` (interval firing),
`TrustVerdict` + `VerdictServer` (Unix socket for UI polling).  5 integration tests pass.
No further work required; close this item.

---

### 2 — Peer deny-list enforcement ✅ Already shipped

`RelayForwarder::forward` returns `RelayForwardError::PolicyRejected` when
`src_peer_id` matches any entry in the `RelayTrustPolicy` deny list (hex strings,
checked via `hex_peer_id` conversion).  Both `openpulse-ardop/src/main.rs` and
`openpulse-kiss/src/main.rs` read `cfg.relay.deny_list` at startup and pass it into
`RelayTrustPolicy::deny_relays`.  Two inline unit tests in `relay.rs` cover the
rejected and allowed-peer paths: `forwarder_rejects_denied_src_peer` and
`forwarder_allows_non_denied_peer_when_deny_list_active`.

---

### 3 — IQ output for OFDM and SC-FDMA plugins ✅ Already shipped

`ofdm_modulate_iq` and `scfdma_modulate_iq` are implemented in
`plugins/ofdm/src/modulate.rs` and `plugins/scfdma/src/modulate.rs`.  Both plugins
override `ModulationPlugin::modulate_iq()`.  Both OFDM and SC-FDMA use Hermitian
symmetry (real IFFT output) so Q is identically zero; the interleaved output is
`[I₀, 0, I₁, 0, …]`.  Round-trip tests `ofdm16_iq_round_trip` and
`scfdma52_iq_round_trip` pass.

---

### 4 — GPU extensions: QPSK correlator + modulate-side RRC ✅ Already shipped

`QpskPlugin::with_gpu()` constructor exists; `openpulse_gpu::gpu_rrc_fir` is dispatched
inside `qpsk_modulate` via `#[cfg(feature = "gpu")]`, replacing the CPU RRC convolution.
CPU vs GPU equivalence test in `plugins/qpsk/src/modulate.rs` asserts max sample delta
< 1e-4.  `cargo test --package qpsk-plugin --no-default-features` passes unchanged (PR #325).

---

### 5 — SC-FDMA adaptive pilot density ✅ Shipped (PR #335)

`AdaptivePilotState` (EMA α=0.3), `ScFdmaParams::with_pilot_density()`, and
`estimate_coh_bw_hz()` lag-1 pilot correlation estimator.  `ScFdmaPlugin::estimate_afc_hz`
feeds coherence BW into the adaptive state; `adaptive_params_for_mode()` returns adjusted
params.  Tests: flat → sparse, delay-26 2-tap (B_c ≈ 57 Hz) → dense, EMA reversion.

---

### 6 — On-device tuning/calibration wizard ✅ Shipped (PR #336)

`openpulse calibrate audio|ptt|afc` subcommands wired into `openpulse-cli`.  All three
tests run against the loopback backend; optional `--output <path>` writes JSON.
4 integration tests pass.

---

### 7 — Turbo codes ✅ Shipped (PR #337)

`crates/openpulse-core/src/turbo.rs`: `TurboCodec` with `encode(data: &[u8]) -> Vec<u8>` and
`decode(llrs: &[f32]) -> Result<Vec<u8>, ModemError>`.  Rate-1/3 PCCC, RSC G1={1,1,1} G2={1,0,1},
3GPP TS 36.212 QPP interleaver (K=40–6144), Max-Log-MAP BCJR, 8 iterations, CRC-16 early exit.
`FecMode::Turbo` (strength=8) wired into `transmit_with_fec_mode` / `receive_with_fec_mode`.
BER ≤ 0.01 at Eb/N0 = 2 dB for 256-bit blocks confirmed by `tests/turbo_ber.rs`.

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

- Turbo codec: rate-1/3 PCCC `TurboCodec`, Max-Log-MAP BCJR, 8 iterations, `FecMode::Turbo` wired into engine dispatch (PR #337).
- Peer deny-list enforcement: `RelayForwarder::forward` returns `PolicyRejected` for deny-listed `src_peer_id`; ARDOP and KISS bridges wire `cfg.relay.deny_list` via `RelayTrustPolicy::deny_relays`; two unit tests in `relay.rs`.
- IQ output for OFDM and SC-FDMA: `ofdm_modulate_iq` / `scfdma_modulate_iq` implemented; both plugins override `modulate_iq()`; round-trip tests pass.
- GPU QPSK modulate-side RRC: `QpskPlugin::with_gpu()`, `gpu_rrc_fir` dispatch in `qpsk_modulate`, CPU/GPU equivalence test (PR #325).
- On-device calibration wizard: `openpulse calibrate audio|ptt|afc`; loopback-only, JSON output via `--output` (PR #336).
- SC-FDMA adaptive pilot density: `AdaptivePilotState`, `estimate_coh_bw_hz()`, `ScFdmaParams::with_pilot_density()` (PR #335).
- OFDM16/52 GPU hard+soft demodulation via `gpu_fft256_batch`; `OfdmPlugin::with_gpu()` constructor (PR #330).
- README expanded with modulation/MAC/compression/ARQ/FEC/GPU feature tables; first-to-market table with 12 entries; PayPal sponsor badge restored (PRs #327–#329).
- QSY incoming event (`QsyIncoming` `ControlEvent`), 64-byte token length bound, e2e initiator→responder test, SC-FDMA GPU soft-demod (`scfdma_demodulate_soft_gpu`), `CHANGELOG.md` created (PR #326).
- GPU RRC FIR convolution kernel and 256-pt FFT/IFFT kernel wired into BPSK, QPSK, 8PSK, SC-FDMA, 64QAM plugins (PR #325).
- GPU soft-demod kernels for 64QAM and 8PSK via wgpu (PR #324).
- Daemon QSY RF wiring: `QsySession` wired into `AcceptQsy`; QSY_REQ/LIST frames transmitted; `process_received_bytes` drives responder role (PR #321).
- Daemon CrossBandRepeater wiring: `EnableRepeater`/`DisableRepeater` daemon commands (PR #321).

For full completion history (Phases 0-9, FF series, BL-FEC series), use `docs/dev/roadmap.md`.

