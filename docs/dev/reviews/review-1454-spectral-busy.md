---
project: openpulsehf
doc: docs/dev/reviews/review-1454-spectral-busy.md
status: resolved
last_updated: 2026-09-30
---

# Adversarial review — #1454 stage 2: a spectral busy criterion for weak narrowband frames

Reviews by Fable, read-only except for planted, deleted probes, every one prompted for
falsification: design v3 (round 3, revise), v4 (round 4, revise narrowly), v5 (round 5, revise the
text then build), the failed acceptance run and its diagnosis (round 6, revise), the choice of
statistic (round 7, revise then build), the build (round 8, revise — the gate was red), and the hold
rule's trade (round 9, revise — a fourth option that dissolved the trade). Design drafts, every
review, every probe and log are parked outside the repo (`~/parked/openpulse-1454/`).

## Consumer

- The verdict is formed at the `InputCapture` seam (`engine.rs`, `update_dcd_at_seam`), which every
  capture path passes; one production caller of `NoiseFloorTracker::judge` (`grep -rn "\.judge("`).
- It is read by: the hold start and burst gathering (`accumulate_routed` → the daemon rx tick,
  `server.rs` `accumulate_capture` → the OTA arm, the non-OTA arm, the monitor, the repeater);
  `DcdState` via `force_busy` (CSMA, the repeater's carrier sense, discovery's deferral); the RX
  AGC's unlock (`apply_rx_agc`).
- The ring's lead: `last_flush_lead` (peeked by the daemon's fan-out strip, taken by
  `ota_decode_and_ack_inner` and `decode_burst_with_fec`); the evidence spans: `last_flush_spans`,
  taken by `ota_decode_and_ack_inner`.

Found by: `grep -n "update_dcd_at_seam\|accumulate_capture\|last_flush_\|is_channel_busy"` over the
modem, daemon and repeater crates.

## Prior art

- Stage 1 (#1452): the per-bin floor, the hold/commit/discard lifecycle, the recognition-window
  evidence rule — extended, not replaced.
- #1255's `last_flush_capped` is the pattern the new flush flags follow.
- No overlapped analysis, spectral persistence or pre-trigger ring existed in the tree
  (`grep -rn "overlap\|pre-trigger\|ring" crates/openpulse-dsp crates/openpulse-modem/src/engine.rs`
  before stage 2; my filter). #1443 (a total-power pre-trigger ring) is stage 3 and stays open.

## Twins

- The monitor and the repeater decode with their own engines and cannot know a ring's length: the
  daemon strips it before handing them the burst (maintainer). ARDOP and KISS use their own
  accumulators: lead always 0.
- The repeater's rig_b sensor reaches the rule through the same tracker (D5).
- The evidence rule's two halves (total power, spectral) are twins: each is measured to the end of the
  carrier its detector saw, not to the flush.

## What each round changed

- **Round 3.** ε (a median-relative floor on the ratio) never binds: replaced by a passband mask. The
  M-of-K persistence is right; a false open becomes ladder evidence whenever a fast rung is a
  candidate — hence a minimum spectral span (maintainer: 16 windows).
- **Round 4.** A 2 048-sample ring is falsified by opening latency (worst 7 875): 16 windows. The
  evidence rule is a RUN length, not "any total-power block". The monitor and the repeater cannot know
  the lead (maintainer: strip it).
- **Round 5.** D3 as a per-burst latch; the non-OTA arm gets the ring; the scan widens only when a
  lead exists.
- **Round 6.** The acceptance gate failed (4/8 gathered). The split mechanism is the frame's own
  reversal envelope at a bad window alignment, shown by a PAIRED test (same noise, frame phase forced);
  the cross-placement test of the same prediction had failed because the noise differed. The
  continuous ring (a total-power flicker drained it) with three amendments. The earlier "held whole"
  evidence was one frame at the best alignment.
- **Round 7.** The second analysis phase supplies the other preamble parity, so the phases are judged
  separately and OR'd — a max of their ratios takes the larger of two noise draws and raised the idle
  tail. Hold: an 8-window mean ≥ 2.5.
- **Round 8.** The gate was red at the build: fmt, clippy, two re-homed docs, and three #1452 tests the
  packet had reported failing only under sabotage (it had never run them clean). The spectral evidence
  span measured to the flush, counting the hold's tail: now to the last open. A burst with no
  accumulator provenance was judged spectral: now #1452's rule. A missing straddle cleared both
  histories after every burst (nine blind windows): fixed; a floor-moving commit now clears them. D5
  had no test: four added.
- **Round 9.** `monitor_during_ota` failed on the branch and passed on `main`: the uncapped mean-8
  hold carried a strong frame's burst ~0.55 s and merged a second transmission 0.4 s later
  (reproduced on real idle). The packet posed a two-way trade (keep +6 dB margin, or separate the
  transmissions); the review found the third option in the hold's arithmetic — cap each window at the
  open threshold — which kept every decode count and separated the pair.

## Stated limits (carried into `CLAUDE.md`)

Two transmissions under ~0.3 s apart are one burst; a third-party monitor on an ARQ exchange is that
case and is unmeasured. Sub-window broadband events open S about twice as often as with one phase
(never ladder evidence). The idle open rate is sample-limited (< 0.44 %/window at 95 %). BPSK63's
losses are total-power-opened heads (#1443). Multi-fragment receive through the accumulator has no
test in the tree (filed separately).
