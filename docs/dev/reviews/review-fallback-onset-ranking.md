---
project: openpulsehf
doc: docs/dev/reviews/review-fallback-onset-ranking.md
status: resolved
last_updated: 2026-10-02
---

# Adversarial review — fallback onset ranking design (receive cost, M2)

One round by Fable, held before implementation (an acquisition-chain change and a new DSP constant, K).
The reviewer was read-only and was prompted for falsification. Target: the draft
`docs/dev/design/fallback-onset-ranking.md` and the probe `crates/openpulse-modem/tests/receive_cost_scaling.rs`.

## Consumer

`ota_decode_and_ack_inner` → `decode_burst_phase1` → `scan_burst_onsets`, on the daemon's
`ota_decode_burst` path. Found by
`grep -n "decode_burst_phase1\|ota_decode_burst" crates/openpulse-modem/src/engine.rs crates/openpulse-daemon/src/server.rs`.

## Prior art

`PreambleVeto`, `preamble_rho`, `preamble_search_plan`; `openpulse_dsp::acquisition::rho_profile`;
#1118 (fallback is phase-1 only), #1138 (coded scan after the fallback). Found by
`grep -n "fn preamble_rho\|fn rho_profile\|#1118\|#1138" -r crates`.

## Twins

`decode_burst_inner` (CLI and TNC entry) runs the same exhaustive phase-1 scan; left unchanged.
Found by `grep -n "scan_burst_onsets(" crates/openpulse-modem/src/engine.rs` (3 sites).

## Prompt

Falsify, do not agree, with file:line:
1. Is the measured mechanism right, including `fallback_is_a_candidate`?
2. Can the matched filter rank onsets (top-K), and what does that cost?
3. Does skipping `decide_preamble_veto` avoid the calibration side effects, and are there others?
4. Does uncoded control traffic carry the same preamble as the BPSK250 template?
5. What are the failure modes, and is "no exhaustive fallback after ranking" acceptable — or what is
   a better scheme, and how should K be measured?
6. Which simpler alternatives were missed?

Every grep names a positive control. Does this block Release 1? If not, say so.

## Verdict

**Fix design first → all five fixes folded in.** The cost item does not block Release 1 (the Pi
measurement is pending). But draft item 3 would have been a silent Release 1 regression in
receiving handshake, filexfer and station ID.

1. **Mechanism TRUE.** `fallback_is_a_candidate` requires `FecMode::None`, so the fallback runs at
   every coded rung. Two fidelity holes: the "~1 s per frame" figure holds at SL2 only, and the probe's
   400-sample reads understate the station, whose `onset_bound` grows `reach` to ~13 k samples
   (~400 onsets, ~3×). → **Doc corrected; the probe takes `PROBE_READ`.**
2. **Ranking needs new code.** `preamble_rho` returns the argmax; top-K needs a per-lag max-over-frequency
   profile and a peak picker separated by ≥1 symbol, because of the −2-symbol alias of the period-4
   preamble. The cost estimate was ~5× low (≈170 M MAC over the full grid). → **Rank at 1–3 frequencies;
   measure on the Pi before claiming a saving.**
3. **Side effects TRUE** (`preamble_rho` is pure), but this would create an unobservable path. → **Add a
   decoded-onset-rank counter, asserted on the production entry.**
4. **The preamble claim is TRUE** for daemon control traffic (`engine.transmit` at `cfg.modem.mode`, the same
   `preamble_bits` generator as the template). RRC fallbacks have no template and keep the exhaustive path.
5. **Dropping the exhaustive fallback would silently lose control frames.** → **New order:**
   candidates@0 → ranked fallback (K) → coded scan → exhaustive fallback → phase 2. A miss costs
   latency, not loss. → **K is set by a rank measurement** (corpus + AWGN/Watterson at the floor, real
   ring leads), not by "tests still pass"; `behind_a_lead` is a weak control.
6. **Alternatives recorded:** `refine_onset` energy onset; a per-(mode, onset) demod cache; starting at
   `last_flush_onset_bound` is wrong (the frame can start inside the ring).
