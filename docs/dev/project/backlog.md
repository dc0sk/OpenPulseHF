---
project: openpulsehf
doc: docs/dev/project/backlog.md
status: living
last_updated: 2026-10-15
---

# Backlog

> **The plan of record is [`workplan.md`](workplan.md).** It decides what is worked on: every open
> item belongs to a Release 1 milestone (M0–M4) or to `post-release`. Nothing in this file is
> scheduled unless the work plan says so. Every item below is `post-release` under the work plan's
> scope (decision 1) except where noted; the issue tracker's `post-release` and
> `post-release-rerank` labels are the live list.

**Scope:** this file is the register of *open* work. What 1.0 requires (and explicitly does not) is
proposed in [release-1.0-criteria.md](release-1.0-criteria.md). Completed work lives in
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

### 8 — Operator transmit-settings auto-tune and restore (`post-release`)

Partly covered since 2026-10-14: the on-air runners only ever lower a rig's RF power
(`scripts/lib/rig-power.sh`) and keep or check the rig's mode per station. A general
snapshot-and-restore helper is still open.

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

### 9 — Integrated tuner on high SWR (explicit opt-in) (`post-release`)

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

### 11 — Control-channel security (REQ-CTL-01..05) — ✅ shipped for TCP; two follow-ups open

Closed by CAP-68 for the TCP control channel: `openpulse-linksec` Noise `NNpsk0`, the non-loopback
`auth_required` fail-closed gate, `openpulse-keystore` (Argon2id + ChaCha20-Poly1305 file keystore and
OS-keychain `SecretStore`), and owner-only secret-file enforcement. The earlier "open decision:
TLS-PSK vs Noise" was resolved in favour of Noise and is no longer open.

**Still open (`post-release`):** WebSocket transport auth/encryption. Sourcing the control PSK from
the keystore was **closed by decision** (#1234, maintainer 2026-09-20): the PSK stays in
`OPENPULSE_CONTROL_PSK`, and `openpulse-keystore` stays unwired. Detail in [traceability-matrix.md](traceability-matrix.md) (CAP-68, slice 4).

### 12 — Wide-channel (VHF/UHF) 12.5/25 kHz support (REQ-BW-01..07) — `post-release`, after 1.0

Extend the modem beyond its ~2.7 kHz HF SSB channel to 12.5 kHz and 25 kHz VHF/UHF-class channels.
Targeted at a future **1.x** release. Feasibility + phased action list in
[docs/dev/design/wide-channel-extension.md](../design/wide-channel-extension.md).

Phase 0 (decisions, blocking): pick the RF path (direct-IQ SDR vs linear wide exciter vs
constant-envelope 4FSK), target sample rates, wideband strategy (clock-scaling vs new FFT layout),
PAPR/PA-linearity, and AFC budget at VHF/UHF. Phase 1: sample-rate generalization (parameterize the
engine off the hard-coded 8 kHz; this is what would revive the 9600-baud HD waveforms, whose
`hpx_narrowband_hd` profile was retired on 2026-09-14 because it could never run at 8 kHz — the
implementations and their 48 kHz loopback tests are kept for exactly this phase, #1359). Phase 2: wide modes (clock-scaled
OFDM/SC-FDMA at 48/96 kHz; RX-IQ path) + `hpx_wide12`/`hpx_wide25` ladders. Phase 3: VHF/UHF bandplan
+ regulatory + a mobile-fading channel model + recalibrated floors.

Why:

- much groundwork already exists (rate-parameterized single-carrier plugins, 9600-baud modes, the TX
  IQ seam), but 8 kHz is de-facto hard-coded in the engine so wide modes cannot run; since work plan
  decision 18 the only profiles are `fast` and `robust`, so a wide ladder would be a new profile;
- the deepest question is RF architecture, not code — a wide linear waveform passes neither an SSB
  rig's 3 kHz path nor an FM class-C PA, so a direct-IQ SDR path (RX half missing) or linear exciter
  is required. This must be decided before implementation.

### 13 — FF-15 JS8 discovery: Phase H (on-air) **[deferred]** (`post-release`; discovery/JS8 is outside Release 1)

Phases A–G are shipped (PRs #744–#805); only on-air validation remains. Off by default behind
`[discovery] mode` + a callsign + the ±2 s clock-skew / DCD / self-ID gates. Needs real radios.

### 14 — FF-16 file transfer: Phase F (on-air) **[deferred]** (`post-release`; file transfer is experimental in Release 1)

Phases A–E are shipped (PRs #730–#743, #787); since 2026-10-02 a transfer also survives a lost
fragment, a lost ack and a long block (`filexfer_lossy_link`). Only on-air validation remains. Needs
real radios. Work plan decision 19 may turn file transfer on for the M4 UI client; that decision is
taken there, not here.

### 15 — Modem audit deferred findings (issue #917) **[deferred]**

Tail of the 2026-07-16 modem loose-ends audit; the cpal double-capture item is hardware-gated.

### On-air validation — moved to the work plan

The former "On-air regulatory validation (Phase 5.5-reg)" execution checklist is superseded by work
plan milestone **M3** (on air on **2 m**, IC-9700 ↔ FT-818: G0–G3, A1, A2, A4/A5, one ARDOP Pat
session) and the HF run with the release candidate in **M4** (decision 13). The gate procedure is in
[onair-execution-plan.md](../onair-execution-plan.md); what waits on the maintainer is in
[maintainer-todo.md](maintainer-todo.md).

---

## Completed work

Not listed here — see [roadmap.md](roadmap.md) for the full phase history with PR numbers, and
[traceability-matrix.md](traceability-matrix.md) for per-requirement (REQ/CAP) status. This file
previously carried a duplicate digest of shipped items, which is how items 1–7 and 10–11 came to be
listed as "open work" long after they closed.
