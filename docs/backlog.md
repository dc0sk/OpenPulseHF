---
project: openpulsehf
doc: docs/backlog.md
status: living
last_updated: 2026-05-15
---

# Backlog

All scheduled phases (1–9), far-future items (FF-1 through FF-13), and FEC backlog items
(BL-FEC-1 through BL-FEC-6) are shipped and merged.  See `docs/roadmap.md` for the full
history with PR numbers.

---

## Open work items

### Adaptive equalizer LMS/DFE 🔄 In Progress

Follow-on to FF-3 RRC for robust 1000 baud operation under Watterson Moderate/Poor channels.

Kickoff status:
- ✅ Initial QPSK demod-path LMS equalizer wiring landed in plugin demodulation pipelines (hard + soft paths).
- ✅ Baseline validation passed on `qpsk-plugin` unit tests and `openpulse-modem` QPSK hardening integration tests.
- 🔄 Next: channel-stress validation on Watterson Moderate/Poor fixtures and DFE/pilot tuning.

### Deferred (no target date)

| Item | Reason |
|---|---|
| On-air regulatory validation (Phase 5.5-reg) | Requires licensed station and coordinated test schedule |
| 64QAM / SL12–SL20 speed levels | Deferred pending equalizer and OFDM research |
| External Winlink Type C LZHUF compatibility | 4-byte length prefix differs from Winlink convention; deferred |

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
  - Export benchmark/test artifacts to `docs/test-reports/on-air/` with scenario labels.
  - Document any compliance exceptions and mitigations.
5. Completion criteria
  - No unresolved compliance exceptions.
  - Stable on-air sessions across the required matrix.
  - Follow-up docs updated: `docs/roadmap.md`, `docs/releasenotes.md`, and compliance notes.

---

## Recently completed (summary)

- Bandplan awareness for QSY and operating mode shipped (PRs #235, #236, #237).
- Release packaging workflow shipped (PR #231).

For full completion history (Phases 0-9, FF series, BL-FEC series), use `docs/roadmap.md`.
