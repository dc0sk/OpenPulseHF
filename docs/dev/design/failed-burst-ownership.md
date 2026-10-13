---
project: openpulsehf
doc: docs/dev/design/failed-burst-ownership.md
status: draft
last_updated: 2026-10-13
---

# A narrowband foreign over is not ladder evidence (#1460)

Work plan M2, *Next, no maintainer input needed*, item 2. It follows
[`reply-window-evidence.md`](reply-window-evidence.md) v3, which left this case open (*What it does not
do*, first bullet): a foreign over longer than the 0.5 s evidence floor still counts as a failed
frame, keys a NACK and moves the ladder down. Decision 22 keeps #1460 open until this lands. **It
changes when a NACK is keyed, so it is reviewed before it is built (decision 8).**

> **Round 1 (2026-10-11): not implemented as written**
> ([`reviews/review-failed-burst-ownership.md`](../reviews/review-failed-burst-ownership.md)). Four
> blocking findings:
> - the false-match rate is minimised over 10³ wires per burst, so D = 16 refuses almost nothing;
> - a steady tone at baud/2 descrambles to 17 bits from the header;
> - SL1 loses every acquisition failure;
> - a refused frame of ours turns a NACK into ISS abandonment, on 2 m as well.
>
> **Not a Release 1 blocker.** The maintainer kept #1460 in M2 (decision 21) and chose a round-2
> design. **Round 4 (2026-10-13) is a different approach, for review: see *Round 4*.** It caps
> the ladder damage of any run of failures instead of classifying the burst.
>
> **Round 2, also not implemented** (round 2 review: the shipped half-Hann pulse puts
> ~0.6 of our own frame's power in 4 bins, so C overlaps CW; floor subtraction is not implementable as
> written). Round 3 starts from a measurement of the notch bank's in-band interferer state. Round 1's header rule is summarised at the end, under *Round 1*.

## Problem

During an OTA session, a burst that fails every candidate decode is ladder evidence once it clears the
#1452 recognition window, the #1456 evidence floor (0.5 s) and the ACK check. Evidence means
`on_rx_frame(Failed)` runs and the daemon keys a NACK, within its leaky budget of 3. Three in a row
are one demotion step.

Since #1454's spectral detect, a burst can be a foreign narrowband over in the passband: CW, a
carrier, RTTY, PSK31. Each over longer than 0.5 s is one failed frame. A busy HF band therefore
demotes a working session and keys NACKs into other stations' QSOs. That is the issue's "why now",
and it is what this design addresses.

## The asymmetry that decides the shape (round 1, finding 4)

Getting this wrong costs very different amounts in the two directions:
- **A foreign over that counts** costs one NACK and a third of a demotion step. The budget caps it
  at 3 + 6 per hour.
- **A frame of ours that is refused** gives the ISS silence. Two silent windows abandon the message
  (`MAX_SILENT_ACK_WINDOWS` = 2, `server.rs:1948`), with no step-down. That happens on 2 m as well
  as HF.

So the rule must not depend on finding evidence that a burst is ours, because a failing frame of ours
is exactly the burst that hides that evidence. Fade-split pieces, a timing lock one symbol off, and an
off-frequency first frame all lose their header (round 1, finding 4). **A burst is refused only on
positive evidence that it is foreign.** Where there is no such evidence, today's rules decide.

## Decision (proposed)

**Spectral concentration.** For a failed burst that today's rules count, the engine computes **C**:
the fraction of the in-band power held by the strongest **4** frequency bins of a Welch power
spectrum.
- **The spectrum** is taken over the post-lead samples, with Hann windows and 50 % overlap, and bins
  **baud / 8** wide at the strongest candidate's baud.
