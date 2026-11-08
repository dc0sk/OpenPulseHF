---
project: openpulsehf
doc: docs/dev/design/union-ack-recognition.md
status: draft
last_updated: 2026-11-08
---

# A heard sub-floor ACK is not ladder evidence: recognise the K=3 union ACK

Work plan M3, A2 (stability), and the HF confirmation. This changes when a station keys a NACK, so it
has the mandatory review **before** it is built (CLAUDE.md, "Anything that keys a transmitter").
Review: pending.

## Problem — seen on air

On-air task 32, after f2l's traffic ended, both daemons stayed up for about 3 h and keyed a NACK
ping-pong between them. The task 32 review is in `docs/dev/onair-tasks.md` on `m3/onair`, "Review of
task 32"; the evidence is the f2l directory's `events-{a,b}-full-pre-f2m.ndjson.gz`.

- **How it started.** B's receiver opened a burst on its own floor (DCD energy 0.0182 and 0.0188;
  B's idle busy p10–p50 is 0.018–0.023). B NACKed it, a `leaked` verdict from a budget the run had
  banked.
- **How it went on.** From then on, each station's raised DCD follows the other's keying, row for row.
  - A keyed 9 times: 5 NACKs and 4 sign-off IDs.
  - B keyed 7 times: 4 NACKs and 3 IDs.
  - B logged 40 more holds, muted by the budget.
- **At about 16:22, A's first decision was `nack_step_down` SL9 → SL8, keyed.** That is a ladder move,
  made on a burst A could not decode, by a station with no traffic.

#1456's rule 2 (`reply-window-evidence.md`) exists for exactly this case: "without this rule two idle
OTA stations NACK each other". It did not hold, because of what B transmits:
- B's ladder was at SL1 (`nack_step_down SL1→SL1`). Its NACK recommends the MFSK16 sub-floor rung.
- So `transmit_ota_ack` keys the **K=3 union ACK** (`transmit_ack_mfsk16_k3`): one FSK4 ACK, then
  three MFSK16-ACK copies, each after 0.5 s of silence.

The ping-pong is bounded by the budget (3 NACKs plus 6 per hour), so it stays inside #1456's design
limit. What #1456 meant to exclude is **the ladder move on a heard ACK**.

## The geometry — measured

A temporary unit probe in `openpulse-modem` measured these through the shipped modulators. It built
the union ACK exactly as `transmit_ack_mfsk16_k3` does, and was deleted after the run. Release build,
x86.

| item | samples (8 kHz) | seconds |
|---|---|---|
| `fsk4_ack_frame_len()` (FSK4-ACK, 13 ShortFEC bytes) | 4 160 | 0.52 |
| one MFSK16-ACK copy | 10 240 | 1.28 |
| gap (`MFSK16_ACK_GAP_S`) | 4 000 | 0.50 |
| whole union ACK (FSK4 + 3 × (gap + MFSK16-ACK)) | **46 880** | **5.86** |
| today's guard: `post <= 2 × fsk4_ack_frame_len()` | 8 320 | 1.04 |

What `holds_an_ack_codeword` returns:
- the whole union buffer: **true** (it finds the leading FSK4 copy), in 83 µs;
- an FSK4 ACK alone: true;
- **one MFSK16-ACK copy alone: false**, because the guard looks only for an FSK4 codeword;
- 46 880 samples of noise: false, in **70 ms**. This is the worst case: every offset is tried.

One MFSK16-ACK copy demodulated with `stage_demodulate_payload("MFSK16-ACK")` gives 13 bytes, and
ShortFEC decodes them to 5. The same on a noise window fails.

So **two holes**, not one:
1. **Length.** A union ACK heard as one burst is 5.86 s, past the 1.04 s bound. It is never scanned,
   even though the scan would find its FSK4 copy.
2. **Pieces.** The 0.5 s gaps can end the burst. The squelch drops on silence, and since #1024
   `CaptureGap` ends a burst on a real carrier drop. Each MFSK16 copy then arrives as its own 1.28 s
   burst. That is under 2 × 1.04 s once the lead is counted, past the 0.5 s evidence floor, and it
   carries **no FSK4 codeword**. Each piece is a failed burst, and it counts.

