---
project: openpulsehf
doc: docs/dev/design/idle-nack-gate.md
status: draft
last_updated: 2026-11-08
---

# An idle receiver does not answer what is not a peer's frame

Work plan M3, A2 (stability), and the HF confirmation. It changes when a station keys a NACK, so it
gets the mandatory review **before** it is built. Review: pending.

This follows `union-ack-recognition.md`; the maintainer chose option A on 2026-11-08: build that guard
now, and design this one separately.

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

## Candidate rules (for the review to choose between or refute)

**G1: peer liveness by time.**
- A failed burst keys a NACK only if the receiver decoded an OTA ladder frame within `T_live`.
- `T_live` has to exceed a peer's longest failing run: several SL2 frames of 66.56 s each.
- **It breaks the constraint:** a new peer's first failed frame gets silence. The sender then abandons
  after two silent windows, and the climb-into-silence fallback steps it down. Whether that costs a
  start-up more than today is UNCHECKED.

**G2: liveness, or the burst is evidently a ladder frame (recommended for review).**
- With no live peer, a failed burst keys a NACK only if a ladder candidate's **preamble** is confirmed
  in it, at the candidate's acquisition coherence:
  - P for the OFDM rungs (`P_EVIDENCE_COHERENCE`);
  - the PN-63 sync for SL3–SL5;
  - the existing per-candidate sync elsewhere.
- **What it stops:**
  - **hiss** has no preamble;
  - **a heard ACK** has none: FSK4-ACK and MFSK16-ACK carry no ladder sync.
- **What it keeps:** a new peer's first frame, which has its preamble.
- **The open question is the station ID.** It is uncoded in the daemon's mode. On BPSK250 that mode is
  the SL5 waveform, PN-63 included, so the ID would pass as an SL5 frame. Then G2 alone does not stop
  the ID-driven exchange.
  - **G2b:** add a length rule. Every ladder frame has a fixed length for payloads up to 209 B
    (`ota-climb-memory.md`). A burst that confirms a preamble but is much shorter than the shortest
    frame of that preamble's rung is a fragment or an ID.
  - But #1452 keeps fade-split fragments as evidence, so this collides with #1452 **while a peer is
    live**. It is therefore applied **only when no peer is live**.

**G3: recognise the ID as an ID.** The ID is not decoded at these levels (0 of 6), so recognition by
decode fails. Recognising it by structure is the same problem as G2b.

## Questions for the review

1. **Is G2 + G2b sound?** In particular: does any ladder rung's preamble detector confirm on hiss at
   the rate B's floor flickers (269 in 3 h)?
2. **Does a new peer's first frame always confirm its preamble** at the SNR where it would otherwise
   fail? If a frame below the preamble's acquisition limit gets silence (task 32's decision-4 regime),
   is that worse than today?
3. **What should "live" mean?** A decoded ladder frame, any decoded frame, or the daemon's own data
   transmission (the ISS role)? And over what `T_live`?
4. **Does the ride-along ID in front of an ACK** need handling here, or in the union guard (a head
   window extended by the longest ID in the mode)?
5. **Is the receiver the right layer?** The alternative is the sender: never key a sign-off or
   ride-along ID with a NACK-only transmission while idle. That removes source 2 only.

## Test plan (sketch; the review decides)

The station's own floor and the peer's sign-off ID must key nothing, while a new peer's first failed
frame still keys a NACK. All through `accumulate_capture` and `ota_decode_burst`, over the recorded
idle (`tests/idle_flicker_is_not_evidence.rs`):
- the recorded idle at a raised floor, as B's (RX source 1.0);
- B's ID at BPSK250 alone, and in front of an FSK4 NACK and of a union NACK;
- a new peer's first frame at each rung, at its decode edge, with no live peer.

Then a twin through `spawn_bridged_pair` with auto-ID on (`auto_id_signoff_idle_secs = 10`), as on
air. Expect no idle exchange once traffic stops. Sabotage each rule.

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
