---
project: openpulsehf
doc: docs/dev/design/idle-nack-gate.md
status: resolved
last_updated: 2026-11-09
---

# An idle receiver does not answer what is not a peer's frame

Work plan M3, A2 (stability), and the HF confirmation. It changes when a station keys a NACK, so it
had the mandatory review **before** a rule was chosen.
- **The review: Fable, 2026-11-08, CHANGES** (`docs/dev/reviews/review-idle-nack-gate-2026-11-08.md`).
  It recommends G1 and rejects G2, G2b and G3.
- **The maintainer chose G1, plus option B** (2026-11-09): a newly started OTA session also counts as a
  live peer. The delta B had its own short review check (see the review's Disposition).
- **The ride-along ID is not restricted to positive ACKs** (the maintainer, 2026-11-09). On a link that
  only NACKs, a restriction would push the ID to the forced interval ID, which can fall over the
  peer's frame. And it would not have changed task 32: there the IDs were interval IDs in their own
  keying.
- **Implemented 2026-11-09** (see "The rule, as built").

**Corrections the review made to the problem as first written below:**
- A decoded **1** of B's post-run IDs, not 0.
- The ride-along ID in front of an ACK **never occurred** on air: every post-run ID was an interval
  or sign-off ID in its own keying.
- A's step-down SL9 → SL8 opened the second episode, not the first.
- **A fourth source:** heard ACKs **too weak to decode**. B kept answering A's FSK4 NACKs at +0.5–1.5 dB.
  That is why no rule that recognises a burst's content can end the exchange.

## The rule, as built (G1 + B)

- **Where:** in `ota_decode_and_ack_inner`'s failed branch, after the ACK guard
  (`crates/openpulse-modem/src/engine.rs`).
- **What:** a failed burst is not ladder evidence (no ACK, no ladder move, no budget charge) unless
  `p_ok`, or `ota_peer_live()`.
- **`ota_peer_live()`:** `last_ladder_decode_at` is within `OTA_PEER_LIVE_SAMPLES` (10 min at 8 kHz) of
  the current `listening_samples`.
- **`last_ladder_decode_at` is set** when an OTA ladder frame decodes, before the deadline checks: a
  frame decoded past its ACK window still shows a live peer.
  - The uncoded fallback returns earlier and does not set it.
  - Nor does a heard ACK, or the station's own transmission.
- **B:** `start_ota_session` also sets it.
  - The production callers are daemon startup under `ota_enabled` (`server.rs`) and the operator's
    `StartOtaSession` command.
  - So a freshly started station answers a new peer's first failed frame (#1456's constraint, in the
    case where it is coherent).
  - A station left idle for 10 min stops answering.
- **The clock** is listening time. It stops while the daemon transmits, and the twin is
  deterministic in it.
- **One owner for the period:** the daemon's NACK-budget leak `LEAK_SAMPLES` now takes its value from
  `openpulse_modem::OTA_PEER_LIVE_SAMPLES`.

**What it costs:**
- **A new peer** that calls a station idle for more than 10 min gets no NACK for a failed first frame.
  The sender then abandons after its silent windows and falls back. That is the "muted until the next
  decode" state #1456 called a defect, now limited to an idle station.
  - The review's remedy, a sender that tries SL1 once before `PeerSilent`, was not chosen.
- **For the first 10 min after start,** two idle daemons can still echo each other, bounded by the
  budget (3, plus one per 10 min).

## Problem — seen on air

On-air task 32, after f2l's traffic ended, both OTA daemons stayed up for about 3 h. Sources: the
reviews of task 32 and of the union-ACK design, and the f2l directory's
`events-{a,b}-full-pre-f2m.ndjson.gz`.
- **What happened.** A keyed 9 times (5 NACKs, 4 sign-off IDs), and B 7 times (4 NACKs, 3 IDs). A's
  first decision at 16:22 was `nack_step_down` SL9 → SL8: a ladder move by a station with no traffic.
- **How it started.** B NACKed a burst its DCD opened **on B's own floor** (energy 0.0182 and 0.0188;
  B's idle p10–p50 is 0.018–0.023).
- **What sustained it.** From then on, each station answered the other's keying. That keying was a
  NACK, sometimes with the station ID riding in front of it, followed by a sign-off ID.

