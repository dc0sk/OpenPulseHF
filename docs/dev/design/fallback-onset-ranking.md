---
project: openpulsehf
doc: docs/dev/design/fallback-onset-ranking.md
status: review
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

At SL2, about 1 s of the decode is the fallback scan finding nothing. At SL6 the whole decode is
0.85 s, so the fallback's share there is under that; the per-rung share is measured only at SL2.
**The probe understates the station's cost.** It reads 400 samples at a time, but the daemon reads
whatever buffered since its last tick, and `onset_bound` (lead + first block, up to the 8 192-sample
ring) grows `reach` to about 13 k samples, so roughly 400 onsets instead of 128 — about 3× the cost
above. The probe takes `PROBE_READ` to match a station's read size. Debug builds take ~80 s
per frame through two daemons. On the 2 m stations (Raspberry Pis, several times slower than the
container) the decode plus the ACK's airtime must fit the ISS's 9 s ACK window
(`ota_ack_timeout_ms`, both profiles carry MFSK16). The station measurement is pending.

The order was deliberate. With the coded scan first, an uncoded filexfer fragment cost 116.5 s, because
that scan exhausted every onset instead (`engine.rs`, the comment above the onset scan). **Both scans are
decode-driven and exhaustive, so whichever runs first pays its full cost on the other class of
traffic.** Reordering moves the cost; it does not remove it.

## Decision (proposed, revised after review)

New order in `ota_decode_and_ack_inner` when the fallback mode has a preamble template:

**candidates@0 → ranked fallback (K attempts) → coded onset scan → exhaustive fallback → HARQ / phase 2.**

1. **Ranked fallback.** Build a per-lag ρ profile of the mode's preamble template over
   `[0, scan_end + template_span)`, taking the max over 1–3 residual frequencies around the fixed
   correction. Phase 1 decodes at a single correction, so the full 21-point grid is not needed. Pick
   the top K local maxima with a minimum separation of one symbol, because the period-4 preamble has
   a −2-symbol alias and unseparated peaks are all one lobe. Attempt the uncoded decode at those
   onsets, best first.
2. **Coded onset scan,** unchanged. It finds a coded ladder frame quickly (0.77 s at SL2 in total,
   with the fallback scan removed), so a coded burst pays K fallback attempts plus this scan, and
   never reaches step 3.
3. **Exhaustive fallback,** today's `decode_burst_phase1`, but only after the coded scan has failed.
   A control frame whose onset ranking missed is therefore delayed, not lost: it pays the coded
   scan's exhaustion first (the #1138 note measured up to tens of seconds for a filexfer fragment).
   That cost is now confined to a ranking miss, which the counter below makes visible.

Properties:
- **Ranking, not thresholding.** No ρ threshold decides anything; the per-mode ρ constants (#1062,
  #1060) and the runtime calibration are not touched.
- **No calibration side effects.** `preamble_rho`/the matched filter are pure (`&self`); the
  calibration push, latch and counters live only in `decide_preamble_veto`, which this does not call.
- **Observable.** It is not left as an unobservable path: a counter records the rank of the onset
  that decoded (or "missed"), exposed like the veto counters, and a test asserts it moves on the
  production entry. This is how K is re-checked on air.
- **Modes without a template keep today's order and exhaustive scan,** bit for bit. Only `BPSK250`
  has a template (`MODES_WITH_VETO`), and it is the default `[modem] mode`.
- **K comes from a measurement, not from "tests still pass".** For each on-air corpus capture and for
  `ChannelSimHarness` AWGN and Watterson runs at BPSK250-uncoded's floor SNR, with random leads drawn
  from the real ring, record the rank of the onset within ±16 samples of the decoding onset. K = the
  maximum rank observed, with the histogram published. Expected small (3–4); the existing
  `behind_a_lead` test is a weak control (a digital-silence lead) and does not set it.
- **New code.** `preamble_rho` returns the argmax only, so a per-lag max-over-frequency profile and a
  separated peak picker are needed in `openpulse_dsp::acquisition` (alongside `rho_profile`).

## Risks and how each is checked

- **A control frame whose true onset is not in the top K is lost.** That is a regression in receiving
  station ID, filexfer, handshake, QSY and relay. Checks: the existing reception tests
  (`twin_daemon_bridge` filexfer with OTA on, the station-ID and handshake suites) and the on-air
  corpus replay, plus the rank measurement above. At low SNR a missed preamble peak usually means the
  decode would also fail, but that is UNCHECKED and has to be measured.
- **The correlation costs something.** Over the full 21-point grid it is about 992 taps × 4 100 lags
  × 21 × 2 ≈ 170 M MAC, which on a Pi could eat the saving. Restricting to 1–3 frequencies cuts it to
  roughly 8–25 M MAC. Measured by the probe, before and after, and on the Pi before any saving is
  claimed.
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

## Alternatives considered (from the review)

- `refine_onset` (template-free energy onset), attempting ±2 symbols around it: cheaper, but it has the
  same miss class and less discrimination against band noise in the ring. Kept as a fallback idea.
- A per-(mode, onset) demod cache: at SL5 the fallback and the coded scan demodulate the same slices
  twice (hard, then soft). Orthogonal; a later saving.
- Starting the scan at `last_flush_onset_bound`: wrong, because the frame can start inside the ring.

## Twins

`decode_burst_inner` (the CLI and TNC uncoded and coded entry) runs the same exhaustive phase-1 scan
on its own bursts. It is not on the daemon's per-frame path and is left unchanged here; a follow-up
could apply the same ranking. Found by `grep -n "scan_burst_onsets(" crates/openpulse-modem/src/engine.rs`
(3 call sites).
