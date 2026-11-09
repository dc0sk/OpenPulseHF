---
project: openpulsehf
doc: docs/dev/design/union-ack-recognition.md
status: resolved
last_updated: 2026-11-08
---

# A heard sub-floor ACK is not ladder evidence: recognise the K=3 union ACK

Work plan M3, A2 (stability), and the HF confirmation. This changes when a station keys a NACK, so it
had the mandatory review before it was built (CLAUDE.md, "Anything that keys a transmitter").
- **The review: Fable, 2026-11-08, APPROVE WITH CHANGES** (`docs/dev/reviews/review-union-ack-recognition-2026-11-08.md`).
- Its changes 1 and 3–10 are applied below, each marked with its number.
- Change 2 is stated under "What it does not do". The maintainer chose option A: build this now, and design
  the peer-liveness gate separately.
- **Implemented 2026-11-08** (see Implementation).

## Problem — seen on air

On-air task 32, after f2l's traffic ended, both daemons stayed up for about 3 h and keyed a NACK
ping-pong. The task 32 review is in `docs/dev/onair-tasks.md` on `m3/onair`; the evidence is the f2l
directory's `events-{a,b}-full-pre-f2m.ndjson.gz`.

- **How it started.** B's receiver opened a burst on its own floor (DCD energy 0.0182 and 0.0188;
  B's idle busy p10–p50 is 0.018–0.023), and NACKed it. That was a `leaked` verdict, from a budget
  the run had banked.
- **What followed.** Each station's raised DCD then follows the other's keying.
  - A keyed 9 times: 5 NACKs and 4 sign-off IDs.
  - B keyed 7 times: 4 NACKs and 3 IDs.
- **At about 16:22, A's first decision was `nack_step_down` SL9 → SL8, keyed.** It was a ladder move, by a
  station with no traffic.

B's ladder was at SL1, so its NACK recommends the MFSK16 sub-floor rung. `transmit_ota_ack` therefore
keys the **K=3 union ACK** (`transmit_ack_mfsk16_k3`): one FSK4 ACK, then three MFSK16-ACK copies,
each after 0.5 s of silence.

**What the rows show (review, change 1):**
- **A heard B's NACK in pieces.** `dcd_change` rows record DCD opens only. A shows two opens per B
  NACK: rows 11070 and 11073 for B's 11067–11074, and 11086 and 11088 for B's 11083–11089. The first
  of each pair was held for under 0.5 s; the second decided about 2 s later.
- **It was not B's ID.** Clock pairing of B `frame_transmitted` with A `frame_received` gives
  A − B ∈ [0, 8] s, so A's open at 11073 precedes B's ID. And B's second NACK had no trailing ID, yet A
  still keyed.
- **It was not A's own floor:** the opens read 0.0108 and 0.0110, against A's floor of 0.0067.
- **Why pieces are the expected form:** `DcdState::new(_, 800)` (`engine.rs`) is a **100 ms** hold,
  against the 0.5 s gap.
- **What the rows cannot show:** whether a codeword would have decoded at A. B's ACK sat about 1–4 dB
  over A's band RMS.

## The geometry — measured

Measured with a temporary unit probe, through the shipped modulators. It built the union exactly as
`transmit_ack_mfsk16_k3` does. Release build, x86.

| item | samples (8 kHz) | seconds |
|---|---|---|
| `fsk4_ack_frame_len()`: FSK4-ACK, 13 ShortFEC bytes, 52 symbols × 80 | 4 160 | 0.52 |
| one MFSK16-ACK copy: 40 symbols × 256 | 10 240 | 1.28 |
| gap (`MFSK16_ACK_GAP_S`) | 4 000 | 0.50 |
| the whole union: 4 160 + 3 × (4 000 + 10 240) | **46 880** | **5.86** |
| the old guard, `post <= 2 × fsk4_ack_frame_len()` | 8 320 | 1.04 |

