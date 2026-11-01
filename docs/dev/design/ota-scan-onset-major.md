---
project: openpulsehf
doc: docs/dev/design/ota-scan-onset-major.md
status: resolved
last_updated: 2026-11-01
---

# Run the OTA burst scan onset-major

Reviewed: `docs/dev/reviews/review-ota-scan-onset-major-2026-11-01.md` (no Release 1 blocker; findings taken as listed there).

Work plan M3, A2 (found on air 2026-10-31, reproduced offline 2026-11-01).

## Problem — measured

A2 needs the receiving daemon to decode a frame and key its ACK inside the sender's 9 s window
(`ota_ack_timeout_ms`). On 2 m it did not, on either Pi:
- **rpi51** decoded station B's first SL2 frame after 267 attempts in 30 s (task 11, Part G);
- **station B** (Pi 4) decoded none of A's frames in 15 min, and saw its later frames only as
  fragments (task 11, Part B).

Task 12 recorded the receive audio beside both daemons
(`docs/dev/test-reports/on-air/rx-audio-2026-11-01/`). Replayed through the daemon's receive entry,
the way `server::run` drives it (`accumulate_capture` in reads, then `ota_decode_burst`), station B's
recording **decodes all four of A's frames**. The audio is fine. **Each decode costs 8–10 s on x86**,
and a Pi is several times slower.

The debug log of the first burst shows where the cost goes, in the OTA arm's scan block
(`ota_decode_and_ack_inner`, the `'scan` loop):
1. The single settle at the burst's trigger point (`acquire_at_onset` at `lead` = 2848) returns
   **−12.79 Hz** and passes all its guards. The carrier, measured from the recording by the 2fc line,
   is at **+3.2 Hz**. −12.8 ≈ 3.2 − 15.6, i.e. half a BPSK31 baud below the carrier. That fits one
   sideband of an alternating preamble, but it is a hypothesis, not checked here.
2. The **corrected pass** at −12.8 Hz walks every onset from 0 to `scan_end` (32 768 samples, 129
   onsets): **all fail.**
3. The **uncorrected pass** then decodes at **onset 2560, on its 10th attempt.**

That is 144 attempts, of which 135 could not succeed.

