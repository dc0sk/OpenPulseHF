---
project: openpulsehf
doc: docs/dev/reviews/review-idle-nack-gate-2026-11-08.md
status: resolved
last_updated: 2026-11-09
---

# Review: an idle receiver does not answer what is not a peer's frame (2026-11-08)

- **Subject:** `docs/dev/design/idle-nack-gate.md` (draft), on branch `design/idle-nack-gate` at `origin/main` eef45ab8.
- **This is the mandatory review** (keying class), held before a rule is chosen.
- **Reviewer:** Fable, read-only.
- **Evidence:** f2l's `events-{a,b}-full-pre-f2m.ndjson.gz`. They have no timestamps; the `ota_status`
  row count serves as the clock, with A ≈ B − 145 s, consistent across both episodes. Plus
  `engine.rs`, `nack_budget.rs`, `server.rs`, `station_id.rs`, `ota_rate.rs`, `bpsk/modulate.rs` and
  the union-ACK branch.

## Prompt

The reviewer was given:
- the draft;
- `reply-window-evidence.md`, `nack_budget.rs`, the daemon's OTA receive path (including the
  ride-along and sign-off IDs), `ota_decode_burst`'s evidence rules and `ofdm-preamble-detect.md`
  (decision 4);
- the union-ACK design and its review;
- the task 32 and 33 reviews, and f2l's full post-run event files.

It was asked to falsify:
1. the problem statement, from the event rows;
2. each candidate (G1, G2, G2b, G3): does it end the idle exchange while keeping #1456's constraint?
3. the ride-along ID's handling and its cost;
4. whether a simpler rule was missed;
5. what "live" should mean, and over what `T_live`.

It was asked to recommend one rule with a test list. And: **"Does this block Release 1? Say so."**

## Verdict

**Verdict: CHANGES. Does not block Release 1.** The keying is bounded (3 + 6/h), the runs are
attended, and A2 is met on 2 m. **Recommend G1** with the semantics below; **reject G2, G2b and G3.**

## 1. The problem statement: three errors and one omission

The episodes, reconstructed on B's clock (A = B − 145):