**The stated union lengths disagree (review, change 3).** `reply-window-evidence.md` says "about 5.4 s",
and `listen_for_ack` says "≈ 4.84 s", which omits the FSK4 lead. 5.86 s is the measured value.

**So there are two holes:**
1. **Length.** A union ACK heard as one burst is 5.86 s, past the 1.04 s bound, so it is never
   scanned. This happens when pumped hiss under a receiver AGC holds the squelch through the gaps.
2. **Pieces.** At the gaps the burst ends, and each MFSK16 copy arrives on its own.
   - A lone copy is 10 240 samples, longer than the whole old bound, so it fails the **length** test
     and never reaches the scan (review, change 3).
   - Had it reached the scan, it would still have failed: it carries no FSK4 codeword.
   - Either way it counts as a failed burst.

## Design

The change sits at the one check in `ota_decode_burst`, the shared seam every OTA failed burst passes through.

1. **The bound is the longest ACK any station keys**, plus a tail slack. The length is
   `longest_ack_len()`, computed from the transmit's own constants (`MFSK16_ACK_COPIES`,
   `MFSK16_ACK_GAP_S`, `MFSK16_ACK_SYMS × MFSK16_ACK_SPS`, `fsk4_ack_frame_len()`).
   - **The tail slack (change 4)** is `ACK_TAIL_SLACK_SAMPLES` = 800 + 8 800. `post` already
     excludes the lead, so the slack covers the tail: the DCD's 100 ms hold, plus the longest
     receive read seen on air (8 800 samples, a busy Pi 4 loop in f2l, B's log 13:50:33). A burst
     ends on a read boundary.
   - **The receiver's own profile is ignored.** Whether the *sender* keys a union ACK depends on the
     sender's profile. That is right in principle, and moot for the shipped profiles: both carry MFSK16
     at SL1 (change 9).
2. **Recognition takes either codeword.** A burst inside the bound is an ACK if it holds a ShortFEC
   ACK codeword of either waveform. Both checks are keyless and session-blind, as #1456's rule 2 is.
   - **FSK4-ACK:** the existing `holds_an_ack_codeword` scan.
   - **MFSK16-ACK (change 6):** `holds_an_mfsk16_ack_codeword`.
     - It anchors on the plugin's Costas lock (`acquire_copy_offset`), not a blind offset scan.
     - It takes the copy slots at `anchor + k·span` and checks each copy, then their MAP sum, for a
       5-byte ShortFEC decode.
     - The slot gathering is `mfsk16_k3_copy_llrs`, now shared with the K=3 ACK listen
       (`decode_mfsk16_k3_ack`), so the guard and the listen cannot drift.
3. **Both scans look only at the burst's head (change 7)**, where an ACK's first codeword ends.
   - The union's FSK4 copy leads its burst, and its first MFSK16 copy follows one gap later. A piece
     is its own head.
   - The head lengths past the lead (`ack_head_lens`) are FSK4 plus the slack (13 760 samples), and
     FSK4 + gap + copy plus the slack (28 000).
   - The head for MFSK16 still holds a whole union's first MFSK16 copy. That matters at SL1, where
     FSK4 can die in a fade while the MFSK16 copies decode.
4. **Order:** FSK4 first, then MFSK16, only on bursts the FSK4 scan rejected.

**The cost (change 7): bounded by the stall it causes in the receive loop**, not by the NACK deadline.
A slow scan turns the next read into one long read, and a long read is itself a floor-passing burst
(`reply-window-evidence.md`, "One long read").
- **Worst case, x86 release, noise with no ACK:**
  - over the whole 56 480-sample bound: 83 + 170 ms (too slow, hence the head windows);
  - at the heads: **19 + 85 ms**.
- **Bursts shorter than one MFSK16 copy** skip that check (`mfsk16_k3_copy_llrs` returns `None`).
  That includes every failed OFDM frame (0.79–1.51 s).
- **The Pi 5 (rpi51, station A), release, the same probe:** **31 + 126 ms** worst case, over five
  runs. That is under the review's ~200 ms stall bound.
- **The Pi 4 (station B, the A→B receiver, with the tightest ACK margin):** **68 + 265 ms**, over the
  review's ~200 ms. It was built with rustc 1.96.1 (station B's toolchain, not the pinned 1.97.1) and
  run with B idle.
  - **What this is still under:** the 0.5 s evidence floor (4 000 samples). A 333 ms stall adds about
    2 700 samples to the next read, which by itself does not make a floor-passing burst.
  - **Which bursts pay it:** only failed bursts between one MFSK16 copy and the bound that hold no FSK4
    codeword, for example a failed SL6 frame (4.18 s).
  - **Accepted by the maintainer (2026-11-09).** The optimisation is parked in the work plan.

