---
project: openpulsehf
doc: docs/dev/reviews/review-1435-timing.md
status: resolved
last_updated: 2026-09-24
---

# Adversarial review — #1435's discriminating test, and the timing defect it found (#1438)

Three rounds by Fable. The first reviewed the measurements and conclusions; the second and third
reviewed the actual text of the issue, the #1435 closing comment, the #1437 correction and the ledger
entry before any of it was posted.

## Consumer

- `plugins/bpsk/src/demodulate.rs` — `find_timing_offset_with_expected`, called by
  `symbol_stream_parts_with_expected` (every BPSK decode and `estimate_snr_db`), by
  `estimate_afc_hz`, and by the #1062 candidate-vetting column.
- `crates/openpulse-modem/src/engine.rs` — `ota_decode_and_ack_inner` feeds the resulting SNR to
  `OtaRateController::on_rx_frame` and to `EngineEvent::OtaRateDecision`, which the panel displays.

## Prior art

#1142 fixed a lock that becomes a noise argmax when a long lead-in pushes the preamble out of the
correlation window, by measuring on the decoded span; its engine comment documents the misalignment
curve. `additive_snr_db_windowed`'s doc already states that a decision-directed estimate "saturates
once symbol errors are common". Neither covers a deterministic early lock with the preamble inside
the window.

## Twins

The GPU path has its own timing search, `timing_offset_search_gpu` (#1083; #1433's shape). The #1062
vetting column calls the same function with a candidate `expected`, so its numbers will move with any
fix. Every BPSK rung shares the geometry.

---

## Prompt

**Round 1 — findings.** Sent the fixed-span decisions measurement, the lead-in sweep, the production
lead sweep with logged spans in both builds, the re-measurement of #1435's fixture, and the claim that
the SNR gates use lead-0 fixtures. Asked whether "true 10 dB" was the right reference, for an
explanation of the unexplained sub-symbol mis-lock from the code, why the scan's first successful onset
sits one symbol early, whether the defect predates the union, whether #1142's fix was incomplete, and
which fix direction is right — without implementing.

**Rounds 2 and 3 — the text.** Sent the four texts and the pin, asking first about provenance
(results stated as mine that were the reviewer's), then scope, arithmetic, and whether the pin could
fail on a fix. Round 3 checked only that round 2's corrections were applied faithfully and that the
rewrite introduced nothing new.

## Verdict

**Round 1.** My mechanism ("an odd whole-symbol lead lands the preamble on a zero-correlation lag")
was wrong in a way that changes the fix. The search's objective peaks before the symbol boundary
because the half-Hann window is not matched to the full-Hann pulse. I independently reproduced that
gain table from the code's window definitions. The unexplained sub-symbol mis-lock was the whole
mechanism. My "true 10 dB" reference was confirmed. The decode runs mis-locked too, so passing the
decode's timing to the estimator is a no-op, and a data-aided estimator would not help. It also asked
for the direct test of span causality, which I then ran: 7 of 8 paired differences were reproduced.

**Round 2.** It found two things that changed sentences everywhere. The lock is measured at −8, not
the model's −9. The production case is a RANGE defect as well as an objective defect: the search
scans only `0..n`, so at lead ≥ n the boundary is never visited. My pin's `assert_ne!(off32, 0)` could
not fail on any fix; it was rewritten to assert the measured lock at a reachable boundary (lead 16) and
past it (lead 32). My #1437 correction claimed the +18 was inflated, when 18/96 is already below 10/48;
the direction is unknown. Seed 22's anomaly was explained from my own log (a phase-2 AFC correction).
The review also caught `hpx500`'s ceilings having been cited as `hpx_hf`'s. The cap was measured
rather than derived before posting.

**Round 3.** Corrections applied faithfully, with no meaning inverted. New issues: "within 0.01 dB" is
0.02 on two seeds; "RS absorbs" an inferred mechanism stated as measured; the pin's values quoted from
the 8-seed sweep rather than the pin's own run; and one misworded exception. All applied.

**The pattern, again:** every round found errors in claims about what the code or the data *is*, not in
the code, and round 2 found them in text written to correct earlier errors.
