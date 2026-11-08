---
project: openpulsehf
doc: docs/dev/reviews/review-union-ack-recognition-2026-11-08.md
status: resolved
last_updated: 2026-11-08
---

# Review: a heard sub-floor ACK is not ladder evidence (2026-11-08)

Subject: `docs/dev/design/union-ack-recognition.md` (draft), on branch `design/union-ack-recognition`
at `origin/main` 476c8bae. This is the mandatory review (keying class), held before implementation.
Reviewer: Fable. It was read-only. Every number was re-derived from the constants; no cargo was run.

**Verdict: APPROVE WITH CHANGES. Does not block Release 1.** The keying is bounded by the budget
(3 + 6/h), and the runs are attended. **But the design as written does not end the idle ping-pong**
(change 2), and must not be presented as doing so.

## Findings and changes

1. **The mechanism: the rows show the pieces case, and not the ID.**
   - **The event rows.** `dcd_change` rows record DCD *opens* only (0 `busy:false` rows).
     - A shows two opens per B keying: rows 11070 (0.0077) and 11073 (0.0108) for B's NACK at
       11067–11074, and rows 11086 and 11088 for B's NACK at 11083–11089.
     - The first open of each pair produced no decision, so it was held for < 0.5 s. The second
       decided about 2 s later.
   - **The clock.** 136 pairs of B `frame_transmitted` and A `frame_received` give A − B ∈ [0, 8] s,
     never negative. So A's open at 11073 precedes B's ID keying (11074–11075).
   - **Ruling out the ID and A's own floor.** B's second NACK had no trailing ID, and A still keyed
     a `nack_hold` at 11090, so the ID is excluded. A's own floor is excluded too: 0.0108 and 0.0110
     against a floor of 0.0067.
   - **Why the pieces case is the expected form.** `DcdState::new(_, 800)` is a **100 ms** hold
     (`engine.rs:1344`), against the 0.5 s gap. Both holes still have to be closed: AGC-pumped hiss
     can hold the squelch through the gaps.
   - **What the rows cannot show:** whether a codeword would have decoded at A. B's ACK sat about
     1–4 dB over A's band RMS. The fix's effect on this link is a prediction.
2. **The trailing sign-off ID keeps the ping-pong alive.**
   - Every idle NACK is followed by that station's ID: BPSK250 uncoded 18 B, 6 880 samples, 0.86 s.
     That is above the 4 000 evidence floor and inside today's bound, and it is never an ACK
     codeword.
   - A decoded **0 of B's 6 IDs** all day. *(Corrected by the idle-gate review, 2026-11-08: A decoded 1 of B's post-run IDs, row A-11302.)*
   - So after this design, A still counts B's ID after each NACK, and keys.
   - **Changes:**
     - say so in "What it does not do";
     - run twin test 4 with auto-ID on as on air (`auto_id_signoff_idle_secs = 10`). With IDs off it
       passes for the wrong reason;
     - stop describing the peer-liveness gate as merely "needs its own review". It is the only thing
       that ends the idle exchange.
3. **Geometry confirmed:** 4 160, 10 240, 4 000, 46 880 and 8 320.
   - **Fix the sentence on a lone copy.** A lone copy (10 240) is longer than the whole 8 320 bound.
     Today it fails the **length** test and never reaches the scan. It still counts, but for that
     reason.
   - **Reconcile the stated union lengths:** this note says 5.86 s, `reply-window-evidence.md` says
     "about 5.4 s", and `listen_for_ack` says "≈4.84 s", which omits the FSK4 lead.
4. **The slack.**
   - `post` already excludes the lead, so slack covers the *tail*: the 100 ms hold plus one read.
   - A busy receive loop reads up to 1.1 s (8 800 samples; B's log at 13:50:33).
   - Size the slack from the longest read the loop admits.
5. **False recognition.**
   - 1.64·10⁻⁷ per window checks out.
   - The union bound needs **no** independence; the note's caveat is backwards.
   - The unproven premise is that a non-ACK waveform demodulates to *uniform* 13-byte words. Test 3
     measures that, so keep it and reword the caveat.
6. **MFSK16 recognition: use the Costas anchor, not a blind scan.**
   - `Mfsk16Plugin::acquire_copy_offset` is the K=3 listen's primitive. Demodulate one copy at the
     anchor, then ShortFEC.
   - That is one acquisition in place of about 570 demods, and about 1 exposed offset in place of 570.
   - The scan earns its place only if the anchor fails where ShortFEC would still decode. Measure
     that first.
7. **The cost bound is the wrong quantity.**
   - The scan runs in the blocking rx tick, so a slow scan turns the next read into a long blob. That
     blob is itself a floor-passing burst (`reply-window-evidence.md`, "One long read").
   - Bound the Pi worst case by the read stall (on the order of 200 ms), not by the NACK deadline.
8. **The test plan.**
   - With digital-silence gaps, test 1 is test 2: the hold splits the burst. For the length case,
     fill the gaps with noise above the squelch.
   - The twin runs faster than real time and the hold is wall-clock, so test 4 reaches only the
     length case.
   - The fixture's test function is `failures_after_the_budget_mutes_take_no_silent_step`, in
     `muted_failures_move_no_ladder.rs`. The review said the test does not exist; the file exists,
     and that is its test.
   - **Negative control:** sweep every ladder rung × every FEC in the candidate set. Add the
     non-ladder frames the idle guard sees: the BPSK250 18 B ID, and the handshake.
   - **Add the invariant:** for every ACK `transmit_ota_ack` can key (both branches, every
     recommended level), the recogniser accepts it at lead 0–1 symbol and tail ≤ one read.
   - **Sabotage:** also remove the FSK4 scan, which must turn test 1 red.
9. **The layer is right.** Receiver-side is right. The rejection of the sender-side FSK4-only NACK is
   sound. The length bound ignoring the receiver's profile is right in principle, and moot for the
   shipped profiles, since both carry MFSK16 at SL1.
10. **Asserted without evidence — cite:**
    - "the squelch drops on silence": cite the 100 ms hold;
    - "B's RX source at 1.0": cite `rig-b.txt`.

## Disposition

The maintainer chose option A on 2026-11-08: build this guard now, and design the peer-liveness gate
separately, with its own review. Change 2 is stated in the design's "What it does not do". Changes 1
and 3–10 are applied in the design note.

Change 8's twin test was not built, and the design note says why: in the faster-than-real-time twin
the wall-clock hold never splits the gaps, and the residual it would show belongs to the gate's
design. The Pi cost measurement is a merge condition.