### What it does not do

- **It does not end the idle ping-pong (change 2).**
  - Every idle NACK is followed by that station's sign-off ID: BPSK250 uncoded, 18 B, 6 880 samples,
    0.86 s. That is above the 4 000 evidence floor and inside the bound, and it is never an ACK
    codeword.
  - A decoded few of them. The idle-gate review (`idle-nack-gate.md`) corrected the union review's "0 of
    6": A decoded **1** of B's post-run IDs (8 B payload, event row A-11302), which correctly keyed
    nothing.
  - So after this change A still counts B's ID after each B NACK, and keys.
  - **Only a peer-liveness gate on idle NACKs ends the exchange.** #1456 keeps idle NACKs so that a
    new peer's first frame is answered, so the gate is a keying change with its own design and
    mandatory review. The maintainer chose to design it next (option A).
  - Until then, the NACK budget (3, plus 6 per hour) is what bounds idle keying, and the on-air runs
    tear the daemons down after the sign-off IDs (task 33).
- **It does not touch an idle daemon NACKing its own floor** (B's first NACK each episode). That is the
  squelch, B's RX source at 1.0 (`rig-b.txt` of f2l), compare #1060.
- **It does not recognise an ACK too weak for ShortFEC.** That stays a failed burst, as for FSK4 today.
  On air this is not a corner case.
  - The idle-gate review found that B counted A's FSK4 NACKs heard at only +0.5–1.5 dB over its
    floor; the FSK4 guard, which already existed, did not fire.
  - So on that link no content recognition stops the idle exchange. That is the peer-liveness gate's job.
- **It does not see an ACK behind a ride-along station ID.** When an ID is due, it can go in front of
  the ACK in the same keying, uncoded in the daemon's mode. The ACK's codeword then starts up to one ID
  airtime past the head (6 880 samples at BPSK250).
  - An active sender hears that ACK in its own ACK listen, not here, so only an idle or third station
    is affected.
  - Extending the heads by one ID would cost about +50 % / +25 %.
  - It never occurred on air: every post-run ID was an interval or sign-off ID in its own keying.
  - Left to the idle gate.

### False recognition (change 5)

A real failed data frame taken for an ACK loses its NACK.
- ShortFEC (13 → 5 bytes, 8 parity, up to 4 byte errors corrected) passes a uniformly random 13-byte
  word with probability Σ_{i≤4} C(13,i)·255^i / 256^8 ≈ 1.6·10⁻⁷.
- The union bound over the windows tried is valid whatever their overlap. **The unproven premise was
  uniformity:** a structured data frame through the FSK4 or MFSK16 demodulator need not give uniform
  words.
- The negative-control test measures exactly that. Its result is under Implementation.

## Implementation (2026-11-08, branch `design/union-ack-recognition`)

`crates/openpulse-modem/src/engine.rs`:
- the guard in `ota_decode_burst`;
- `ACK_TAIL_SLACK_SAMPLES`, `longest_ack_len`, `ack_head_lens`;
- `holds_an_mfsk16_ack_codeword`;
- `mfsk16_k3_copy_llrs`, split out of `decode_mfsk16_k3_ack`.

**Tests.** All run through `accumulate_capture` and `ota_decode_burst`, the daemon tick's path, over the
recorded IC-9700 idle (`tests/idle_flicker_is_not_evidence.rs`), except the last, which is a unit test.

| test | what it shows |
|---|---|
| `a_union_ack_heard_in_pieces_is_not_evidence` | asserts the fixture split the ACK (≥ 2 bursts), then that no piece keyed a NACK |
| `a_union_ack_heard_whole_is_not_evidence` | the gaps are held open with hiss; asserts one burst ≥ 90 % of the ACK, then no NACK (change 8: the length case needs its own fixture) |
| `every_ack_the_transmit_keys_is_not_evidence` | the invariant (change 8): `transmit_ota_ack` for one level per ACK waveform, from the profile's closed level list (the waveform depends only on whether the level's mode is MFSK16); asserts both waveforms were covered |
| `engine::union_ack_guard_tests::no_data_frame_or_station_id_is_taken_for_an_ack` | every data rung of `fast` and `robust` with its own FEC, and the BPSK250 18 B station ID, each clean and under noise at three levels, against both recognisers |