Which of the two happened at 16:22 is **UNCHECKED**: there is no receive audio after 13:50 that day.
Both have to be closed.

## Design

At the one check, in `ota_decode_burst` (`engine.rs:5212–5220`), the shared seam that every OTA
failed burst passes through:

1. **The length bound becomes the longest ACK waveform the engine can key**, plus one FSK4 frame of
   slack for the lead. It is computed from the same constants the transmit uses
   (`MFSK16_ACK_COPIES`, `MFSK16_ACK_GAP_S`, `MFSK16_ACK_SPS × MFSK16_ACK_SYMS`,
   `fsk4_ack_frame_len()`), as one function `longest_ack_len()`, so the two cannot drift.
   - A burst longer than any ACK the protocol defines cannot be an ACK, so the bound costs nothing in
     recognition.
   - The bound applies whatever the receiver's own profile is. Whether the *sender* keys a union ACK
     depends on the sender's profile, which the receiver does not know (a mixed-profile peer).
2. **Recognition takes either codeword.** A burst inside the bound is an ACK if it holds a ShortFEC
   ACK codeword in **either** waveform:
   - **FSK4-ACK:** the existing `holds_an_ack_codeword` scan, step sps/4 = 20.
   - **MFSK16-ACK:** a new scan of one-copy windows (10 240 samples), at step
     `MFSK16_ACK_SPS / 4` = 64, with the same keyless rule: `stage_demodulate_payload` → ShortFEC
     decode → 5 bytes.

   Both are keyless and session-blind, as rule 2 is today, so a foreign station's ACK is recognised
   too.
3. **Order:** FSK4 first. It is the cheap and common case, and finds a whole union ACK in 83 µs.
   Then MFSK16-ACK, only on bursts the FSK4 scan rejected.

**The cost** applies only to failed bursts no longer than 47 k samples:
- The FSK4 scan's worst case at that length is measured at 70 ms x86. A Pi 5 is expected to take
  about 4× that: UNCHECKED, and measured in the test plan.
