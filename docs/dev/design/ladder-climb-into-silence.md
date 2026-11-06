---
project: openpulsehf
doc: docs/dev/design/ladder-climb-into-silence.md
status: resolved
last_updated: 2026-11-05
---

# A climb into silence: the sender's way back (P3), and SL7's peak (P1)

Work plan M3, A2 (stability, both directions). Found on air in task 23, run f2c (B sends, `fast`
profile). Proposals P1 and P3 of the task 23 review in `docs/dev/onair-tasks.md`. The maintainer's go:
2026-11-05.

## Problem — measured

**On air (task 23, f2c):**
- **The climb:** A, the receiver, climbed on evidence SL7 → SL8 at 07:57, after reading B's SL7 at
  6.0 dB.
- **What A heard after that:** A's receive audio (`rx-a-f2c`) holds noise only, 0.0052 RMS, for
  20 minutes. The only exceptions are B's two `with-data` BPSK250 station IDs (0.010 RMS). Each ID
  rides in the same stream as an SL8 frame, and the frame is not in the audio. B's PTT watcher shows
  B keyed every ~11 s through that time.
- **The cause is the transmit level per rung** (through `transmit_with_fec_mode`, 64-byte payload):

  | rung | RMS (dBFS) | peak |
  |---|---|---|
  | SL2–SL6 | −4.3 | 1.00 |
  | SL7 OFDM52 | −0.6 | 2.82 |
  | SL8–SL14 | −12.9 to −14.8 | 0.90 |

  - The FT-991A shows no ALC compression (its meter reads 0.0625 throughout), so its RF follows the
    audio. B's SL8 went out ~13 dB under its SL7.
- **What followed:**
  - A heard nothing, so it keyed no NACK and made no decision (19 decisions in 33 minutes, the last at
    07:57).
  - B kept A's last recommendation, SL8, and abandoned 51 messages, each after 2 silent ACK windows
    (`MAX_SILENT_ACK_WINDOWS`).
  - **The link never recovered.**

**Why nothing recovers it today:**
- The rate follows the receiver (`adopt_recommendation`).
- A receiver that hears nothing says nothing.
- The sender's only signal is silence, and silence moves nothing (`dcd-raised-floor.md`, decision 3,
  parked).

## Decision P3 — implemented: the sender returns to the level before an unconfirmed climb

Review: `docs/dev/reviews/review-ladder-climb-into-silence-2026-11-05.md` (mandatory: it changes when
and at what level a station transmits). Its must-fixes and the backoff are taken.

**The rule** (`OtaRateController`, `crates/openpulse-core/src/ota_rate.rs`):
- **`adopt_recommendation_from(level, nack)`** records, when an ACK raises the transmit level, the level
  the sender was on (`tx_before_climb`).
  - **A decode-path ACK** (not a NACK) that does not raise it clears the record. The sender sends only
    at its level, so such an ACK answers a frame sent at the new one.
  - **A NACK keeps the record** (review M1). It is evidence against the rung, and one NACK followed by
    silence would otherwise strand the sender as in f2c.
  - A step down clears it: the receiver led that step.
- **`fall_back_after_silence()`**, asked by `run_ota_retry` when `MAX_SILENT_ACK_WINDOWS` windows in a
  row stay silent:
  - it sets the level to the record, clears it, and returns true;
  - the send continues at that level within the same budget.
  - Without a record nothing changes, and the send is abandoned as before.
- **Never onto the MFSK16 sub-floor** (review M2). A body over its one-block capacity cannot ride it,
  so the send would key with nothing radiated, and the receiver, never told SL1, would hear nothing:
  decision 3's permanent stall.
- **A backoff for the re-probe** (review S2): after a fallback from a rung, the sender declines the
  next 4 climbs back to it (`TX_CLIMB_BACKOFF_FIRST`), doubling per repeat up to 32
  (`TX_CLIMB_BACKOFF_MAX`).
  - A decode-path ACK at the rung, or a step down, forgets it.
  - It is safe for the lockstep: a declined climb leaves the sender on the receiver's confirmed rung,
    which is always among the receiver's candidates.
