---
project: openpulsehf
doc: docs/dev/design/dcd-raised-floor.md
status: review
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
  frame** than in the gaps, and it stays down for about **0.4 s after each frame** (the AGC's hang).
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
2. The 38 s cap then committed every window it held (#1452). The per-bin 25th percentile picked the
   **AGC-suppressed hiss from the frame windows** (60 % of the time): a floor of **0.011, half the
   gap hiss**.
3. The squelch (1.25 × floor = 0.0138) stayed below the gaps, so the carrier never dropped. Every
   later cap repeated it: a **permanent lockout**.
4. The tracker itself is right on this hiss: fed only the gaps it reads 0.0212 against a true RMS
   of 0.0213. It is what the cap commits that is wrong.

**Why it matters for Release 1:** on HF the receiver hears steady hiss or QRN, and every transceiver
has an AGC. B's earlier runs on 2 m probably all had a gated, silent channel between frames.

## Decision 1 — implemented: a capped burst learns the band from its middle ranks

`NoiseFloorTracker::commit_band` (`openpulse-dsp`), called at the cap flush instead of `commit`:
- learn the held windows ranked from the **10th to the 30th percentile** by total power
  (`BAND_COMMIT_FROM`, `BAND_COMMIT_TO`);
- these **replace the history**: a capped burst means the band changed;
- a window that holds a frame is never in those ranks, so the AGC-suppressed hiss under the frames
  is never learned;
- the quietest 10 % are the AGC hang and the station's own transmissions, which the replay
  recording contains (the live daemon drops capture while it keys), so they are skipped too.

Short flicker commits keep `commit`.

**The constants, what they were set from, and what would prove them wrong:**
- **Inventory:** one recording, `rx-b-a2g`, 127 s after the restore.
  - window RMS 5th / 10th / 20th / 40th percentile: 0.0071 / 0.0183 / 0.0203 / 0.0214;
  - frames above;
  - AGC hang and own transmit at about 5 %.
- **A first try, the quietest tenth,** learned a squelch of 0.0038 there, below the hiss: it took
  the hang.
- **Falsified by:**
  - a band whose gaps are under a fifth of a capped burst (traffic over ~70 % duty);
  - an AGC hang over a tenth of it;
  - a receiver whose frames do not lift a window's total power over the gaps'.

**Measured:**

| Case | Before | After |
|---|---|---|
| Replay of `rx-b-a2g` across the restore (SL6 locked) | every burst to the cap; 1 frame per 38 s | the squelch goes to 0.0215 after the first cap, then tracks to 0.024 (gap hiss 0.021, frames 0.06); bursts are 4.9 s frames again; 12 of 14 decode with an ACK; the 2 failures are fragments |
| `dcd_agc_pumped_hiss` (synthetic: SL6 frames, a quiet gated channel, then hiss stepping up to 0.021 with the hiss 31 dB down under frames) | 3 capped bursts, 0 frames decoded from the step + 60 s | 1 capped burst, 11 decoded |

- **Sabotage** (`commit` at the cap again): 0 decoded, the test fails.
- `openpulse-dsp` 125 passed.

**What it leaves:** one cap length (38 s) of deafness after a step up in the band, before the floor
re-learns. Shortening that, for example by learning at a shorter, separate "band check" span, is
parked.

## Decision 2 — proposed, for the mandatory review: NACKs while the channel is "busy"

**Today:** after a failed burst, if the channel is still above the squelch (`channel_still_busy`),
the receiver keys no NACK, so as not to key over the other station's frame (task 15).

Under the lockout above, that suppressed **every** NACK. With decision 1 the lockout ends after one
cap, so the suppression lasts at most 38 s. The residual risk is a band whose level the squelch
cannot settle on (QRN bursts, a slowly rising floor).

**Proposal:** "busy" means another station's signal, not total power over a floor. Suppress the NACK
only if the spectral test (#1454) finds structure in the audio after the burst. A NACK withheld
because of total power alone is keyed anyway after one more silent ACK window of the sender's.

**Open for the review:**
- does the spectral test hold on QRN?
- the cost of a wrong NACK (keyed over a frame) against a missing one (a stalled link).

## Decision 3 — proposed, for the mandatory review: the sender after silent ACK windows

**Today:** the ladder is receiver-led. After `MAX_SILENT_ACK_WINDOWS` (2) silent windows the sender
abandons the message, and it never steps down. A link whose receiver hears frames but cannot answer
(a stall, as here, or an ACK path that fails) stays at its top rung for good.

**Proposal:** after K silent windows in a row (K = 2, the abandon point), the sender's next message
goes out at **one rung lower**.
- The receiver already decodes its recommended and its confirmed rung (`rx_candidates`, at most 2).
- A lower rung is decodable there when it is the confirmed one.
- It is not, when the receiver has itself moved on.

**The lockstep question for the review:** can a sender-side step leave the two ladders on different
rungs with no path back? Options:
- (a) the receiver adds the rung below its recommendation to its candidates after a silence;
- (b) the sender steps only to the receiver's last confirmed rung, which is in the ACKs it did
  receive.

(b) needs no change at the receiver.

## Tests

- `dcd_agc_pumped_hiss` (`openpulse-modem`), above.
- The replay of `rx-b-a2g` is scratch code; the recording is in the repository
  (`docs/dev/test-reports/on-air/rx-audio-2026-11-01/`).
- Decisions 2 and 3: none until the review.

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