- **"In-band"** is fc ± (50 Hz + 1.5 × baud): the AFC range (REQ-PHY-03) plus the rung's main lobe.
- **The noise floor** is first subtracted per bin. The floor is the learned idle floor the carrier
  detect already keeps (#1452).

A tone, a CW carrier or an RTTY pair puts nearly all its power in 1–2 lines, so C is near 1. A frame
of ours spreads its power over a main lobe 2 × baud wide, which is 16 bins, so C is well below 1.
Noise, fading and a carrier offset only spread a frame of ours further, so **a failing frame of ours
cannot be pushed towards the foreign side**. The one thing that narrows it is a receive filter
narrower than the rung, which M1 measures.

**The rule:**
- A failed burst at a single-carrier RS rung, SL2–SL6, is **not** ladder evidence if C ≥ **θ_r**, the
  per-rung threshold set by M1.
- **SL1 (MFSK16) is unchanged** (round 1, finding 3). Its symbols are single tones, so a short piece
  can be concentrated. It may join later, but only after its own M1.
- **SL7+ (OFDM) is unchanged.** It is wideband, and the spectral detect does not gather it.
- **With two candidates,** the burst is refused only if every candidate is in scope and refuses it.

This rule needs no demodulation and no onset, and it gives one number per burst. Round 1's findings
1–3 do not arise:
- there is no minimum over 10³ wires;
- a steady tone is the case it catches by construction;
- SL1 is out of scope.

**What it does not refuse:** a foreign signal as wide as ours, such as another station's PSK31 at
SL2, an OpenPulseHF frame, or a wideband data mode. Those still count. That is stated, not solved.

**The template (round 1, finding 5)** is not used, because a low ρ is absence evidence, which is the
round-1 trap again. A **high** ρ could later *override* a refusal, but no refused burst of ours has
been measured to need one.

**HARQ:** retention follows the evidence decision, as today (#1502).

## Measurements, pre-registered (rules fixed here, before any number exists)

Every measurement runs through the production entry: `accumulate_capture`, the flush, then
`ota_decode_burst`, under the `fast` profile, locked at each rung SL2–SL6. Every burst today's rules
count has its C recorded.

### M1, our frames, failing

The cells are crossed (every channel with every filter and offset):
- **Channels:** `moderate_f1` at the rung floor − 2, − 4 and − 6 dB, and AWGN 3 dB below the cliff.
- **Filters:** wide (300–2 700 Hz), and **behind 500 Hz and 250 Hz** brick-wall receive filters
  (`common::filter::band_limit`).
- **Carrier offset:** 0 and +50 Hz.
- **Sample:** 100 seeds per cell, payload 16–200 B `Rs`, in both idle recordings.
- **Also:** the fade-split pieces of `marginal_frame_pieces`. In those, BPSK31 at the floor shreds
  into short counted pieces.

Two quantities are reported per rung:
- C_max, the largest C of any counted burst;
- the per-frame count of frames that key at least one NACK today but none under the rule, at the θ_r
  below.

**Rule:**
- θ_r = min(C_max + **0.15**, 0.95).
- **M1 passes only if not one** failing frame of ours, in any cell, loses its last NACK at θ_r.
- **If C_max > 0.80 at a rung,** that rung leaves the rule's scope, because a 250 Hz filter
  concentrates its own frames too far.

### M2, foreign overs, at θ_r

**Signals:**
- a steady carrier, swept across the in-band span in steps of baud/8;
- CW at 12, 20 and 30 wpm;
- RTTY 45.45 Bd / 170 Hz, with one tone placed at fc ± baud/2;
- a carrier pair at fc ± baud/2;
- real BPSK31 varicode text.

**Conditions:** each at +6, +12 and +20 dB over the idle, both idle recordings, until **at least 300
counted bursts per cell**.

**Reported:** per signal and rung, the refused fraction with its 95 % lower bound.

**The claim:** a signal class is claimed as refused at a rung only if that lower bound is ≥ 0.99. The
design **passes** if the steady carrier and CW are claimed at SL3–SL6. Every other cell is reported
as measured and is neither averaged nor claimed.

### M3, cost

`receive_cost_scaling` re-measures decode per frame on x86. **Rule:** no rung slower by more than
2 %. One Welch PSD per failed burst is the only new work.

## What it does not do (stated)

- Wideband foreign signals, foreign signals as wide as the rung, and other OpenPulseHF stations still
  count.
- SL1 and SL7+ are unchanged.
- A refused foreign over still takes the receiver's time while it is decoded. This changes only what
  happens after the decode fails.

## Tests (to write with the implementation)

- `a_narrowband_foreign_over_is_not_evidence`: CW and a steady carrier at +12 dB, at SL3 and SL5,
  through `accumulate_capture`. Neither keys an ACK frame, and the rung holds. Sabotage: θ_r = 1.0,
  and both count.
- `a_failing_frame_of_ours_behind_a_250hz_filter_is_still_evidence`: SL5 behind 250 Hz at the floor
  − 4 dB fails and is still answered. This is the case closest to θ.
- The existing `idle_flicker_is_not_evidence`, `cap_flush_is_not_ladder_evidence` and
  `marginal_frame_pieces` stay green.

## Consumer

The not-evidence guards in `ota_decode_and_ack_inner` → `on_rx_frame(RxOutcome::Failed)` → the daemon
OTA arm's NACK keying (`on_ladder_burst`); the HARQ retention gate (#1502). Found by
`grep -n "not ladder evidence" crates/openpulse-modem/src/engine.rs`.

## Prior art

- `reply-window-evidence.md` v3 and its review rounds.
- Round 1 of this design and its review.
- The #1454 spectral busy test, which already judges per-band power against a learned per-bin floor
  (`openpulse-dsp::noise_floor`).
- The ACK check `holds_an_ack_codeword`, which has the same shape: recognise a class of non-evidence
  in a failed burst.

Found by `grep -n "fn holds_an_ack_codeword" crates/openpulse-modem/src/engine.rs` and
`grep -rn "pub const WINDOW" crates/openpulse-dsp/src/noise_floor.rs`.

## Twins

- **ARDOP** (`receive_with_ack_hint`) keys its NACK on any failed one-shot receive and does not use
  the OTA evidence rules. It is unchanged by this.
- **linksim** drives `on_rx_frame` directly, without bursts.

linksim was found by `grep -rn "RxOutcome::Failed" crates apps --include=*.rs`. ARDOP was found by
`grep -rn "receive_with_ack_hint" crates/openpulse-ardop/src`, because that grep cannot see it.

## Round 3: measure before any rule (pre-registered 2026-10-11, before any output)

The round-2 review pointed at the notch bank's own classifier. `NotchBank::band_filled` calls a
block ours when more than 40 % of the protected band's bins sit 6 dB above the local median.
`peaks_from_spectrum` finds peaks of ≥ 14 dB local prominence. Both run on 4 096-point Hann spectra
(1.95 Hz bins). Both are scale-free, so they need no floor subtraction, no AGC correction and no
resampling.

**The statistic.** T is the fraction of a burst's analysis blocks that are **tone-dominated**. A
block is tone-dominated when the band is *not* filled and an in-band peak of ≥ 14 dB prominence is
present.
- **Blocks:** 4 096 samples, hop 512, over the flushed burst's post-lead samples. The flushed burst
  comes from `accumulate_capture`, so its boundaries are production's.
- **Fidelity:** the classifier is a public method on `NotchBank`, so the probe and any later rule
  call the same code (verification rule 5).

**Hypothesis.** Our failing frames have few tone-dominated blocks: only the `--++` preamble lines
(BPSK31) and the constant-phase tail. A CW, carrier or RTTY over has mostly tone-dominated blocks.

**Cells.**
- **Ours:** SL2–SL6, failing:
  - `moderate_f1` at the floor − 2 and − 6 dB, at 0 and +50 Hz, wide and behind 250 Hz;
  - the deterministic head piece (preamble only, 1 s) and tail piece (last 0.5 s of data plus the
    tail);
  - our frame plus an in-band carrier at −6, 0 and +6 dB relative to the frame.
- **Foreign:** a steady carrier swept over the band, CW at 20 and 30 wpm (word-gap split included),
  RTTY 170 Hz, and a carrier pair at fc ± baud/2, each at +6 and +12 dB.
- **Sample:** 50 seeds per cell. All five idle recordings in `tests/captures/` are used, rotated by
  seed.

**Reported.** Per rung, the distribution of T for ours (min, p50, p95, max) and for each foreign
class.

**What decides round 3:**
- **Go:** at SL3–SL6, the largest T of any failing frame of ours that is *not* under an in-band
  carrier lies at least **0.3 below** the smallest T of the CW and carrier classes.
- **Ours plus a carrier** is reported separately. Refusing those costs our NACK, so if they overlap
  the foreign classes, round 3 must state that cost as a decision, not hide it in a threshold.
- **No go:** without separation, #1460's remainder goes back to the maintainer with the three rounds'
  evidence.

### Round 3 result: NO GO

The probe is `burst_tone_dominance_probe` (release, `--ignored`): SL5 at 5 seeds, then all rungs at
3 seeds, all five idle recordings. Cells are as registered except the 250 Hz filter on our frames,
which `ic9700-idle-250hz.wav` covers within the rotation. A second statistic was added after the
first SL5 run showed why T fails: **P** is the share of blocks with any in-span peak of ≥ 14 dB
prominence (`NotchBank::has_prominent_peak`, no band-filled gate).

| | our bursts (all cells) | ours + carrier −6 dB | carrier sweep +6 dB | CW 20 wpm +6 dB | RTTY +6 dB |
|---|---|---|---|---|---|
| T, every rung | 0.000 max | ≤ 0.061 | **min 0.000** | **min 0.000** | **min 0.000** |
| P, SL2 | **max 1.000** | 0–1.000 | min 0.983 | min 0.000 | min 0.160 |
| P, SL3–SL5 | **max 1.000** | min 0.708 | min 0.984 | min 0.905 | min 0.861 |
| P, SL6 | max 0.889 | min 0.984 | min 1.000 | min 0.914 | min 1.000 |

- **Why T fails.** `band_filled` takes its noise reference as the median over the whole 200–3 600 Hz
  guard. Behind the 250 Hz and 500 Hz rig filters that median is stopband, so the filter's own
  passband noise "fills" our band, and a foreign carrier there reads T = 0 (traced per seed: 0 on
  both filtered recordings, 0.37–0.98 on the wide ones).
- **Why P fails.** Our own bursts carry prominent peaks: the recordings' own birdies, the
  constant-phase tail, and the +50 Hz cells. An in-band carrier 6 dB under our frame already reads
  0.7–1.0.

The overlaps are structural, so more seeds can only widen the ranges. **By the pre-registered rule,
#1460's remainder goes back to the maintainer with three rounds' evidence.**

## Round 4 (2026-10-13): stop classifying, cap the damage

The maintainer chose this after round 3's NO GO. Three rounds found no statistic that tells a foreign
over from a failing frame of ours, so round 4 does not try. It bounds what **any** run of failures
without a decode can do to the ladder, whatever caused them. It changes when the ladder steps down
and so what a keyed NACK carries, so it is reviewed before it is built (decision 8).

### What a run of failures does today

Read from the code, not measured:
- **Every counted failed burst** runs `on_rx_frame(Failed)` inside `ota_decode_burst`, before the
  daemon's budget decides whether to key (`server.rs`, `ota_nack_budget.on_ladder_burst`). Every
  `nack_threshold` (3) of them is one `NackStepDown`, which moves **both** `rx_confirmed` and
  `rx_recommended` down a rung.
- **The budget keys 3 NACKs** without a decode, then one per 10 min of listening (#1456). So
  failures 1–3 are keyed and NACK 3 announces the first step; failures 4–6 are muted, and the second
  step at failure 6 is **silent**.
- **A silent step desyncs the receiver.** `rx_candidates` holds only `rx_recommended` and
  `rx_confirmed`, both now two rungs down, while the ISS sends at the rung NACK 3 announced. Its
  frames are no longer tried, so they fail too, and each third one walks the ladder down again.
- **One minute of CW** is 3–5 demotion steps, because word gaps split one over into one counted
  burst per word (round 2 review, finding 4). One leaked NACK then announces all of them at once.
- **Between sessions** (#1456), idle flicker that clears the 0.5 s floor does the same, and the
  peer's next frame arrives at a rung no longer tried.

### Decision (proposed)

Two changes in `OtaRateController`, both in the `NackStepDown` branch:
1. **At most one step down between decodes** (`MAX_STEPS_WITHOUT_DECODE` = 1). After it, further
   failures answer `NackHold` at the stepped rung until a decode. The value is not fitted: it is
   `OTA_NACK_BUDGET / nack_threshold` = 3 / 3, the most steps the budget can ever announce before it
   mutes. A deeper step could only arrive silently or through a leaked NACK 10 min later, after the
   ISS has abandoned (two silent windows).
2. **The step moves the recommendation, not the anchor.** `rx_confirmed` stays at the last decoded
   rung until a decode, as `FastDownshift` already does ("a lost downshift ACK can't desync the
   receiver"). So after the step, both the rung the ISS was on and the rung the NACK asked for are
   tried, whether or not the ISS heard the NACK.

**What it does to a real fade.** A link that needs two rungs between decodes fails today as well:
the second step is silent and the ISS abandons after two silent windows. With the cap it abandons
the same way, one rung higher, with candidates that still match what it sends. A link that needs one
rung is unchanged, except that the old rung stays a candidate until a decode.

**What it does to a foreign over.** One minute of CW costs at most one step and at most 3 NACKs
plus the leak, against several steps today. The NACKs keyed into another QSO are unchanged: the
budget already bounds them. **This does not close #1460**: a foreign over still counts as evidence,
still keys up to 3 NACKs and still costs one step. It makes the cost bounded and recoverable.

**Cost.** After a step, two candidates are decoded per burst instead of one until a decode. That is
already the normal state after a climb recommendation, so the receive cost is inside today's
envelope. UNCHECKED on the Pi at SL2 (the slowest decode); M3 measures it.

### Measurements, pre-registered (rules fixed here, before any number exists)

- **M1, linksim goodput.** The linksim models no NACK budget: its ISS hears every NACK, so it can
  descend several rungs between decodes, which the daemon cannot. Run the linksim goodput gate and
  the `ota_channel_adaptation` slow suite (CAP-33) before and after. **Rule:** the slow suite passes.
  A goodput cell that drops by more than its gate tolerance is a cell that relied on two or more
  steps between decodes; each is listed. If any gate row fails, the cap is made a controller option
  that the daemon sets and linksim leaves off, and a budget model for linksim is parked as its own
  item.
- **M2, the #1456 falsifier** (from `review-nack-streak-decay.md`): a recorded 250 Hz idle at SL2
  through `accumulate_capture` for 90 min, then a peer's SL2 frame and then a failing BPSK31 frame.
  **Rule:** at most one step; the SL2 frame decodes (it is still a candidate); the failing frame
  keys a NACK. Today's expected result, recorded for comparison: more than one step and the SL2 frame
  fails.
- **M3, CW at SL4:** one minute of 20 wpm CW, word gaps included, at +6 dB through
  `accumulate_capture`. **Rule:** at most one step and at most 3 NACKs, and the next frame of ours at
  the original rung decodes. Plus the two-candidate decode time at SL2 on x86 (Pi pending).

### Tests (to write with the implementation)

- Unit (`ota_rate.rs`): six failures without a decode give one step, and both rungs are candidates;
  a decode resets the cap; a decode at the stepped rung moves the anchor; a lost NACK (the ISS stays
  on the old rung) still decodes. Each fails with its half of the change reverted.
- Daemon: M2 and M3 as integration tests through `accumulate_capture`.
- Existing: `abstention_still_steps_down_at_the_nack_threshold`, the NACK-hysteresis tests and the
  budget tests stay green; any that pinned two steps without a decode is rewritten with the reason.

### Consumer, prior art, twins (round 4)

- **Consumer:** `OtaRateController::on_rx_frame` and `rx_candidates`, called by
  `ota_decode_and_ack_inner` (engine) on every counted burst; the daemon keys what it returns. Found
  by `grep -rn "on_rx_frame\|rx_candidates()" crates apps --include=*.rs`.
- **Prior art:** `nack-streak-decay.md` (parked: decay by spacing, and the falsifier M2 reuses);
  `nack_budget.rs` (#1456); `FastDownshift`'s keep-the-anchor rule in `ota_rate.rs`.
- **Twins:** linksim's ISS loop (`apps/openpulse-linksim/src/lib.rs`, calls `on_rx_frame` with no
  budget, M1); the ARDOP TNC steps down on a missing ACK on its own and does not use this controller
  (UNCHECKED beyond `grep -rln OtaRateController`, which lists core, modem, daemon and linksim).

**Review asked for:** is the desync story right (do muted failures really step silently, and is the
budget's count the same set as `on_rx_frame`'s)? Is a two-rung fade really lost today? Does keeping
the anchor break the climb or the lockstep invariant? Is one step the right cap? Does this block
Release 1?

## Round 1 (2026-10-11, not implemented)

Round 1 counted a failed RS burst only if one of its tried wires came within D bits of the 56 known
header bits. The review found four blocking flaws:
- the false-match rate is minimised over 10³ wires;
- a steady tone at baud/2 descrambles to 17 bits from the header;
- SL1 fails before any wire exists;
- refusing our own failing frame turns a NACK into ISS abandonment.

Round 2 keeps round 1's diagnosis of the wire and drops its direction.
