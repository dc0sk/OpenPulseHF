---
project: openpulsehf
doc: docs/dev/reviews/review-1443-pretrigger-lead.md
status: resolved
last_updated: 2026-09-30
---

# Adversarial review — #1443 stage 3: a pre-trigger lead for bursts total power opens

Two design rounds by Fable, read-only except for planted, deleted probes, prompted for falsification,
then a review of the write-up. Designs, reviews, probes and logs are parked outside the repo
(`~/parked/openpulse-1443/`).

## Consumer

- The lead is attached at a burst's open block in `accumulate_routed` (`engine.rs`), the single
  accumulator the daemon's rx tick drives through `accumulate_capture` (`server.rs`).
- It is read by the OTA arm (`ota_decode_and_ack_inner`) and the non-OTA arm (`decode_burst_with_fec`,
  via `decode_burst`), each taking `last_flush_lead` and `last_flush_onset_bound`; and stripped by the
  daemon's fan-out before the monitor and the repeater (`last_flush_lead`, peeked).

Found by: `grep -n "rx_burst_lead\|last_flush_lead\|burst_onset_scan_bounds"` over `engine.rs` and
`server.rs`.

## Prior art

- #1454 stage 2 built the ring (`rx_ring`, every block pushed, copied onto a burst the spectral test
  opens, retention and cold-discard rules) and the lead-aware scan bound; this extends both.
- The issue proposed "a one-tick pre-trigger ring"; measured, it is not enough (the flicker chain).
- #1438 PR2 gave BPSK's hard path a −n/2 timing reach, which is why BPSK tolerates a head loss up to
  about half a symbol and no more; QPSK has no negative reach.

## Twins

- ARDOP and KISS run their own accumulators and see no lead (unchanged).
- The monitor and the repeater get the burst with the lead stripped (unchanged).
- The two decode arms take the same two flags, peeked by the fan-out before either takes them.

## Prompt

- **Round 1 — design v1**: the lost-onset probe, a prototype sweep of total-power lead lengths
  (0 / 400 / 1 600 / 8 192), a QPSK500 finding, the proposed rule (the previous read), and the draft
  QPSK issue. Asked to test whether the gain is head coverage and whether the QPSK finding is real.
- **Round 2 — the lead floor**: the built rule failed its read-size invariance gate at 171-sample
  reads; a fixed 2 048-sample floor was proposed with measurements. Asked whether 2 048 is fitted and
  what falsifies it.
- **The write-up** — the ledger entry, the acceptance row, this artifact, the PR body, the commit
  messages and the follow-up issue texts, checked claim by claim against the logs.

## Verdict

- **Round 1: revise, build the mechanism.** The QPSK500 "400-sample structure" was an alias of the
  probe's 25-sample step against the 16-sample symbol; the real variable is the frame's onset carrier
  phase — a shipped coherent-QPSK defect, ~1/7 of on-air frames at any SNR, filed as #1463 with the
  review's text. The lead's gain on BPSK is room before the burst start, not head coverage (the same
  length of idle from elsewhere rescues it); QPSK500 needs the real head. The QPSK500 drop at longer
  leads was the onset-scan reach, `lead + acq`, which also shipped in #1454 for bursts the spectral test
  opens (BPSK250 +8 dB at 4 096-sample reads 9/16 on `main`): the bound became lead + trigger read +
  acq. The probe's instrument went blind at R > 0; the gates were re-specified.
- **Round 2: revise.** The "late opens" are a flicker chain (total power has no hold), bounded by the
  spectral test's arming latency, `S_LOOKBACK × WINDOW` + one read — which is why the measured maxima
  sat near 2 048. A fixed 2 048 was falsified by the first cell outside its inventory (a 2 096-sample
  trigger). The review preferred the whole ring for every burst; the only acceptable shorter form was
  the derived `min(ring, previous read + S_LOOKBACK × WINDOW)`. The review's cost argument weighed
  failed-burst cost and did not price a successful decode's walk from the burst start to the onset; the
  probe-R sweep's per-successful-decode times (already on disk at round 1) show that cost grows with the
  lead — BPSK31 0.18 s at 400, 0.46 s at 1 600, 1.96 s at the whole ring; the derived lead is 2 448 at
  400-sample reads, ~0.6–0.7 s by that sweep — and the maintainer chose the derived lead. Gates added: flicker-chain
  cells with a chain-class positive control; sabotages that pin the lead to the retention rule.
- **The write-up: revise the text.** The gate at the first squash failed clippy on a test helper
  (fixed). Required text corrections, all applied: the 64-sample-read cells were BPSK63 only; S4's
  QPSK500 figure came from 400-sample reads; the cost comparison quoted the 400-sample lead instead of
  the shipped 2 448; the "tightest acquisition window" claim held only for the modes the fixture
  registers; the last-symbol fixture did not place its onset on the trigger read's last symbol at
  4 096-sample reads (the fixture now does, and asserts it); the lost-onset range, one attribution of
  a count to the wrong class, and a citation a GitHub reader could not resolve.

## Stated limits

The lead covers the flicker chains the spectral test ends; not a frame the spectral test never holds,
nor one whose fragment was long enough to clear the ring (filed). Filed separately: #1463 and
#1464 (total power has no hold at the frame head), #1465 (flicker fragments committed to the noise floor), #1466 (the ring's retention keyed on the preamble), #1467 (BPSK250 bursts drop after the preamble at small reads). The fixtures run with the receiver notch off.
