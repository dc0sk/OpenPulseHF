---
project: openpulsehf
doc: docs/dev/project/backlog.md
status: living
last_updated: 2026-07-06
---

# Backlog

All scheduled phases (1–9), far-future items (FF-1 through FF-13), FEC backlog items
(BL-FEC-1 through BL-FEC-6), and all previously documented daemon wiring gaps are
shipped and merged.  See `docs/dev/project/roadmap.md` for the full history with PR numbers.

Completed research is archived in docs/dev/archive/ (FEC evaluation, waveform evaluation).

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

### 10 — Observability / audit mode (REQ-OBS-01..03)

Give users an opt-in way to persist logs and structured events to disk during a run and collect a
diagnostic bundle to hand to a developer, so gaps/bugs/errors can be analysed after the fact. Today
the building blocks exist but are scattered (human-readable logs to stdout only; `openpulse monitor`
NDJSON must be piped by hand; the daemon event stream isn't recorded server-side; `session-metrics`
is pull-only; the `onair-bundle-evidence.sh` collector is RF-test-specific).

Scope (sliced smallest-risk first):

1. **Persistent rotating file logging (REQ-OBS-02):** `[logging] file` config path; a shared
   `openpulse_config::logging::init_tracing()` helper that tees `tracing` to a daily-rolled,
   non-blocking file appender in addition to stdout; wire the daemon first, then the other binaries.
2. **Event-stream capture (REQ-OBS-01):** when audit mode is on, the daemon records its own
   `ControlEvent`/`EngineEvent` stream to `events.ndjson` and auto-dumps `SessionDiagnostics` per
   session — no live client required.
3. **Startup snapshot (REQ-OBS-01):** write `snapshot.json` (config with secrets redacted, version,
   git SHA, system info) into the archive dir at startup.
4. **`openpulse audit-bundle` command (REQ-OBS-03):** package logs + events + session diagnostics +
   snapshot into a single archive, generalising `onair-bundle-evidence.sh` to everyday runs.

Why:

- users can currently only reconstruct a run by manually orchestrating several commands/scripts;
- traces vanish on restart (no on-disk log), and failed sessions leave no crash/error archive;
- a one-switch audit mode + one-command bundle turns "it broke" into an analysable artifact.

### 11 — Control-channel security (REQ-SEC-CTL-01..05)

Authenticate + encrypt the daemon ↔ panel/client control channel, and give secrets a safe home. Today
the control port (TCP :9000 / WS :9001) is plaintext with no auth — safe on the default loopback bind,
but wide open (transmitter control!) if bound to a non-loopback address for remote operation. K4remote's
TLS-PSK client is the reference. Full design + threat model in
[docs/dev/design/control-channel-security.md](../design/control-channel-security.md).

Scope (sliced smallest-risk first):

1. **Shared file-permission helper (REQ-SEC-CTL-05):** lift the owner-only `0600`/`0700`
   validate+enforce logic out of `openpulse-cli/src/state.rs` into a shared module; apply it to the
   identity key + trust store on **both** daemon (server) and clients (client) — refuse group/world-
   readable secret files, set owner-only on write.
2. **File keystore + master password (REQ-SEC-CTL-04):** Argon2id KDF + AEAD (ChaCha20-Poly1305)
   keystore; master password never persisted; owner-only files via slice 1.
3. **System keychain backend (REQ-SEC-CTL-03):** `keyring`-backed store (Secret Service / macOS
   Keychain / Windows Credential Manager) behind a trait; falls back to the file keystore.
4. **TLS-PSK transport (REQ-SEC-CTL-01/02):** rustls external-PSK server (daemon) + client (panel);
   required on non-loopback bind, optional on loopback; transmitter-keying commands fail closed for
   unauthenticated clients.

Each slice ships behind config, off by default. **Open decisions to confirm before coding:** TLS-PSK vs
PSK-token; Argon2id + ChaCha20-Poly1305 primitives; whether the panel prompts for the master password
in-UI or reads the OS keychain only.

Why:

- the control channel commands the transmitter; on a non-loopback bind it is unauthenticated and
  unencrypted — a safety/regulatory hazard (unauthorised emission), not just a confidentiality one;
- the RF link is heavily secured while this, the more network-exposed surface, is not;
- operators want to run the panel from another machine (the WebSocket transport already anticipates it).

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

### In active execution

| Item | Status |
|---|---|
| On-air regulatory validation (Phase 5.5-reg) | In active execution (see [onair-status.md](../onair-status.md)) — started 2026-06-10 |

#### On-air regulatory validation execution checklist

This checklist is being worked through as part of the active on-air execution; see
[onair-status.md](../onair-status.md) for current progress.  Run it to completion before
marking Phase 5.5-reg complete.

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
  - Follow-up docs updated: `docs/dev/project/roadmap.md`, `docs/releasenotes.md`, and compliance notes.

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

For full completion history (Phases 0-9, FF series, BL-FEC series), use `docs/dev/project/roadmap.md`.