**Read size matters too; the mechanism is not established.** On the recordings, A's first frame took
1.3 s at 800-sample reads, 9.7 s at 4096, and at 16 000 the bursts merged and frames were lost. An
earlier draft blamed `scan_end` growing with the trigger read (`onset_bound + acq`, #1454/#1443). The
review showed that does not hold here: for BPSK31 the `4·acq` = 32 768 floor dominates until
`onset_bound` exceeds about 24 576 samples, so at 4096-sample reads `scan_end` is unchanged. What makes
larger reads costlier is UNCHECKED. It is parked below.

The same pass-major shape holds across candidates. Once the receiver had recommended SL3, B's 4th
frame paid a full SL3 scan before SL2 decoded: 16.9 s.

## Decision

**Run the scan block's attempts onset-major.** First build the same attempt set as before:

    { (onset, candidate, pass) : every candidate, each of its passes (corrected from 0 if a settle
      passed, uncorrected from `step`), every onset 0, step, …, scan_end }

Then run it sorted by onset. At one onset the order is the candidate's profile order, then the
corrected pass before the uncorrected one, which is the order the passes used to run in. The first
attempt that decodes wins, as before.

Properties:
- **Same attempt set.** A burst no attempt decodes costs exactly what it cost: the same attempts, in
  another order. That includes one corner the first draft dropped (review): when
  `0 < scan_end < step`, the pass-major walk made one attempt at onset `step` itself, past
  `scan_end`. The schedule adds it explicitly.
- **A decoding burst stops at the first onset where any attempt decodes.** A wrong correction or a
  wrong candidate costs one attempt per onset, not a span.
- **The earliest onset wins.** The scan was ascending within a pass and stays ascending overall. A
  burst carrying several frames still yields its first, and `decode_following_frames` takes the rest
  from there.
- **When the outcome can differ:** only if two attempts in the set would both decode. Before, the
  first candidate's or pass's earliest decode won; now the globally earliest onset does. Two
  different (candidate, correction) attempts decoding the same burst means two valid CRC-checked
  frames. That is a multi-frame burst, where earliest-first is the documented intent.

Not changed:
- the attempts before the scan (offset 0 per candidate, the ranked uncoded fallback);
- the deferred exhaustive fallback, phase 2 and HARQ;
- the trigger settle itself, with its wrong estimate on this burst. The reorder removes its cost;
  its accuracy is a separate question, parked.

## Measured (x86, release, the replay)

| recording, read size | first frame before | first frame after | 4th frame before → after |
|---|---|---|---|
| B hearing A (`rx-b-b2`), 800 | 8.13 s | **1.75 s** | 16.9 → **2.7 s** |
| B hearing A, 4096 | 9.64 s | **4.22 s** | 20.1 → 10.8 s |
| A hearing B (`rx-a-g2`), 800 | 1.26 s | 1.70 s | — |
| A hearing B, 4096 | 9.74 s | **4.86 s** | — |

- Every burst that decoded before still decodes, with the same payload length and ACK.
- A's first frame at 800-sample reads got slightly slower, because both passes now alternate there.
- Bursts that do not decode cost the same as before (6–60 s), as designed.
- **Necessary, not shown sufficient, for A2 on a Pi:** at 4096-sample reads the first frame still
  takes 4.2–4.9 s on x86, and a Pi is several times slower. The on-air retest decides.

## Risks and how each is checked

- **A burst that decoded stops decoding:** impossible by construction, since the set is the same. It
  is checked by the replay and by the workspace gate plus `slow-tests.sh` (acquisition path).
- **Multi-frame bursts:** order is still ascending by onset;
  `the_first_of_two_frames_in_one_keying_is_not_lost` and the #1461 suites run in the gate.
- **Cost on a healthy on-frequency burst:** when the trigger settle is in the deadband there is no
  corrected pass, and the order is the old one. When it passes, the alternation adds at most one
  corrected attempt per onset before the decode. Measured on `rx-a-g2` at 800: 1.26 → 1.70 s.
- **The regression test:** `tests/ota_scan_onset_major.rs` replays a cut of the recording
  (`tests/captures/ft991a-ota-frame-bpsk31-rs.wav`). It asserts the decode, and at most 40 decode
  attempts on the burst. Onset-major takes 26; reverted to pass-major, 144 (sabotage run, the test
  fails). It pins a **cost**, not the order: a change before the scan that happened to decode early
  would also pass. `OtaRxResult` does not expose the winning onset, so the onset (2560) is not
  asserted.

## Consumer

`crates/openpulse-daemon/src/nack_budget.rs` (`decode_ota_burst` → `ModemEngine::ota_decode_burst`),
called from `server::run`'s rx tick for every flushed burst while an OTA session is active. Found with
`grep -rln 'ota_decode_burst' crates apps --include=*.rs | grep -v tests`.

## Prior art

- `ota_decode_and_ack_inner`'s own scan block: the passes and the trigger settle are #1118/REQ-PHY-03;
  the slice sizing is #1384.
- `docs/dev/design/fallback-onset-ranking.md`: the same receive-cost item for the uncoded fallback,
  where ordering by likelihood rather than position was the fix. Onset-major is the position-ordered
  analogue for the coded scan.
- The replay follows `capture_replay`'s corpus convention (`tests/captures/README.md`).

Found by reading `crates/openpulse-modem/src/engine.rs` around `'scan:` and
`grep -n 'onset' docs/dev/design/*.md`.

## Twins

The same pass-major or candidate-major shape remains, **not changed here**:
- **OTA phase 2** (`'settle_scan`): per candidate, `acquire_burst_correction` and then a full scan. It
  runs only after the scan block and the exhaustive fallback have failed.
- **`decode_burst_inner`** (the uncoded and ARDOP/KISS burst arm): phase 1 over the span, then phase 2.

Both are candidates for the same reorder. They are parked, because neither is on the A2 path that
failed. `grep -n "scan_burst_onsets\|acquire_burst_correction" crates/openpulse-modem/src/engine.rs`.

## Parked

- **The trigger settle's wrong estimate:** −12.8 against +3.2 Hz on BPSK31, possibly a preamble
  sideband. It settles on a window anchored at the squelch trigger, which is the coarse energy-gate
  window the playbook's §3 warns against. Diagnose it with the swept-correction experiment of §1.
- **Why larger reads cost more on these recordings** (1.3 → 9.7 s at 800 → 4096 samples), given that
  `scan_end` does not change at those sizes.
- ~~A bound on bursts that cannot decode.~~ Done, see the next section.
- **The on-channel carrier at rpi51.** It shows ~1494–1500 Hz in the IC-9700's idle audio, and on
  the RSPdx's port A at 144.6415 MHz.

## Addendum (2026-11-01, after on-air task 13): skip candidates a burst is too short for

### Measured

With the onset-major build on air (task 13), the first frame got through in both directions inside the
window:
- rpi51 decoded B's first frame 1.69 s after it ended;
- station B decoded A's first frame in 2.97 s.

But **every later frame failed live.** In the 900 s run station B decoded 0 of 12. Replayed, the same
recordings decode A's frames 1–3 at about 2 s each. What fills the time between them is the **bursts
that fail**:
- a 5.1 s fragment cost **79 s** on x86;
- a 36 s burst cost 40 s.

On a Pi 4 that is minutes of a blocked receive loop, so the following frames go unheard.

On the 5.1 s fragment, **1088 of about 1600 attempts** were coded attempts, every one failing
`FEC data length N is shorter than one 255-byte block`. The candidates were SL3 (a 33.8 s frame) and
SL2 (66.6 s), and neither fits in 5.1 s.

### Decision

Each candidate's first attempt already decodes the **whole** burst from offset 0. If it fails as
"shorter than one RS block" with at least 2 bytes to spare (`have + 2 < 255`), the onset scan and phase
2 skip that candidate.
- **Why that is safe:** every later onset decodes a suffix of the same burst, so it holds no more
  data. The 2 bytes absorb a symbol of timing-search difference.
- **Not skipped:** the uncoded fallback, because a 0.9 s BPSK250 control frame can sit inside a
  fragment; and HARQ. HARQ **does** run on the `robust` BPSK rungs, since `FecMode::Rs` is admitted
  and BPSK supports soft demodulation: one soft demod per candidate per failed burst, plus a staged
  retention. Its share of the remaining cost is not measured. (An earlier draft said HARQ never ran
  there. The review caught it.)
- **Parsing the error:** it comes from `openpulse_core::fec::rs_short_of_block`, which lives next to
  the message's producer, `FecCodec::decode_prefix`, and shares its prefix constant. A unit test pins
  the two together.

### Measured after (x86, release, replay at 800-sample reads)

| burst | before | after |
|---|---|---|
| 5.1 s fragment (`rx-b-b3`) | 79.2 s | **6.0 s** |
| 36.2 s (`rx-a-g3`) | 39.9 s | **5.2 s** |
| 25.3 s (`rx-a-g3`) | 30.1 s | **4.0 s** |
| 10.8 s (`rx-a-g2`) | 23.8 s | **3.7 s** |

- Every frame that decoded before still decodes, with the same ACKs (all four recordings).
- What the fragments still cost, 2–6 s on x86, is mostly the uncoded fallback.
- Full-length bursts that fail still cost about 30–46 s. One of them is a frame cut off by the end
  of the recording.

### Test

`tests/ota_scan_onset_major.rs::a_burst_shorter_than_a_coded_frame_is_not_onset_scanned` replays
`tests/captures/ft991a-ota-fragment-5s.wav`, 16 s around the 5.1 s fragment. The budget is 600 attempts:
- with the guard: 511;
- with it disabled (sabotage): 888, and the test fails.

The 511 is mostly uncoded-fallback attempts, which the guard does not control. A change to the
fallback could move the count without touching the guard, so the test drifts with unrelated
changes; it is not noisy. There is no separate counter for coded attempts.

Both producers of the "shorter than one block" error build it through
`openpulse_core::fec::rs_short_of_block_error`: `FecCodec::decode_prefix` and the engine's
`rs_interleaved_decode_prefix`. Before, the second one only matched the parser because its text
happened to be the same.

Review: `docs/dev/reviews/review-ota-scan-onset-major-2026-11-01.md` (second round).

### Still parked

- **The uncoded fallback's cost on fragments.** It is exhaustive after the ranked K, and the
  phase-2 entry adds to it.
- **Full-length bursts that fail.**
- **The trigger settle.**
- **The read-size effect.**