The negative control found **none** taken for an ACK. That covers every frame built, at least 15 (the test asserts it), × 4 variants; the exact count was not printed. It carries a positive control in the same
harness: an FSK4 ACK, a whole union ACK and a lone MFSK16 copy are all taken.

**Results with the change:** all 6 tests in `idle_flicker_is_not_evidence` pass, and so does the unit test.

**Sabotage** (release, each on the committed change, restored from `HEAD`):

| sabotage | goes red |
|---|---|
| the old `2 × fsk4` bound | whole, pieces, invariant |
| the MFSK16 recogniser removed | pieces, invariant |
| the FSK4 scan removed | `an_ack_on_air_is_not_evidence`, pieces (the first piece is the lone FSK4 copy), invariant |

The pre-existing negative tests (`idle_behind_a_250hz_filter…`, `a_frame_failing_at_the_decode_edge…`)
stay green throughout.

**Not built: the twin test (review, change 8).** It was meant to show the guard end to end through two
real daemons.
- The DCD hold is wall-clock and the twin bridge runs faster than real time, so the gaps never release
  there. The twin reaches only the length case, which the engine tests already cover through the
  production entry.
- With auto-ID on, as on air, it would show the residual stated above: A keying on B's trailing ID.
- That residual is the next design (the peer-liveness gate). Its twin belongs there.

## Consumer

- **The production feed:** `ModemEngine::ota_decode_burst`'s failed-burst branch, reached from the
  daemon receive loop (`server.rs`, the `engine.ota_active()` paths).
- **The second consumer:** the K=3 ACK listen, `decode_mfsk16_k3_ack`, which now shares
  `mfsk16_k3_copy_llrs`.
- Found with `grep -n "holds_an_ack_codeword\|fsk4_ack_frame_len()\|acquire_copy_offset"
  crates/openpulse-modem/src/engine.rs`. That matched the known guard and the union listen.
  `grep -Rln "holds_an_ack_codeword" --include=*.rs crates apps plugins` finds only `engine.rs`.

## Prior art

- **#1456 rule 2** (`docs/dev/design/reply-window-evidence.md`, item 2) is the rule this extends. It
  was sized for the FSK4 ACK only; the K=3 union ACK came with REQ-WSIG-01.
- **The ISS-side ACK listen already union-decodes both waveforms.** This guard is its receive-side
  twin, and it had lagged.
- External modems: UNCHECKED. ARDOP's and VARA's ACKs are a single waveform.

## Twins

- **The ACK listen** (`receive_ota_ack_within` / `decode_mfsk16_k3_ack`) shares the slot gathering now.
- **The ARDOP TNC and the KISS front-end** do not run the OTA ladder.
  `grep -Rn "ota_active" crates/openpulse-ardop/src crates/openpulse-kiss/src` has no match; the same
  filter matches `server.rs` 4 times.
- **`apps/openpulse-linksim`** drives the controller, not the engine's burst path.
  `grep -Rn "ota_decode_burst" apps` has no match; the same filter matches `engine.rs`.
