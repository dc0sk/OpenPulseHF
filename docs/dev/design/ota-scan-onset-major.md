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


## Addendum 2 (2026-11-01, after on-air task 14): bound the uncoded fallback rescue

### Measured

With the short-of-block guard on air (task 14), station B's coded failures dropped from 813 to 5,
but **B decoded 0 of 4 frames.** Its daemon log shows it spent the run in the **uncoded fallback**:
6822 attempts, stepping one BPSK250 symbol at a time, not counted as `fec attempt`. The sequence:
1. Early fragments of A's first frame each started a fallback rescue, and the loop stalled.
2. The backlog came in as one read, and a 21 s front of the frame flushed short of a block.
3. Its exhaustive fallback scan ran for the rest of the 5-minute run. That scan's reach is
   `max(4·acq, onset_bound + acq)`, and `onset_bound` includes the triggering read.

The fixed-read replay does not reproduce the early fragments. A replay that models the live loop (each
read holds the audio that arrived during the previous decode, CPU 4× slower) does reproduce the
spiral later in the recording: a 7.8 s fragment cost 19.9 s, and two frames merged into a 96.6 s burst.

### Decision

Both uncoded **rescue** stages scan at most `OTA_FALLBACK_RESCUE_ONSETS` = 64 onsets, nearest a point
`OTA_FALLBACK_RESCUE_LEAD_BIAS` = 1024 samples **before** the burst's trigger point (`lead`) first,
within the span they scanned before:
- the deferred exhaustive fallback (`decode_fallback_near_trigger`);
- phase 2's fallback entry.

The ordering is `onsets_nearest`, with ties going to the earlier onset.

Not changed:
- the K = 4 correlation-ranked attempts that run first;
- the ladder candidates' scans and their phase 2, still the full ascending walk;
- a fallback mode without a preamble template, which keeps `decode_burst_phase1` in the ranked
  stage.

**Why the bias:** a frame's head precedes the **trigger point** by up to the pre-trigger ring, 2048
samples. Centred on the trigger, 64 onsets at BPSK250's 32-sample step covered only ±1024 samples, half
the ring (review). Centred 1024 samples earlier, they cover the whole ring. At BPSK31's 256-sample step
they span ±8192 either way.

**What this gives up:**
- at BPSK250, a control frame that ranks below K **and** whose head lies more than 2048 samples before
  the trigger or after it;
