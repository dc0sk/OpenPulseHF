---
project: openpulsehf
doc: docs/dev/design/fallback-onset-ranking.md
status: draft
last_updated: 2026-10-02
---

# Rank the uncoded fallback's onsets by preamble correlation

Work plan M2, receive-cost item (found 2026-10-01, measured 2026-10-02).

## Problem — measured

`ota_decode_and_ack_inner` decodes a flushed burst in this order:
1. every coded rung candidate at offset 0;
2. the **#1123 uncoded fallback**: `decode_burst_phase1` at the station's `[modem] mode` (default
   `BPSK250`), an exhaustive decode-driven onset scan;
3. the **#1138 coded onset scan**, then HARQ, then phase 2.

Every flushed burst carries a lead-in (the ring prepended at open, 2–4 k samples), so step 1 almost
never succeeds. Step 2 then runs for **every coded ladder frame**. A coded frame can never decode
uncoded, so the scan exhausts every onset: about 128 attempts at a 32-sample step up to
`scan_end` ≈ 4 096 samples. **Each attempt demodulates a whole 74 624-sample slice.**

`crates/openpulse-modem/tests/receive_cost_scaling.rs` (release build, x86 container, fallback `BPSK250`
as the daemon passes it) gives the decode time per frame:

| rung | frame audio | decode | decode, fallback scan removed |
|---|---|---|---|
| SL6 QPSK250-D + Rs | 4.2 s | 0.85 s | — |
| SL5 BPSK250 + Rs | 8.3 s | 1.39 s | — |
| SL2 BPSK31 + Rs | 66.6 s | 1.72 s | 0.77 s |

About 1 s of every coded frame's decode is the fallback scan finding nothing. Debug builds take ~80 s
per frame through two daemons. On the 2 m stations (Raspberry Pis, several times slower than the
container) the decode plus the ACK's airtime must fit the ISS's 9 s ACK window
(`ota_ack_timeout_ms`, both profiles carry MFSK16). The station measurement is pending.

The order was deliberate. With the coded scan first, an uncoded filexfer fragment cost 116.5 s, because
that scan exhausted every onset instead (`engine.rs`, the comment above the onset scan). **Both scans are
decode-driven and exhaustive, so whichever runs first pays its full cost on the other class of
traffic.** Reordering moves the cost; it does not remove it.

## Decision (proposed)

Make the fallback scan **rank** its onsets instead of trying all of them, when the fallback mode has a
preamble matched filter:

1. Run one matched-filter correlation of the mode's preamble template over
   `[0, scan_end + template_span)`, across the existing residual-frequency grid
   (`preamble_search_plan`), and take the **top K local maxima of ρ** as candidate onsets.
2. Attempt the uncoded decode only at those onsets, best first. The demodulator's own timing search
   spans about ±1.5 symbols, so an onset within a symbol of the peak is enough.
3. If none decodes, the fallback has failed. It does **not** fall back to the exhaustive scan;
   otherwise every coded burst pays the full cost again.

Properties:
- **Ranking, not thresholding.** No ρ threshold decides anything, so the fragile per-mode ρ constants
  (#1062, #1060) and the runtime calibration are not touched.
- **No calibration samples.** It calls the correlation directly, not `decide_preamble_veto`, so it
  adds nothing to the calibration stream (#1342) and does not move the stand-down latch.
- **Modes without a template keep today's exhaustive scan.** Only `BPSK250` has one
  (`MODES_WITH_VETO`, pinned by `tests/veto_membership_pin.rs`), and it is the default `[modem] mode`
  and so the default fallback. Other fallback modes are unchanged, bit for bit.
- **The coded scan (step 3) is unchanged.** It is already cheap: 0.77 s at SL2 including everything else.
- **K is a new constant.** It must be derived from data, not chosen. Proposed: the smallest K for
  which every currently-passing non-ladder reception test still passes. Then measure the true onset's
  rank on the recorded on-air corpus (`daemon_vs_cli_on_real_captures`), which has real onsets at
  224 and 4 032 samples, and set K to twice the worst rank observed.

## Risks and how each is checked

- **A control frame whose true onset is not in the top K is lost.** That is a regression in receiving
  station ID, filexfer, handshake, QSY and relay. Checks: the existing reception tests
  (`twin_daemon_bridge` filexfer with OTA on, the station-ID and handshake suites) and the on-air
  corpus replay, plus the rank measurement above. At low SNR a missed preamble peak usually means the
  decode would also fail, but that is UNCHECKED and has to be measured.
- **The correlation costs something.** A 992-sample passband template × ~5 k lags × the grid is a few
  tens of millions of MACs, roughly tens of ms in release. Measured by the same probe, before and after.
- **Acquisition-chain change.** Per CLAUDE.md, `scripts/slow-tests.sh` runs before merge: notch
  REQ-QRM-01, OTA CAP-33 and the spectral decode counts.

## Consumer

`ota_decode_and_ack_inner` (fallback step) → `decode_burst_phase1` → `scan_burst_onsets`, reached on the
daemon receive path through `server.rs`'s `ota_decode_burst(&burst, …, Some(&mode))`. Found by
`grep -n "decode_burst_phase1\|ota_decode_burst" crates/openpulse-modem/src/engine.rs crates/openpulse-daemon/src/server.rs`.

## Prior art

- `PreambleVeto` / `preamble_rho` / `preamble_search_plan` (engine.rs), the matched filter and
  frequency grid this reuses.
- #1118 split the fallback to phase 1 only, for the same cost reason (129 settles that changed no
  verdict).
- #1138 placed the coded onset scan after the fallback.

Found by `grep -n "fn preamble_rho\|fn build_preamble_veto\|#1118\|#1138" crates/openpulse-modem/src/engine.rs`.

## Twins

`decode_burst_inner` (the CLI and TNC uncoded and coded entry) runs the same exhaustive phase-1 scan
on its own bursts. It is not on the daemon's per-frame path and is left unchanged here; a follow-up
could apply the same ranking. Found by `grep -n "scan_burst_onsets(" crates/openpulse-modem/src/engine.rs`
(3 call sites).