**Episode 1:**
- **B 2531:** `nack_step_down leaked`, on DCD 0.0182–0.0188 (B's floor). It keyed a union NACK, then
  an ID in a second keying.
- **A 2384–2386:** DCD 0.0077 → 0.0108 (A's floor is 0.0067), then `nack_hold` keyed: an FSK4 NACK
  plus an ID.
- **A 2386:** DCD 0.0093 (B's ID), then at 2389 `nack_hold` keyed; sign-off at 2405.
- **B 2533 and 2547:** DCD 0.0257 and 0.0534. B's holds are now **muted**: the budget is spent.

**Episode 2:**
- **B 11351:** `leaked`, on its floor. It keyed a union NACK plus an ID.
- **A 11203:** DCD 0.0108, then **at 11204 `nack_step_down` SL9 → SL8**, plus an ID.
- **A 11204:** DCD 0.0099 (B's ID), then at 11206 keyed.
- **B 11360:** keyed, on DCD 0.0200 and 0.0255 (A's FSK4 NACK and ID, +0.5–1.5 dB). At 11385 it keyed
  again, on 0.0277 and 0.0597.
- **A 11219:** DCD 0.0110, then at 11220 keyed: its 3rd, and the budget is spent. Sign-off at 11233;
  11240 and 11244 muted.
- **B 11451:** sign-off ID. **At A 11302, `frame_received BPSK250 8 B`:** A decoded B's ID ("DE DD2ZM",
  8 B of payload, 18 B on the wire), `ack None`, and keyed nothing.

**The errors:**
- **"A decoded 0 of B's 6" is false:** A decoded 1 of 4 post-run, and A-1461 in the run.
- **The ride-along ID in front of the ACK never occurred.** Every post-run ID was an *interval* ID:
  forced by `due_reason` (`since_id ≥ interval_ms`), armed by the NACK's `note_tx`, and keyed right
  after the NACK as its own keying. Or it was a sign-off 10–16 s later.
- **"A's first decision was SL9 → SL8"** is wrong for episode 1, which opened with `nack_hold SL9`.
  The step-down was episode 2's.

**The omission: source (c), a heard ACK too weak to decode.**
- B counted A's FSK4 NACKs at +0.5–1.5 dB. The FSK4 guard already exists, and it did not fire.
- Whether A would recognise B's union ACK at +2 dB is a prediction; the union review said so.
- So "after the union guard only (a) and (b) remain" is unproven at A, and false at B.
- **Any rule that recognises the burst's content fails at these levels.** That decides against G2, G2b
  and G3, and against extending the head windows.

## 2. The candidates

- **G1 ends the exchange.** With A silent, nothing echoes.
  - B's own-floor NACKs remain: 2 keyings and 2 IDs in 3 h, instead of 16. That is B's squelch at its
    floor (RX source 1.0, the #1060 class), a separate item.
  - It also closes #1460's idle case.
  - It breaks the #1456 constraint (§4).
- **G2: reject for Release 1.**
  - Only the OFDM plugin implements `carrier_detector`; `FlushSpans.p_confirmed` is the only preamble
    flag.
  - SL1–SL6 would need four new detectors with fitted thresholds (MFSK16 Costas, `--++`, PN-63, QPSK
    16-symbol). Those are new DSP constants with no inventory, and none confirms at +1–2 dB.
  - The 269-flicker question cannot be answered without building them.
- **G2b: reject.** The ID does carry PN-63 (`is_pn_mode("BPSK250")` is true; `transmit` →
  `stage_modulate_payload` → the plugin's preamble), so length is the only separator (6 880 samples
  against 8.3 s). But it is G1 plus a collision with #1452, and recognition still fails when weak.
- **G3: reject.** A decoded ID already keys nothing; the undecoded ones are the problem.

## 3. The ride-along ID

- **Who hears it through `ota_decode_burst`:** a third or idle station (f2l), and an ISS whose window
  has closed.
- **Extending the heads by one ID:**
  - The ID is 6 880 samples at BPSK250. It uses `active_mode`, so it is seconds long only if
    `[modem] mode` is slow.
  - The cost is about +50 % / +25 % of the scan, about 47 + 160 ms on the Pi 5.
  - It helps only when the codeword decodes.
  - **Not now.**
- **A cheap companion:** ride the ID only with `AckOk`, never in front of a NACK.

## 4. A simpler rule, and the constraint

**"No NACK without a peer" is G1.**
- The daemon has no OTA peer state: `ota_session_id` is its own callsign (`server.rs:800`), the
  handshake is optional, and ACKs carry a hash of the receiver's own callsign.
- A decode is the only evidence of a peer.

**A sender-side "no ID after an idle NACK"** is a §97.119 regression: F6 treats the ACK as a
transmission. Reject.

**The constraint is weaker than stated.**
- An idle receiver's `rx_candidates` are `{recommended, confirmed}` only, so A at SL9 cannot decode a
  new peer's SL2 frame at all, and its NACK recommends SL8.
- So the constraint is coherent only for a receiver at the entry rungs.
- An entry-rung exemption reopens the hole: B sat at SL1, and two fresh daemons step SL2 → SL1 on
  their first NACK and union-echo.

**Say plainly:** G1 recreates the "muted until the next decode" state that #1456's finding 3 called a
defect.
- **Route that harm to the sender:** a never-ACKed send at `initial_level` tries SL1 once before
  `PeerSilent` (the shape of `climb_into_silence_falls_back.rs`).
- The f2h review rejected "a sender-side step-down on silence", but in a different context (decodable
  frames, a deaf receiver).

## 5. The rule, exactly

- **Where:** in `ota_decode_and_ack_inner`, the failed branch, after the P check.
- **What:** a failed burst keys no NACK and moves no ladder **unless** `p_ok`, or
  `listening_samples − last_ladder_decode_at ≤ T_LIVE`.
- **What "live" means:** this engine decoded an OTA **ladder** frame (`payload` and `ack` both `Some`).
  Not the uncoded fallback, not a heard ACK, not its own TX.
- **The clock:** `listening_samples`. TX time does not count, and the twin is deterministic.
- **`T_LIVE` = `LEAK_SAMPLES` (10 min):** one constant, above the longest live failing run (4 × 75 s at
  SL2).
  - **Falsifier:** a live peer failing for more than 10 min of listening without a decode. The budget
    already mutes that link.
- The budget's charge and leak are unchanged.

## Tests (red on today's main unless marked)

1. B's union NACK + ID at +2 dB over the recorded idle, into an SL9 engine with no decode: no
   decision, no NACK (A-11204).
2. A's FSK4 NACK + ID at +1.5 dB into an SL1 engine: silent (B-11360).
3. Raised-floor idle for 30 min (squelch at the band RMS, as B's): zero keyed. **It must key on today's
   main first**, or it is fixture-conditioned.
4. **Control:**
   - a decode, then `a_frame_failing_at_the_decode_edge_is_still_evidence` keys at `T_LIVE − 1` and is
     silent at `T_LIVE + 1`;
   - a decode re-arms it.
5. A P-confirmed failure with no live peer still keys.
6. **Twin** `spawn_bridged_pair_in_hiss`, with `auto_id_signoff_idle_secs = 10`, traffic then idle: no
   keying after the sign-offs.
7. **The cost, measured:** a fresh pair, first SL2 frame at −3 dB. Today it is delivered via SL1. With
   G1 it is abandoned, or the sender fallback makes it green.

**Sabotage:**
- `T_LIVE = ∞` turns tests 1, 2 and 6 red;
- `T_LIVE = 0` turns test 4 red.

Also fix in the design note: the ID is BPSK250 whatever the rung; cite the interval-ID mechanism; drop
"ride-along" as the on-air cause.

## Disposition

**The maintainer, 2026-11-09:**
- **G1 is built.**
- **Option B is added:** `start_ota_session` counts as a live peer. Bare G1 broke 19 tests that answer
  a failure in a fresh session, and B keeps #1456's constraint for a freshly started station.
- **The sender-side SL1 fallback is not chosen.**
- **The ride-along ID is not restricted** to `AckOk`.

**The check of delta B (Fable, 2026-11-09, read-only, no cargo): APPROVE. Does not block Release 1.**
1. **It does not reopen the task 32 echo:** on air the echoes began 15 min and 2.7 h after traffic,
   both past the window.
   - **Two daemons started together and left idle** can echo for their first 10 min. That is bounded
     at 3 NACKs (+ IDs) per station: a heard ID decodes only through the uncoded fallback, which
     neither resets the budget nor sets liveness.
   - At the window's edge, the leak (`≥`) and liveness (`>`) can allow a 4th NACK on one read. That
     is negligible.
2. **No path restarts the session unattended.**
   - `server.rs:247` runs once, at startup.
   - `StartOtaSession` is sent only by the CLI `daemon ota start`.
   - The probe is under `#[cfg(test)]`.
   - Panel, TUI, ARDOP, KISS, handshake, QSY and the repeater engines do not call it.
3. **The anchor is right.** `listening_samples` has a single writer and is never reset.
4. **Liveness is set before the deadline checks**, which is right: a late frame still proves a peer.
5. **The uncoded fallback is excluded** at all three of its exits.
6. **The tests are sufficient for the delta.** Optional, not blocking: keying at `LIVE − 1` after
   start, and `StartOtaSession` re-arming after the window expires.
   - Add to the on-air check: read `p_false_opens` after the 10-min idle, since P is the only
     remaining way through the gate.
   - Park, as pre-existing: the comment at `server.rs:1085` says nothing sets `engine.ota` back to
     `None`, but `StopOtaSession` does.

## Consumer

`ota_decode_and_ack_inner`'s failed branch, reached through `ModemEngine::ota_decode_burst` from
`nack_budget::decode_ota_burst` in the daemon's receive loop. The design note's `## Consumer` gives the
commands.

## Prior art

#1456 (`reply-window-evidence.md`), #1460, #1452, `union-ack-recognition.md`, and
`ofdm-preamble-detect.md` (decision 4). See the design note.

## Twins

The ARDOP TNC and the KISS front-end run no OTA ladder. The repeater's extra engines start no OTA
session (delta check, item 2).
