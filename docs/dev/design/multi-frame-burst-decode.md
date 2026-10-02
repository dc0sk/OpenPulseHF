---
project: openpulsehf
doc: docs/dev/design/multi-frame-burst-decode.md
status: resolved
last_updated: 2026-10-02
---

# Decode every frame in a burst, not only the first (#1461)

Work plan M2, "data larger than one frame through the daemon".

## Problem — measured

The daemon transmits a planned burst of SAR fragments inside **one keying**, back to back with no gap
(`drain_filexfer_tx`; the handshake's `transmit_handshake_frame` does the same for a PQ CONREQ). The
receiver's accumulator ends a burst only on a read with no carrier, so the whole keying arrives as one
burst. Every decode arm (`decode_burst`, `decode_burst_with_fec`, the `ota_decode_burst` fallback)
returns the **first** frame that validates and drops the rest.

`crates/openpulse-daemon/tests/twin_multi_fragment_file.rs`, run 2026-10-02 through two real daemons:

| file | fragments | result | sender keyings | time |
|---|---|---|---|---|
| 125 B (control) | 1 | received, byte-exact | 2 (offer, data) | 3.6 s |
| 878 B | 4, one keying | **not received**: B decoded one 255 B frame, then emitted `file_failed` `Stall` at 121 s | 2 | 300 s timeout |

No NACK path recovered the lost fragments, and none can today. The sender has a selective-repeat
arm (`BlockAck { complete: false, missing_frag_bitmap }` → resend, `filexfer/sender.rs`), but the
daemon's receiver only ever sends `complete: true` (`daemon/src/filexfer.rs`) and has no fragment-gap
timer. That is a **second defect**, tracked as its own work-plan row: with this design alone, one
faded fragment on HF still fails the transfer after the 120 s block stall. The default `burst_max_secs` = 20 s does not
avoid this: a BPSK250 fragment is about 8.5 s, so the default bursts hold two fragments each (derived
from `plan_bursts`, not run). Any file larger than one fragment fails, which matters for the release
candidate's UI client (decision 19).

## Decision (proposed)

**The receiver continues past each decoded frame.**

1. `scan_burst_onsets` returns the onset that decoded together with the payload. This is internal;
   the public signatures do not change.
2. A new engine entry, `decode_burst_frames_with_fec(mode, fec, burst) -> Vec<Vec<u8>>`, loops:
   - **frame 1:** today's two-phase path (`decode_burst_inner`) with the flush's `onset_bound`;
   - **frames 2..N:** **phase 1 only**, at the correction frame 1 committed. A later frame in the
     same keying is at the same frequency, so acquisition there is pure cost. Phase 2's failure
     path also sets `afc_correction_hz = 0.0` (`decode_burst_inner`) instead of restoring the entry
     value, so running it on the tail would wipe frame 1's correction (review finding 5);
   - on success, advance `cursor` to `onset + frame_len − 4·step`. The back-off covers an onset
     found a few symbols late. A slice starting inside the previous frame cannot decode that frame
     again, because it lacks the preamble and magic;
   - stop at the first failure, or when the remainder is shorter than the **shortest possible
     frame** in that mode and FEC (an empty payload, measured the same way as below). That floor is
     higher than `min_frame_samples`.

   `frame_len` is exact rather than modelled: `tx_airtime_seconds(payload, mode, fec) × fs`, the
   function that runs the real codecs and modulator (#1299). `tx_airtime_matches_the_emitted_frame`
   already asserts it against the emitted samples for every FEC.

   `decode_burst_with_fec` and `decode_burst` keep their signatures and return the first frame. The
   daemon arm switches to the new entry.
3. **OTA arm.** `OtaRxResult` gains `more: Vec<Vec<u8>>`, filled only by the #1123 uncoded fallback.
   Ladder frames are one per keying, because ARQ waits for the ACK. The rate controller, HARQ state
   and ACK are untouched by the extra frames, exactly as for the first fallback frame.
4. **Daemon.** Both arms produce a list. Each frame runs `unpack_received` and
   `process_received_bytes` in order, and then `drain_filexfer_tx` runs once.

**Cost.** A single-frame burst pays one extra phase-1 scan **only when its tail is at least one
shortest frame long**. The total-power detect has no hang, but the spectral detect holds for up to
8 × 512 = 4 096 samples (`S_HOLD_WINDOWS`), so the tail can reach that length. Phase 1 over such a
tail scans short slices. It is measured in the PR (time per single-frame burst, before and after),
not assumed. A multi-frame burst pays one phase-1 scan per extra frame, starting near its onset.

**Sender constraint.** A keying longer than the receiver's burst cap is cap-flushed mid-frame and
loses its tail regardless of this design. For non-OTA BPSK250 the cap is 4 × 74 624 = 298 496 samples
(37.3 s). The default `burst_max_secs` = 20 s keeps every keying under it. The test runs at the
defaults (two 8.6 s fragments per keying) rather than raising `burst_max_secs`.

## Alternatives considered

- **Silence between fragments inside the keying** (review finding 8). Write at least the spectral
  hold plus one read of zeros between `transmit` calls, so each fragment flushes as its own burst.
  It needs no receiver change, also fixes the monitor and the twins, and costs about 6 % airtime.
  - *Not chosen as the fix:* the gap that guarantees a flush depends on the RECEIVER's read size and
    detector hold, which the sender cannot know. It also does nothing for a peer running an older
    build, or for any sender we do not control.
  - The receiver-side loop is the cure either way. A sender gap could be added later as
    belt-and-braces; it is parked in the work plan.
- **One keying per fragment.** It has the same receiver-timing dependence, plus a PTT cycle and the
  rig's key-up per fragment.

## Risks and how each is checked

- **A duplicate decode of the same frame.** It cannot happen by construction (above). The test
  asserts the frame count equals the fragment count, so a duplicate fails it.
- **A trailing scan on a long tail.** When the remainder is longer than `min_frame_samples` but carries
  no frame, one failing scan runs over a 4·acq reach. That reach is small next to the first frame's
  scan (`onset_bound` up to the 8 k ring).
- **The frame length is wrong for a mode** (e.g. padding the modulator adds per call). The cursor would
  then sit after the next frame's start. The 4·step back-off absorbs symbol-level error only. The
  multi-fragment test runs each fallback mode actually used for non-ladder traffic (BPSK250 by
  default) and fails if a fragment is lost.
- **Hardware gaps between frames.** On CPAL each `transmit` opens a stream, and `flush` appends a
  32 ms pad and drains before the next call, so on real audio the frames are not back to back. The
  next frame then starts after `cursor`, which the forward 4·acq reach (4 096 samples at BPSK250)
  absorbs, unless the reopen gap exceeds about 0.5 s. Measure once at the hardware-loopback stage.
- **Acquisition-chain change** (CLAUDE.md): `scripts/slow-tests.sh` before merge.

## Tests

- **Through the production entry:** `twin_multi_fragment_file`, with two real daemons, the
  `accumulate_capture` path, and filexfer of 4 fragments in one keying. It asserts the file arrives
  byte-exact within the budget a 1-fragment file needs times the fragment count, plus a margin. The
  control is the 1-fragment file.
- **Engine:** a burst of N back-to-back frames gives exactly N payloads in order, with the
  first-frame-only entry as the sabotage control.
- **OTA on:** the same file twin with `ota_enabled`, the shape of `a_file_crosses_the_bridge_with_ota_enabled`.

## Consumer

`server.rs` receive tick: the non-OTA arm (`engine.decode_burst(&mode, &burst)`) and the OTA arm
(`ota_decode_burst(..., Some(&mode))` → #1123 fallback). Senders that produce multi-frame keyings:
`drain_filexfer_tx` and `transmit_handshake_frame`. Found by `grep -n "decode_burst\|ota_decode_burst"
crates/openpulse-daemon/src/server.rs` and `grep -n "fn drain_filexfer_tx\|fn transmit_handshake_frame"
crates/openpulse-daemon/src/*.rs`.

## Prior art

- `scan_burst_onsets` / `decode_burst_inner`, the two-phase onset scan (#1118, #1138). It is reused
  unchanged per frame.
- `tx_airtime_seconds` (#1299), the exact frame length.
- #1123, the uncoded fallback this extends.

Found by `grep -n "fn scan_burst_onsets\|fn decode_burst_inner\|fn tx_airtime_seconds" crates/openpulse-modem/src/engine.rs`.

## Twins

Other `decode_burst*` callers that return one frame per burst:
- `openpulse-kiss/src/bridge.rs:215`
- `openpulse-ardop/src/bridge.rs:625`
- `openpulse-repeater/src/lib.rs:263`
- `openpulse-daemon/src/monitor.rs:92`

The ARDOP host path sends one frame per keying today; #1385 (host segmentation) would change that and
should use the new entry. KISS and the repeater are not Release 1. They are left on the first-frame
entry here and listed in the work plan. Found by `grep -rn "decode_burst\(_with_fec\)\?(" crates --include=*.rs`,
excluding tests and `engine.rs`.
