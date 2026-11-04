---
project: openpulsehf
doc: docs/dev/design/dcd-raised-floor.md
status: resolved
last_updated: 2026-11-04
---

# The link under a raised noise floor: the DCD lockout, NACKs, and the sender's step-down

Work plan M3, A2 clause 2. Found on air in task 21, run a2g; reproduced offline on 2026-11-04.

## Problem — measured

**On air (task 21, run a2g, station B receiving):**
- Before B's RF-gain steps, the audio between A's frames was **digital silence**: the 10th percentile
  of window RMS was 0.0000. Something in the FT-991A gated it.
- After the gain was restored (08:24:45) the gaps held **steady hiss at RMS 0.021**, 99.5 % of it in
  300–2700 Hz, about 9 dB under the frames (0.06). The RSPdx showed the air was quiet.
- The **receiver AGC pumps**. Away from the signal (2300–2700 Hz), the hiss is **31 dB lower during a
  frame** than in the gaps. After each frame the total RMS takes **0.58 s** (median, p90 0.70) to
  recover, and the band away from the signal stays more than 5 dB down for about 1 s (the AGC's hang;
  the review's measurement, correcting a first reading of 0.4 s).
- From then on, B's daemon:
  - never saw the carrier drop;
  - flushed a burst only at the **38.3 s length cap**, or as fragments too short to be ladder evidence
    (#1456);
  - keyed **no NACK** (a "busy" channel suppresses NACKs, task 15);
  - moved **no rung**.
- A, whose ladder only follows the receiver, logged 25 `ACK not received` and abandoned 12 messages.

**The mechanism**, from the replay of `rx-b-a2g` through `accumulate_capture` and the tracker's own
test:
1. The squelch was learned on silence (1e-4). The hiss stepped up past it, so every block was
   "carrier" and nothing was learned: a floor learner that learns only below the squelch cannot
   follow a step up.
2. The 38 s cap then committed every window it held (#1452). Frames are 30–33 % of each slab and the
   hang about 7 %, so 39 % of the windows carry the hiss held down. The per-bin 25th percentile picked
   that: a floor of **0.011, half the gap hiss**. The review reproduces it exactly by emulating the
   estimator on the slabs (0.0116–0.0122; the live squelch 0.0147/1.25 = 0.0118).
3. The squelch (1.25 × floor = 0.0138) stayed below the gaps, so the carrier never dropped. Every
   later cap repeated it: a **permanent lockout**.
4. The tracker itself is right on this hiss: fed only the gaps it reads 0.0212 against a true RMS
   of 0.0213. It is what the cap commits that is wrong.

**Why it matters for Release 1:** on HF the receiver hears steady hiss or QRN, and every transceiver
has an AGC. B's earlier runs on 2 m probably all had a gated, silent channel between frames.

## Decision 1 — implemented: a capped burst learns the band from its plateau

`NoiseFloorTracker::commit_band` (`openpulse-dsp`), called at the cap flush instead of `commit`:
- rank the held windows by total power;
- learn the **20 %-wide run of ranks whose powers spread least** (top over bottom), starting at or
  below the median (`BAND_COMMIT_WIDTH` 0.2, `BAND_COMMIT_MAX_START` 0.5). These replace the history:
  a capped burst means the band changed.
- **Why this works:** steady gap hiss is a plateau, its 512-sample windows within about ±10 % of each
  other. The AGC's hang after each frame spreads below it as a ramp. The frames lie above, and the
  start cap keeps a constant-envelope frame's own plateau out of reach.

Short flicker commits keep `commit`.

**The first version (fixed ranks 10–30 %) and why it was replaced.** The review showed that the test
did not model the hang, so the fitted lower edge was untested. With the test given the on-air cadence
(4.2 s frames every 13.8 s, a hang, then a 1 s recovery), fixed ranks held for a 0.6 s hang (7/7)
and failed for 1.0 s (1/7). The plateau rule holds 7/7 and 6/7.

**The limit, measured:**
- **From a 1.5 s hang, no commit rule helps.** The steady-state floor learner (#1452, the 25th
  percentile of every non-carrier window) absorbs the held-down hiss, and the squelch slides under
  the gaps again: 0.0268 after the commit, then 0.0238, 0.0182, 0.0083, 0.0008 frame by frame
  (`where_a_slow_agc_hang_defeats_the_band_commit`).
- HF transceivers' slow AGC decays over 1–3 s.
- **So Release 1 needs the receiving rig's AGC set to FAST (or off)**, as digital-mode setups usually
  are. That is an operating requirement for the HF confirmation, written into task 22.
- A floor learner that only learns the gap plateau is parked (post-M3, with the HF confirmation).

**Further limits (the review, S1 and S2), all in the HF regime:**
- **A signal near the AGC's knee:** the AGC holds the output level, so frames do not rank above the
  gaps by total power. There the commit is `commit`'s input subsampled: no worse, but no fix.
- **SL2's 66 s frames run at ~87 % duty:** the gaps are under a fifth of a capped burst. The SL2 cap
  is also 266–320 s, so one cap there costs minutes, not 38 s.
- **A systematic low bias:** learning from windows below the median total power biases the floor
  6–8 % low, so the squelch after a band commit is about 1.16× the hiss, not 1.25×. The cost is more
  flickers; each keys nothing (#1456).

**The constants, what they were set from, and what would prove them wrong:**
- **Inventory:**
  - `rx-b-a2g`, 127 s after the restore: window RMS 5th / 10th / 20th / 40th percentile
    0.0071 / 0.0183 / 0.0203 / 0.0214, the frames above;
  - the synthetic cadence in `dcd_agc_pumped_hiss`.
- **Falsified by:** gaps under a fifth of a capped burst; a hang over about 1 s; frames that do not
  lift a window's total power over the gaps.

**Measured:**

| Case | `commit` (before) | `commit_band` |
|---|---|---|
| Replay of `rx-b-a2g` across the restore (SL6 locked) | every burst to the cap; 1 frame per 38 s | the squelch steady at 0.024–0.026 after the first cap (gap hiss 0.021, frames 0.06); 12 of 14 bursts decode |
| `dcd_agc_pumped_hiss` (on-air cadence, hiss stepping up to 0.021, 31 dB under frames, 0.6 s hang) | 0 of 7 | 7 of 7 |

- **Sabotage:**
  - `commit` at the cap: 0/7, fails;
  - the run forced to the quietest fifth (`BAND_COMMIT_MAX_START` 0): 0/7, fails.

**What it leaves:** one cap of deafness after a step up in the band: 37 s at SL5/SL6, 266–320 s with
SL2 among the candidates. The cap flush's decode also blocks the loop for 1.3–2.5 s.

## Decision 2 — parked after the review: NACKs while the channel is "busy"

**The premise was wrong.** The review found that the busy mute (`nack_budget.rs`, "a burst failed
while the channel was still busy") never acted in a2g: 0 lines. The NACKs were withheld for two other
reasons:
- the capped slabs, which are not ladder evidence (#1255);
- the 400–3400-sample fragments, under the evidence floor (#1454, #1456).

Decision 1 removes both. The proposal itself would reopen task 15 for weak frames: a frame whose power
dips and whose structure is under the spectral threshold would read "not busy", and the NACK would be
keyed over the rest of it. **Not needed; parked.**

## Decision 3 — not to be implemented as written

**Silence is not rung evidence.** In a2g, B decoded SL6 at 7 dB; what failed was the ACK path. A
sender-side step-down after silent windows would have walked A down while B held {SL6}.

**The review found a lockstep break with no path back:**
1. The silent windows add up per message, so 12 abandoned messages reach SL1.
2. At SL1 a body over 209 B is skipped without transmitting.
3. The receiver hears nothing, and no ACK can ever raise the sender's level: a permanent stall.

The only safe shape is option (b), bounded to the receiver's previous recommendation on an ACK-up,
and even that should require a decode-less reason (a NACK heard), not silence. **Parked.**

## Found by the review: a decoded cap flush keys a late ACK (pre-existing; a keying change) — fixed

- **The mechanism:** every cap flush that decoded a frame keyed an ACK: 13 after the restore in a2g.
  The ACK deadline is set from the slab's end, not the decoded frame's, and #1255 guards only failed
  slabs.
- **Why it matters:** `AckFrame` carries no sequence number, so an ACK for a frame that ended up to
  38 s earlier can land in the sender's window for a later frame and mark that one delivered.
  Whether that happened in a2g is UNCHECKED.
- **Proposal:** a decoded cap flush keys nothing unless the decoded frame ended inside the sender's
  window.
- It changes when the receiver keys, so it waited for the maintainer's go. The review recorded here is
  its design review.
- **Fixed** (`8177910`, then the review's should-fixes): on a capped slab, the frame's end is its slice
  start plus the decoded payload's own coded airtime (`tx_airtime_seconds`). The deadline is shifted
  back by the slab's tail after that end; past it the receiver keys no ACK and the ladder holds, as
  for a late decode. The first version's bound, the slice reserve, left late ACKs through by 5 s at
  SL6 and 83 s at SL2. Review: `docs/dev/reviews/review-late-ack-capped-slab-2026-11-04.md`. Test:
  `no_ack_for_an_early_frame_in_a_capped_slab`.

## Tests

- `dcd_agc_pumped_hiss` (`openpulse-modem`): the gate case (0.6 s hang) and an `#[ignore]`d
  measurement across hangs of 0.6–2.5 s.
- The replay of `rx-b-a2g` is scratch code; the recording is in the repository
  (`docs/dev/test-reports/on-air/rx-audio-2026-11-01/`).
- `no_ack_for_an_early_frame_in_a_capped_slab` (`openpulse-modem`): the late-ACK rule, three cases.
- Decisions 2 and 3: parked.

## Consumer

- `ModemEngine::accumulate_routed`'s cap flush (`crates/openpulse-modem/src/engine.rs`), on every
  capture path that flushes bursts: the daemon's `server::run` receive tick, ARDOP and KISS through
  `CaptureTicker`.
- Found with `grep -n "noise_floor.commit" crates/openpulse-modem/src/engine.rs`.

## Prior art

- **#1452:** the cap flush learns the band. This note changes what it learns.
- **#1454:** the spectral test, decision 2's "structure".
- **#1456:** the evidence floor for short bursts.
- **Task 15:** no NACK while the channel is busy.
- **`docs/dev/research/acquisition-comparison-codec2-ft8.md` §2.** codec2 and FT8 normalise their
  detectors by a band percentile and do not gate on total power.

## Twins

- **The CLI's EnergyGate path** (`decode_burst_inner`, the condemnation chain) does not use this cap
  flush. Whether it locks out the same way is UNCHECKED.
- **The ARDOP and KISS `CaptureTicker` paths** share `accumulate_capture`, so they get the fix. No
  test runs them under pumped hiss: UNCHECKED.