- **Locked sessions never fall back,** and a lock or new bounds drop the record and the backoff.
- **Observability** (review S4): an `info` line, and its own event, `EngineEvent::OtaFallBack { from,
  to }`. A2 scoring must tell a sender-side change apart from a receiver's step-down.

**The bounds:**
- One step, never below the level the climb started from, at most once per climb.
- The loop's budget is unchanged (`MAX_RETRIES` + 1 attempts). After an unconfirmed climb a message
  can see 4 silent windows instead of 2 (the `MAX_SILENT_ACK_WINDOWS` comment says so).

**Lockstep, corrected by the review (S1):**
- **When the receiver has not decoded at the new rung** (f2c), its confirmed rung is still the old
  one and among its candidates. It decodes the fallen-back frame and recommends that rung again.
- **When it has decoded there** and two ACKs were lost, its confirmed rung is the new one. The old rung
  is no longer a candidate: the fallen-back frame fails, and recovery is by its NACK, which carries
  the new rung back. The cost is one failed frame, and it depends on that NACK being keyed.
- **A dead ACK path** (a2g: frames decode, ACKs don't arrive) is a dead ARQ link either way. With P3 it
  settles one rung lower than before, and the message is lost after its attempts.

**What it leaves:**
- With the backoff, a failed probe of the same rung costs 2 silent windows after 4, 8, 16, then
  32 declined climbs. The receiver re-proposes every `ACK_CLIMB_THRESHOLD` (3) decodes.
- The receiver learns nothing: it cannot tell a sender that fell back from an ACK-up that was lost.
- A station ID heard with no ladder frame (f2c: A decoded B's IDs and nothing else) would be a
  decode-less reason for the receiver to step back. Parked.

## Decision P1 — implemented: the QPSK OFDM frame's peak is normalised like the others

**The change:**
- `modulate_with_params` scales every OFDM mode's frame to a 0.9 peak when its peak is above that.
  It did this only for the higher-order modes; QPSK (OFDM16, OFDM52) left `clip_iterative`'s output
  at a peak of 2.82.
- CE-SSB, applied after on SL7, restores the frame's own peak, which is then 0.9.

**What it changes on air** (the review measured it through `transmit_with_fec_mode`):
- SL7 falls from −0.6 to **−10.5 dBFS** (RMS 0.930 → 0.297).
- The SL6 → SL7 step becomes −6.2 dB, and SL7 → SL8 −3.1 to −4.1 dB.
- **On the stations it was not clipped.** cpal writes f32, PipeWire applies the sink volume (0.05–0.15)
  in float, and nothing in `openpulse-audio` or the daemon clamps. So SL7's +9.9 dB was real RF, which
  is why B→A's SL7 decoded at all in f2c.
- **Expected on air:**
  - B→A (no ALC compression) loses SL7. f2c's 6.0 dB becomes about −4 dB.
  - So the `fast` profile B→A tops at SL6, with SL7 probes answered by P3's fallback and its backoff.
  - The next f2 run is to be read that way, not as a regression.

**Why:** a sample above full scale is outside the audio contract, and it clips wherever a path converts
float to integer before the volume.

## P2(a) — the per-rung level prediction: not implemented in this round

A static table (the next rung's predicted SNR = the current reading + the level difference) would
block climbs that work, and the review found a second reason (S6).

**The estimators are not on one scale:**
- In f2c, on the FT-991A, SL6 read 9.6 dB and SL7 6.0 dB while SL7's audio was 3.7 dB louder: a 7.3 dB
  gap that no ALC explains.
- Task 22 found SL7 reading about 6–7 dB at a pilot SNR of 12 dB.
- So f1c's "SL6 11.1 → SL8 12.6" is not evidence of ALC compression either. It is the OFDM estimator
  reading on a different scale from the PSK one.

**What P2(a) needs first:**
1. a per-mode calibration of the estimators against a common SNR;
2. the level difference as measured on the link, not as transmitted.

That is a design for the next round.

## Tests

- **`ota_rate` unit tests** (`openpulse-core`):
  - `a_climb_answered_by_silence_returns_once_to_the_level_before_it`;
  - `an_answer_at_the_new_level_clears_the_way_back`;
  - `a_nack_after_the_climb_keeps_the_way_back` (M1);
  - `never_falls_back_onto_the_mfsk16_sub_floor` (M2);
  - `a_climb_back_to_a_rung_that_went_silent_waits_and_the_wait_doubles` (S2);
  - `a_lock_or_new_bounds_drop_the_way_back`;
  - `no_climb_or_a_lock_means_no_fall_back`.
  - **Sabotage:** each of M1, M2 and S2 removed fails its own test (33 passed, 1 failed each time).
- **`run_ota_retry` unit tests** (`openpulse-daemon`):
  - `silence_after_a_climb_continues_at_the_level_before_it_within_the_budget`;
  - `a_fall_back_on_the_last_attempt_moves_the_level_but_sends_nothing_more`.
- **`climb_into_silence_falls_back`** (`openpulse-daemon`): two daemons over the twin bridge.
  - The forward channel delivers exactly what it makes of silence from the moment A's level reaches
    SL8 until A leaves it.
  - A falls back to SL7, and B decodes there.
  - **Sabotage:** `false && fall_back_after_silence()` → timeout at 400 s, fails.
  - Two invalid sabotages found on the way are recorded in the review record.
- **P1:** `every_ofdm_frame_peaks_within_full_scale` (`ofdm-plugin`). The OFDM, CE-SSB and level
  suites pass unchanged (the review ran ten of them with P1 applied).

## Consumer

- **P3:** `server::ota_send_with_ptt` → `run_ota_retry` (the daemon's send path, every OTA data
  frame); `ModemEngine::apply_ota_ack` → `OtaRateController::adopt_recommendation`.
- **P1:** `ModulationPlugin::modulate` for OFDM16 and OFDM52 (SL7 on `fast`).
- Found with `grep -n "run_ota_retry\|apply_ota_ack" crates/openpulse-daemon/src/server.rs` and
  `grep -n "bits_per_sc != 2" plugins/ofdm/src/modulate.rs`.
- **Missed, found on air (task 26):** the senders of the bounds command, which drops the record. Before
  `306cad29` the CLI's `ota-status` was such a command, polled by the on-air runner every pass, so the
  record never survived to a fall-back. Senders, from the `ControlCommand` enum
  (`OtaSetLevelBounds`, `OtaLockLevel`, `OtaUnlock`): the CLI's `ota-bounds` / `ota-lock` /
  `ota-unlock` and the panel's lock toggle, all on user action only. `ota-status` now sends
  `GetOtaStatus`.

## Prior art

- **`docs/dev/design/dcd-raised-floor.md`, decision 3:** silence is not rung evidence. Its review allowed
  only "option (b), bounded to the receiver's previous recommendation on an ACK-up"; this is that
  option. It asked for a decode-less reason, a NACK heard; f2c shows a link where none is ever heard.
- **#917:** `MAX_SILENT_ACK_WINDOWS`, the silent-peer bound.
- **`docs/dev/design/ofdm-padding-whitening.md`:** the last change to the OFDM frame's level.

## Twins

- **`transmit_arq_ota`** (feature `instruments`) has its own retry loop and does not get P3. No Release 1
  on-air path uses it: its only callers are in `engine.rs` and `tests/ota_rate_lockstep.rs` (the
  review's grep over `crates/`, `plugins/` and `apps/`).
- **ARDOP and KISS** do not run the OTA ladder.
