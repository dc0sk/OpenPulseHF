---
project: openpulsehf
doc: docs/dev/design/ota-scan-onset-major.md
status: review
last_updated: 2026-11-01
---

# Run the OTA burst scan onset-major

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

That is 144 attempts, of which 135 could not succeed. **Read size multiplies it:** `scan_end` grows to
`onset_bound + acq` with the read that triggered the flush (#1454/#1443). A Pi that falls behind reads
larger chunks and widens the next scan. On the recordings, A's first frame took 1.3 s at 800-sample
reads, 9.7 s at 4096, and at 16 000 the bursts merged and frames were lost.

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
  another order.
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
  fails).

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
  sideband. It is the playbook's §1 territory.
- **A bound on bursts that cannot decode.** The 10.8 s fragment burst cost 258 coded attempts, each
  failing `FEC data length N is shorter than one 255-byte block`, plus 257 exhaustive fallback
  attempts. Once a window truncated by the burst's end holds less than one RS block, every later
  onset holds less, so that candidate's scan could stop.
- **The on-channel carrier at rpi51.** It shows ~1494–1500 Hz in the IC-9700's idle audio, and on
  the RSPdx's port A at 144.6415 MHz.