- in particular, a frame that opens late inside a long read (#1454's case). Bounding that regime is
  the point: a long read means the loop is already behind.

The ranked stage still covers the best correlation peaks anywhere in reach. `fallback-onset-ranking.md`
found every measured frame at rank 0, but on synthetic frames only. No real on-air frame was checked, so
this rests on an assumption that is still UNCHECKED.

### Measured after (x86, release, replay)

| | before | after |
|---|---|---|
| 5.1 s fragment (`rx-b-b3`) | 6.0 s | **2.5 s** |
| fragments on `rx-a-g3` | 2–6 s | **1.1–3.0 s** |
| `rx-b-b4`, live-loop model, CPU ×4 | 3 of 4 frames; the 4th merged into a 96.6 s burst | **4 of 4 frames decode** |

Every frame that decoded on the recordings still decodes. Full-length bursts that fail still cost
27–39 s; they are phase 2 for the ladder rungs, and the next item.

### Tests

- `a_burst_shorter_than_a_coded_frame_is_not_onset_scanned` now has a budget of 200 attempts. With
  both bounds the fragment takes 133. Sabotage: with the rescue cap unbounded, 511; with the
  short-of-block guard disabled too, 888.
- `onsets_nearest` has a unit test covering the order, the cap and the clamped end.

### Also in this round

- **The daemon logs every capture read** at debug level, with its sample count and the time since the
  last read. A read of a second or more is logged at info, with how many seconds of audio it holds. At
  debug that is about 20 lines a second, roughly 70 000 an hour.
- **The runner waits `TRAFFIC_WARMUP` (30 s) after the daemons start before traffic begins.**

### Still parked

- **Phase 2's cost for the ladder rungs on full-length bursts that fail.**
- **The trigger settle.**
- **Why live capture fragments where the recording does not.** The new read log is meant to show it.

## Addendum 3 (2026-11-01, after on-air task 15): no NACK while the channel is busy; bound the fallback acquisition; log flushes

### Measured

The capture-read log (task 15) showed the receive loop still held for 28–64 s on both Pis.

**Phase 2's fallback-entry acquisition is the multiplier left.** In the live-loop replay of `rx-b-b5`, a
14.6 s burst cost 19 s on x86, about 4700 settle passes. `acquire_burst_correction` settled across the
whole span, which grows with the triggering read. Addendum 2's cap limited only the onsets scanned after
acquisition.

**Station B keyed a NACK over A's frame.** It flushed 56.5 s of A's 66.6 s frame, failed it, and keyed
a NACK at 20:06:56.3, while A transmitted until about 20:07:06.
- It was not the burst cap, which is at least every candidate's frame length. The carrier detect
  dropped mid-frame, after the loop had been held.
- The replay cannot show why, because the recording already contains the gap B's own NACK caused.
- A NACK keyed while the sender is still transmitting is never heard, and it costs B its own receive
  for that time: a needless key-up.

### Decision

1. **(keying)** Right after a burst flushes, and before the decode, the daemon reads what has arrived
   since (`channel_still_busy` in `server.rs`). If the newest 200 ms are above the DCD squelch and the
   noise floor is warm, the channel is **busy**: the sender is still transmitting.
   - A failure on that burst gets the `Muted` verdict: no NACK is keyed, the ladder does not move
     (#1460's rule that a failure moves the ladder only when its NACK is keyed holds), and the NACK
     budget is not spent.
   - A successful decode is still ACKed. A station whose noise floor reads busy must not stop
     acknowledging.
   - The samples the check reads go into `rx_pending`, which the next tick hands to the accumulator
     first, so nothing is lost.
2. **(cost)** Phase 2's fallback entry acquires only across the onsets it will scan
   (`acquire_burst_correction_span`). The ladder candidates' acquisition is unchanged, over
   `[0, scan_end]`.
3. **(diagnosis)** A carrier-drop flush is logged at debug, with the burst length, the read size, the
   DCD energy and the squelch. The next on-air run can then show why a frame was cut.

**What it does not do:** recover the frame. The early flush already lost it. This change prevents the
needless key-up and keeps the ladder honest. Why the carrier detect drops mid-frame after a held loop
stays open, and the new flush log is there to show it.

**Limits of the busy check:** it reads only what has arrived since the flush, often 25–100 ms. A carrier
dip longer than that can still read idle, and the NACK is then keyed as before. The check is a guard,
not a guarantee.

### Tests (run)

- `nack_budget::tests::a_failure_while_the_channel_is_busy_keys_nothing_and_spends_no_budget`: on a
  failing burst, `channel_busy = true` keys nothing, and the three budgeted NACKs are all still
  available afterwards. Sabotage, with `channel_busy` ignored: it fails, "a NACK keyed while the
  channel is busy goes out over the sender's frame".
- `the_fallback_rescue_acquisition_does_not_grow_with_the_read`: the fragment fixture fed in 2 s
  reads takes 17 settles on its burst (budget 60). Sabotage, with the fallback entry acquiring across
  the full span: 206.
- The replay decodes the same frames on all four earlier recordings (3, 2, 4, 2).

### Not tested

The server's busy check itself, reading the stream after a flush. It runs only with a capture stream,
and the next on-air run exercises it; its debug line "channel still busy after a burst flushed" shows
when it fires.

### After review (round 4)

`rx_pending` is cleared wherever the receive stream is dropped (before every transmit and on a read
error). Without it, audio read before our ACK could be prepended to audio captured after it, open a
burst, fail, and key a NACK right after the ACK. Not tested: like the busy check, it needs a capture
stream.
