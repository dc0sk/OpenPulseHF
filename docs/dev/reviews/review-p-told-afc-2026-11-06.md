---
project: openpulsehf
doc: docs/dev/reviews/review-p-told-afc-2026-11-06.md
status: resolved
last_updated: 2026-11-06
---

# Review: the preamble detect judges a frame at the receiver's centre (2026-11-06)

Mandatory design review by Fable before implementing, because the fix changes when a NACK is keyed.
Design: `docs/dev/design/ofdm-preamble-detect.md`, Implementation subsection "P follows the
receiver's centre".

## Prompt

On-air run f2i (`docs/dev/onair-tasks.md`, task 29) logged `false_opens=22`. An offline analysis found
that all 22 were genuine B frames (15 at SL9, 7 at SL10) that failed to decode. P's
`burst_coherence` searched around 0 Hz, while the decoder searched around
`center_frequency + afc_correction_hz` (−6.7 Hz on A). So the frames read 0.846–0.900 against the
0.9 evidence threshold, keyed no NACK, and cost B 21 silent ACK windows, 4 abandons and 2 fall-backs.

The reviewer was asked to falsify five things:
- the cause, re-derived in code and re-measured on the recording;
- the choice between four fixes:
  - (A) pass the told through the trait;
  - (B) reuse the decoder's coherence;
  - (C) rebuild P's detectors at the receiver's centre;
  - (D) report the refined coherence;
- the false-NACK risk at the corrected told (the #1178 class: noise, tones, BPSK frames);
- whether 0.9 is the right threshold;
- the gates and sabotages, the twins, and whether it blocks Release 1.

## Verdict

**Diagnosis CONFIRMED and reproduced exactly:**
- A window per detector open reproduces A's 93 on-air opens.
- They give 59 decodes, with the on-air split.
- 22 bursts read < 0.9 at told 0 and ≥ 0.9 at −6.7 (refined 0.903–0.935).
- 11 failed bursts read ≥ 0.9 at both tolds, and were NACKed on air.
- The coherence `acquire` reported was the coarse grid point's. The refined pass's value was computed
  and discarded.

**Fix: (C) and (D), plus a threshold of 0.8.**
- **(C):** one `rx_center()` serves P and every receive site. P rebuilds a unit when the centre
  moves ≥ 1 Hz. No trait change.
- **(D):** `acquire` reports the refined coherence.
- **Threshold:** `P_EVIDENCE_COHERENCE` drops from 0.9 to 0.8, decoupled from the search's
  early-accept. Genuine decodable frames read down to 0.877 refined at the right told; the worst
  non-None negative P could hold is 0.76.
- **(A) rejected:** a major trait change that leaves two tolds.
- **(B) rejected:** the decoder's coherence is only a log field, and it is absent for skipped
  candidates.

**No false-NACK risk added:**
- Wide hiss and tones give `None` at every told.
- BPSK frames read 0.93–0.999 at every told, but they can reach the evidence rule only through a
  confirmed OFDM preamble open (BPSK ρ ≤ 0.42).

**Gates required:**
- an undecodable on-air SL9 frame at AFC −6.7 keys a NACK and is not a false open, and fails today;
- the verdict is the same at AFC 0 and +6.25;
- the false-open control asserts that no NACK is keyed.

**Release 1:** blocks; fixed here.

**Implementation note:** on the on-air window, any one of (C), (D) and 0.8 restores the NACK. So the
sabotages of (C) and (D) are caught by dedicated mechanism tests, not by the NACK gate. The table is in
the design note.

## Consumer

`ModemEngine::accumulate_routed` (the P evidence check and the false-open / held-no-frame counters),
fed by `CarrierDetector::burst_coherence`. Found with
`grep -n "burst_coherence\|P_EVIDENCE_COHERENCE" crates/openpulse-modem/src/engine.rs`.

## Prior art

`docs/dev/design/ofdm-preamble-detect.md` (decision 4, the evidence rule) and
`docs/dev/design/ofdm-carrier-offset.md` (the decoder's told and its AFC).

## Twins

The receive sites that build `center_frequency + afc_correction_hz` now share `rx_center()`; found with
`grep -n "self.center_frequency" crates/openpulse-modem/src/engine.rs`. `decode_mfsk16_k3_ack` builds
at nominal centre; noted, not changed (FSK tolerance).
