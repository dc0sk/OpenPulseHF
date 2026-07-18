---
project: openpulsehf
doc: docs/dev/project/backlog.md
status: living
last_updated: 2026-07-08
---

# Backlog

**Scope:** this file is the register of *open* work. Completed work lives in
[roadmap.md](roadmap.md) (phase history with PR numbers) and per-requirement status lives in
[traceability-matrix.md](traceability-matrix.md) (REQ/CAP coverage). Do not re-list shipped items
here — an entry in this file should be something somebody could still pick up.

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

### 10 — Observability / audit mode (REQ-OBS-01..03) ✅ Shipped

Closed by CAP-67 (2026-07-05): `[observability] audit_mode` → `events.ndjson` + startup
`snapshot.json`, rotating file logging via `[logging] file`, and `openpulse audit-bundle`.
Status detail lives in [traceability-matrix.md](traceability-matrix.md) (CAP-67).

### 11 — Control-channel security (REQ-SEC-CTL-01..05) — ✅ shipped for TCP; two follow-ups open

Closed by CAP-68 for the TCP control channel: `openpulse-linksec` Noise `NNpsk0`, the non-loopback
`auth_required` fail-closed gate, `openpulse-keystore` (Argon2id + ChaCha20-Poly1305 file keystore and
OS-keychain `SecretStore`), and owner-only secret-file enforcement. The earlier "open decision:
TLS-PSK vs Noise" was resolved in favour of Noise and is no longer open.

**Still open:** WebSocket transport auth/encryption, and sourcing the control PSK from the keystore
rather than config. Detail in [traceability-matrix.md](traceability-matrix.md) (CAP-68, slice 4).

### 12 — Wide-channel (VHF/UHF) 12.5/25 kHz support (REQ-BW-01..07) — release 1.x

Extend the modem beyond its ~2.7 kHz HF SSB channel to 12.5 kHz and 25 kHz VHF/UHF-class channels.
Targeted at a future **1.x** release. Feasibility + phased action list in
[docs/dev/design/wide-channel-extension.md](../design/wide-channel-extension.md).

Phase 0 (decisions, blocking): pick the RF path (direct-IQ SDR vs linear wide exciter vs
constant-envelope 4FSK), target sample rates, wideband strategy (clock-scaling vs new FFT layout),
PAPR/PA-linearity, and AFC budget at VHF/UHF. Phase 1: sample-rate generalization (parameterize the
engine off the hard-coded 8 kHz; unblocks `hpx_narrowband_hd`). Phase 2: wide modes (clock-scaled
OFDM/SC-FDMA at 48/96 kHz; RX-IQ path) + `hpx_wide12`/`hpx_wide25` ladders. Phase 3: VHF/UHF bandplan
+ regulatory + a mobile-fading channel model + recalibrated floors.

Why:

- much groundwork already exists (rate-parameterized single-carrier plugins, 9600-baud modes, the TX
  IQ seam, and `hpx_narrowband_hd`), but 8 kHz is de-facto hard-coded in the engine so the wide
  profiles cannot run;
- the deepest question is RF architecture, not code — a wide linear waveform passes neither an SSB
  rig's 3 kHz path nor an FM class-C PA, so a direct-IQ SDR path (RX half missing) or linear exciter
  is required. This must be decided before implementation.

### 13 — FF-15 JS8 discovery: Phase H (on-air) **[deferred]**

Phases A–G are shipped (PRs #744–#805); only on-air validation remains. Off by default behind
`[discovery] mode` + a callsign + the ±2 s clock-skew / DCD / self-ID gates. Needs real radios.

### 14 — FF-16 file transfer: Phase F (on-air) **[deferred]**

Phases A–E are shipped (PRs #730–#743, #787); only on-air validation remains. Needs real radios.

### 15 — Modem audit deferred findings (issue #917) **[deferred]**

Tail of the 2026-07-16 modem loose-ends audit; the cpal double-capture item is hardware-gated.

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

## Completed work

Not listed here — see [roadmap.md](roadmap.md) for the full phase history with PR numbers, and
[traceability-matrix.md](traceability-matrix.md) for per-requirement (REQ/CAP) status. This file
previously carried a duplicate digest of shipped items, which is how items 1–7 and 10–11 came to be
listed as "open work" long after they closed.
