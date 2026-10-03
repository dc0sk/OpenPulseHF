---
project: openpulsehf
doc: docs/dev/reviews/review-multi-frame-burst-decode.md
status: resolved
last_updated: 2026-10-02
---

# Adversarial review — multi-frame burst decode (#1461, M2)

One round by Fable, held before implementation (an acquisition-chain change). The reviewer was
read-only and was prompted for falsification. Target: the draft
[`design/multi-frame-burst-decode.md`](../design/multi-frame-burst-decode.md) and the measured twin
result (4 fragments in one keying → one decoded, `Stall` at 121 s; 1-fragment control in 3.6 s).

## Consumer

`server.rs` receive tick: the non-OTA arm (`decode_burst`) and the OTA arm (`ota_decode_burst` → the
#1123 fallback). Found by `grep -n "decode_burst\|ota_decode_burst" crates/openpulse-daemon/src/server.rs`.

## Prior art

`scan_burst_onsets` / `decode_burst_inner` (#1118, #1138), `tx_airtime_seconds` (#1299). Found by
`grep -n "fn scan_burst_onsets\|fn decode_burst_inner\|fn tx_airtime_seconds" crates/openpulse-modem/src/engine.rs`.

## Twins

KISS, ARDOP, the repeater and the monitor call `decode_burst*` and keep the first-frame entry. Found by
`grep -rn "decode_burst\(_with_fec\)\?(" crates --include=*.rs`, excluding tests and `engine.rs`.

## Prompt

Falsify, do not agree, with file:line evidence:
1. Is "decode returns the first frame" the mechanism, or are fragments lost elsewhere (cap flush,
   ring, SAR or the filexfer receiver, the stall timer)? Which experiment would distinguish?
2. Is `tx_airtime_seconds × fs` the exact on-wire length of the frame `transmit` emitted?
3. Is the cursor rule `onset + frame_len − 4·step` safe against both a duplicate decode and a miss?
4. Does decoding frames 2..N disturb the committed AFC correction, or the flush side state?
5. Is `OtaRxResult.more`, filled by the fallback only, sound for the rate controller, HARQ and NACKs?
6. Is "single-frame bursts pay nothing" true, given the real tail?
7. Is there a simpler or more robust alternative (gaps on the sender side)?
8. Which twins share the limitation?

Does this block Release 1? If not, say so.

## Verdict

**Release 1: yes.** Any file over one fragment fails through the daemon, and the RC UI client
(decision 19) depends on file transfer.

| # | Finding | Class | Outcome |
|---|---|---|---|
| 1 | Diagnosis correct. SAR reassembly tolerates order and duplicates, and the 121 s is `block_stall_ms`. The cap flush is close but not the mechanism here | NOTE | Kept. The engine-level N-frames test is the distinguishing experiment |
| 2 | **Dead selective-repeat.** The sender's `BlockAck{complete:false}` arm is unreachable: the daemon receiver only sends `complete: true` and has no fragment-gap timer | FIX (separate defect) | Problem statement corrected; own work-plan row |
| 3 | `frame_len` via `tx_airtime_seconds` is exact on loopback (`tx_airtime_matches_the_emitted_frame`). On CPAL the frames are separated by the flush pad and the stream reopen | NOTE | Risk added; measure at the hardware-loopback stage |
| 4 | Cursor rule safe: the onset is within about one step of truth, so no re-decode and no miss | NOTE | Kept |
| 5 | Phase 2's failure path sets `afc_correction_hz = 0.0`, so running it on the tail would wipe frame 1's correction | FIX | Frames 2..N run phase 1 only |
| 6 | "Single-frame bursts pay nothing" is false: the spectral hold leaves tails up to 4 096 samples | FIX | Floor = shortest possible frame; cost measured in the PR |
| 7 | A keying longer than the receiver cap loses its tail; the test's `burst_max_secs = 600` hid that | FIX | Constraint stated; test runs at the defaults |
| 8 | No alternatives considered (silence between fragments) | FIX | Section added. Not chosen, because the gap depends on the receiver's timing; parked as belt-and-braces |
| 9 | The OTA `more` field is sound. Profiles where the fallback is a candidate stay one-frame | NOTE | Not `hpx_hf`; post-release |
| 10 | Twins: PQ CONREQ is the same failure; the monitor reports the first frame only (cosmetic) | NOTE | Listed |