The union-ACK guard stops a heard **ACK** counting. Three other idle evidence sources remain:
1. **The station's own floor:** B's first burst in each episode.
2. **A peer's station ID.** It is BPSK250 uncoded, 18 B, 0.86 s.
   - It is above the 4 000-sample evidence floor, and it is not an ACK.
   - A decoded **0 of B's 6** that day.
   - The sign-off ID is a separate burst after the NACK. The ride-along ID sits *in front of* the ACK,
     in the same keying (`server.rs`, `kind = "with-ack"`). It is uncoded in the daemon's mode, so a
     slow mode makes it seconds long, and it moves the ACK's codeword past the union guard's head
     windows.
3. **A foreign over** (#1460), which is HF only.

The NACK budget bounds all of this: 3 NACKs, plus one per 10 min of listening. That is the only bound,
and it allows a ladder move per episode.

## Why idle NACKs exist (the constraint)

`nack_budget.rs` and #1456: "the peer's first failed frame got silence and its ISS abandoned after two
silent windows." An idle receiver must still answer **a new peer's first frame**, even when that frame
fails. Any gate has to keep that.

## Candidate rules (as drafted; the review chose G1)

- **G1:** peer liveness by time. **Chosen**, with B.
- **G2:** preamble evidence when no peer is live. **Rejected:**
  - only the OFDM plugin has a carrier detector;
  - SL1–SL6 would need four new fitted detectors;
  - none confirms at +1–2 dB.
- **G2b:** a length rule for the PN-63 ID. **Rejected:** it is G1 plus a collision with #1452.
- **G3:** recognise the ID as an ID. **Rejected:** a decoded ID already keys nothing.

## Tests (as built)

**Modem**, through `accumulate_capture` and `ota_decode_burst`, over the recorded idle
(`tests/idle_flicker_is_not_evidence.rs`):
- **`with_no_live_peer_a_failure_keys_nothing`.** After `OTA_PEER_LIVE_SAMPLES` of listening, two
  bursts key nothing:
  - the failing QPSK250-D frame that keys a NACK in a live session;
  - an FSK4 NACK with a station ID behind it at +1.5 dB.

  Each asserts that its burst was flushed.
- **`a_decode_makes_the_peer_live_again`.** After the window, a clean frame decodes, and the same
  failure then keys.

The listening time is silence, then 60 s of idle so the floor re-learns the band. A first version fed
only silence: nothing flushed, and one test passed vacuously.

**Daemon**, `nack_budget::tests::with_no_live_peer_only_a_p_confirmed_failure_keys`:
- **The control:** a fresh session's failure keys.
- **The subject:** after the window, the same failure keys nothing and spends no budget, while a
  P-confirmed failure still keys.
- A first version had no control and dropped its warm-up flushes undecoded. Their flush state then
  disqualified the burst under test, so an always-live sabotage passed. Fixed.

**Sabotage** (release, on the committed change):

| sabotage | goes red |
|---|---|
| always live | both no-live tests |
| never live | `a_frame_failing_at_the_decode_edge_is_still_evidence`, `a_decode_makes_the_peer_live_again` |
| G1 without B | 19 modem and daemon tests that answer a failure in a fresh session |

**Not built:**
- the review's twin with auto-ID (`spawn_bridged_pair_in_hiss`, traffic then idle);
- the 30-minute raised-floor idle.

The twin's bridge feeds idle in real time, so the 10-minute window makes it a test over 10 minutes. The
engine-level tests cover the rule through the production entry. Both are UNCHECKED, and the next 2 m
run after a deploy is their on-air check: keep the daemons up for over 10 min after the sign-off IDs,
count key-ups from the daemons' `ptt_changed` events, and read `p_false_opens`, since P is the only
remaining way through the gate (delta check, item 6).

## Consumer

`crates/openpulse-daemon/src/nack_budget.rs` (`decode_ota_burst`) and the engine's failed-burst
verdict (`ota_decode_burst`), reached from `server.rs`'s receive loop when `engine.ota_active()`.
UNCHECKED beyond `grep -n "decode_ota_burst" crates/openpulse-daemon/src/*.rs`.

## Prior art

#1456 (`reply-window-evidence.md`), #1460 (the evidence rule, foreign overs), #1452 (fragments are
evidence), `union-ack-recognition.md`, and `ofdm-preamble-detect.md` (decision 4, the evidence
coherence). External modems: UNCHECKED. ARDOP answers only inside a session; whether it answers
outside one is UNCHECKED.

## Twins

The ARDOP TNC and the KISS front-end do not run the OTA ladder (`ota_active` is daemon-only;
`grep -Rn "ota_active" crates/openpulse-ardop/src crates/openpulse-kiss/src` → no match).
