---
project: openpulsehf
doc: docs/backlog.md
status: living
last_updated: 2026-05-19
---

# Backlog

All scheduled phases (1–9), far-future items (FF-1 through FF-13), FEC backlog items
(BL-FEC-1 through BL-FEC-6), and all previously documented daemon wiring gaps are
shipped and merged.  See `docs/roadmap.md` for the full history with PR numbers.

---

## Open work items

None.

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
  - Follow-up docs updated: `docs/roadmap.md`, `docs/releasenotes.md`, and compliance notes.

---

## Recently completed (summary)

- Daemon QSY RF wiring: `QsySession` state machine wired into `AcceptQsy` handler; QSY_REQ and QSY_LIST frames transmitted via modem engine (PR #321).
- Daemon CrossBandRepeater wiring: `CrossBandRepeater` pre-built in `main.rs` with plugin-registered engines; `EnableRepeater` spawns worker thread; `DisableRepeater` stops and joins it (PR #321).
- SC-FDMA 64QAM promoted into `hpx_wideband_hd` (SL14); SNR gate at SL14 protecting SL15 ceiling (PR #320).
- Winlink Type C LZHUF ISS compatibility: `queue_message_type_c` uses `compress_lzhuf_winlink` (LE prefix) for wire-compatible ISS sends (PR #320).
- ARQ retry loop resolves adaptive mode before each retry; PTT hard-failure no longer emits spurious `PttChanged` event (PR #319).

For full completion history (Phases 0-9, FF series, BL-FEC series), use `docs/roadmap.md`.
