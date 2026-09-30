---
project: openpulsehf
doc: docs/dev/reviews/review-ota-first-claim.md
status: resolved
last_updated: 2026-09-30
---

# Adversarial review — a ladder frame keeps first claim on its burst wherever it sits

One design round by Fable (read-only, planted and deleted probes), prompted for falsification. The
packet, the review and the probe logs are parked with #1443's work outside the repo
(`~/parked/openpulse-1443/`: `design-v3-ota-claim.md`, `review-v3.md`, `review-v3-probe.log`).

## Consumer

- `ota_decode_and_ack_inner` (`engine.rs`) — the daemon's OTA arm, reached through
  `ota_decode_burst` from the rx tick (`server.rs`) for every burst while an OTA session is active.

Found by: `grep -n "ota_decode_burst\|fallback_mode" crates/openpulse-modem/src/engine.rs
crates/openpulse-daemon/src/server.rs`.

## Prior art

- #1123 placed the uncoded fallback after the offset-0 candidates "so a ladder frame always keeps
  first claim", and #1138 placed the candidates' onset scan after the fallback so its cost lands only
  on bursts nothing else decodes.
- The phase-2 settle pass in the same function already deduped the fallback against a candidate with
  the same mode and `FecMode::None` — the predicate this change reuses.

## Twins

- Phase 1 (the fallback) and phase 2 (the settle pass) now ask one helper, so they cannot disagree.
- The non-OTA arm (`decode_burst`) has no candidate/fallback split and is unchanged.

## Prompt

The failing gate (`a_ladder_frame_still_classifies_as_ladder_when_the_fallback_could_also_decode_it`
on the #1443 branch), the mechanism read from the code, and a proposed fix (relabel the fallback's
decode as the candidate's). Asked to confirm the mechanism by measurement, to test whether "the same
decoder claims the frame" is the right invariant, what the ladder path needs that the fallback path
lacks, and which shipped profiles are affected.

## Verdict

**Revise the fix; the mechanism is confirmed.** Measured: with the fallback the frame is delivered
without an ACK; with the fallback absent, the candidates' onset scan claims it with an ACK and a
decision. Attempt 0 tolerates only the plugin's timing search (BPSK250: 48 samples yes, 100 no), so
the defect predates #1443 — on `main` most `hpx500` + BPSK250 onsets at ordinary reads were already
claimed by the fallback; #1443 made it deterministic. The proposed relabelling was the wrong shape:
it needs the fallback's decoded span (not returned) or regresses #1142's SNR reading by measuring over
the lead. Build instead: skip the fallback when a candidate is its decoder, using the predicate phase 2
already had, shared. "The same decoder claims the frame" is the only coherent rule, and its cost must
be stated: under such a profile a control frame at the active mode is always ACKed. The census: eight
of eleven shipped profiles have an uncoded rung, which falsified an engine comment and the test
module's doc (both corrected). Land it as its own change, before #1443, with a ledger row saying the
exposure predates it. Gates: a hand-built lead under `hpx500` (ACK) and its `hpx_hf` twin (no ACK);
sabotages: the fallback always (the first fails) and the predicate on mode alone (the second fails).