- The MFSK16-ACK worst case is about 570 windows of 10 240 samples: UNCHECKED, measured likewise.
- Both run after the decode has already failed, inside the decode deadline (#1462), so a slow scan
  delays only the NACK decision of a burst nobody decoded. **If the Pi worst case exceeds 1 s,** the
  FSK4 scan is limited to the burst's first `2 × fsk4_ack_frame_len()` past the lead, because the
  union ACK's FSK4 copy leads its burst. Decided on the measurement, not now.

### What it does not do

- **It does not stop an idle daemon from NACKing its own floor.** B's first NACK each episode was a
  burst on B's floor, which this note does not touch. That is the squelch (B's RX source at 1.0;
  compare #1060), and a peer-liveness gate on idle NACKs. The gate is parked: #1456 keeps idle NACKs
  so that a new peer's first frame is answered, and changing that needs its own review.
- **It does not change what is keyed** for a burst that is evidence, nor the budget.
- **It does not recognise a garbled ACK.** A heard ACK too weak for ShortFEC still counts as a failed
  burst. That is today's behaviour for FSK4 too, and the budget bounds it.

### False recognition — the risk this adds

A real failed data frame recognised as an ACK loses its NACK.
- **Per window:** ShortFEC (13 → 5 bytes, 8 parity, up to 4 byte errors corrected) passes a random
  13-byte word with probability about Σ_{i≤4} C(13,i)·255^i / 256^8 ≈ 1.7·10⁻⁷.
- **Per burst:** at most about 2 300 FSK4 offsets plus 570 MFSK16 offsets, so ≲ 5·10⁻⁴ per failed
  burst inside the bound. That holds only if windows are independent. They are not, because the
  offsets overlap, so this is an upper bound in spirit only: UNCHECKED, and measured in the test plan.
- **Which bursts are exposed:** today, failed bursts ≤ 1.04 s are already exposed to the FSK4 scan,
  which covers every OFDM rung (0.79 s frames). The new exposure is failed bursts of 1.04–5.9 s, and
  MFSK16-ACK windows on every burst inside the bound.

## Test plan (each must fail on today's `main` where marked)

1. **The whole union ACK is not evidence** (fails today). Feed `transmit_ack_mfsk16_k3`'s output for
   a NACK into the OTA failed-burst path through the production entry (`accumulate_capture`, so
   squelch and flush run). Expect: the ACK-not-evidence branch, no `OtaRateDecision`, no NACK verdict.
2. **One MFSK16-ACK copy alone, and copy + gap, are not evidence** (fails today), at lead offsets
   0 to 1 symbol.
3. **Every failed data frame is still evidence** (negative control, which must pass today and after).
   - Sweep every `hpx_hf` rung from the profile's closed list, not a hand list.
   - Each rung's failed frame is cut to every length up to the new bound, under noise.
   - Expect: none recognised as an ACK, over N seeds. Report the measured false-recognition count;
     the bound above is only an estimate.
4. **The twin:** two OTA daemons through `spawn_bridged_pair`, idle, with B at SL1 keying a NACK
   (union ACK). Expect A's ladder not to move and A to key nothing. It fails today, and
   `muted_failures_move_no_ladder` shows the fixture shape.
5. **Cost:** the worst case of both scans at the bound, measured on rpi51 (Pi 5) in release, and
   recorded here.
6. **Sabotage:** with `longest_ack_len()` returned to `2 × fsk4_ack_frame_len()`, tests 1 and 4 go
   red. With the MFSK16-ACK scan removed, test 2 goes red.

Before merge: `scripts/gate.sh`, and `scripts/slow-tests.sh ota`, because CAP-33 drives the OTA
receive.

## Questions for the review

- Is the receiver-side recognition the right layer? The alternative is the sender: key an FSK4-only
  NACK when idle (no traffic outstanding). That makes the K=3 return channel useless exactly when the
  link has collapsed to SL1, so it is rejected here. Is it?
- Is the length bound right to ignore the receiver's own profile?
- Is a 5.86 s window too wide a net for real failed frames, given test 3?

## Consumer

- **The production feed:** `ModemEngine::ota_decode_burst`'s failed-burst branch (`engine.rs:5212`),
  reached from the daemon receive loop (`server.rs` `engine.ota_active()` paths at lines 886 and
  1133).
- Found with `grep -n "holds_an_ack_codeword\|fsk4_ack_frame_len()" crates/openpulse-modem/src/engine.rs`.
  That matched the known guard at 5217 and the union-listen user at 8902.
- `grep -Rln "holds_an_ack_codeword" --include=*.rs crates apps plugins` finds only `engine.rs`.

## Prior art

- **#1456 rule 2** (`docs/dev/design/reply-window-evidence.md`, item 2) is the rule this extends. It
  was sized for the FSK4 ACK only, and the K=3 union ACK came with REQ-WSIG-01 (`engine.rs`
  `MFSK16_ACK_COPIES`).
- **The ACK listen path already union-decodes both waveforms** (`receive_ota_ack_within`, the
  union-listen). This guard is its receive-side twin, and it lagged.
- External modems: UNCHECKED. ARDOP's and VARA's ACKs are a single waveform, so the question does not
  arise in the same form.

## Twins

- **The ISS-side ACK listen** (`receive_ota_ack_within`) already handles both waveforms. It is the
  model for which windows to try.
- **The ARDOP TNC and the KISS front-end** do not run the OTA ladder (`ota_active` is daemon-only;
  `grep -n "ota_active" crates/openpulse-ardop/src crates/openpulse-kiss/src -R` → no match. The same
  filter matches `server.rs`). So they have no twin of this guard.
- **`apps/openpulse-linksim`** drives the controller, not the engine's burst path, so it is
  unaffected: UNCHECKED beyond `grep -n "ota_decode_burst" apps -R` → no match.
