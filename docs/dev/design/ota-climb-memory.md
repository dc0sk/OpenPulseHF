---
project: openpulsehf
doc: docs/dev/design/ota-climb-memory.md
status: draft
last_updated: 2026-11-07
---

# The receiver remembers a failed climb, and counts climb evidence by airtime

Work plan M3, A2 (stability, both directions). The maintainer chose both changes after the Fable
comparison with x0rro/xara of 2026-11-07 (`docs/dev/research/xara-comparison.md`, §4). This note
measures them first. **Not implemented; no production code changed.** The design needs the
mandatory review before it is built: it changes when and at what level a station transmits.

## Problem — measured on air

**1. The receiver re-proposes a rung that has just failed, and lands below the rung that worked.**
- On a link whose ceiling is SL9, the receiver climbs to SL10 after every 3 clean SL9 decodes
  (`ClimbOnEvidence`, `ACK_CLIMB_THRESHOLD`). It has no memory that SL10 failed.
- Each probe is three failed frames. The third steps the anchor (`NackStepDown`:
  `rx_confirmed = prev_mapped(rx_confirmed)`), so a failed SL10 probe lands on **SL8**, below the
  SL9 that kept decoding.
- **f2k** (task 31, B→A): B sent 30 of 106 frames at SL10, and all 30 were NACKed. There were 10
  cycles SL10 → SL8 → SL9 → SL10, and 33 frames went at SL8 under a proven SL9. f1c and f1d (A→B)
  are the same: 9 and 10 cycles, SL10 never decoded.
- **Each probe spends the whole NACK budget.** `OTA_NACK_BUDGET` = 3 = `nack_threshold`, so three
  SL10 NACKs leave the next failure muted until a decode. In **f2h** the 10-minute mute began with
  three SL10 NACKs (task 28 review, correction of 2026-11-07).
- **Against A2's stability clause:** in f2k the anchor demoted SL9 → SL8 ten times on a link where
  SL9 decoded throughout. Each was a `NackStepDown` after a decode failure, but the failures were at
  SL10.

**2. On the entry rung, a climb on evidence takes 3.5 minutes.**
- SL2 (BPSK31 + Rs) is one 66.56 s frame for any payload up to 209 B (measured below). Three clean
  decodes, plus the ACKs, take 3 × 70.0 s on air.
- At SL2 the SNR reads −16 to +6 dB (25 sessions), so `ClimbOnSnr` almost never fires there.

## Part 1 — the replay

### Apparatus

- **Real code, both ends.** A temporary `#[cfg(test)]` child module of `server.rs` drove two real
  `OtaRateController`s (receiver and sender), the real `NackBudget` (`nack_budget.rs`) and the real
  `run_ota_retry`. The attempt closure plays one frame:
  - the receiver's `on_rx_frame_with` with the budget's verdict;
  - the sender's `adopt_recommendation_from` for a keyed ACK or NACK;
  - and `fall_back_after_silence` for silence.
- **The two changes** were prototyped in `ota_rate.rs` behind a temporary switch. With the switch
  off, all 35 `ota_rate` unit tests but one passed (the variant sweep, which fails by design, see
  Gates). All probes and the switch were then reverted. They are not committed.
- **Inputs** are the receivers' `ota_rate_decision` events: A's in f2f, f2g, f2h, f2i, f2j and f2k
  (B→A), and B's in f1c and f1d (A→B). Each event gives the decoded level and SNR, or a failure at
  `from`.
  - B's log confirms `from` is the level sent: in f2k, `from=Sl10` NACKs number 30 on both stations.
  - The sequences were extracted with `python3 -I` over
    `docs/dev/test-reports/onair-twin-ota-*-evidence/events-{a,b}.ndjson.gz` on `m3/onair`.

**THE MODEL. A replay of a fixed outcome sequence cannot know what would have decoded at a rung, or
at a time, that was not tried. This is a model, not a measurement of the changes.**
- **Each rung has its own queue** of observed outcomes, in order: decoded with its SNR, or failed. A
  frame sent at a rung takes that rung's next outcome. When a queue runs out it starts again.
  - So SL10 always fails, because it never decoded.
  - SL9 decodes as observed, including the run's SL9 failures in their order.
  - A rung with no observation fails, heard.
- **f2f and f2g: SL7 and above are unheard.** A decoded no OFDM in those runs (task 27), so those
  frames give silent ACK windows.
- **A second model, as a check:** each frame draws at random from its rung's observed outcomes. The
  columns marked "i.i.d." are the mean over 200 seeds.
- **Time per attempt** is the median spacing of B's `OTA ACK received` lines in f2g–f2k, per rung and
  outcome:
  - decoded: SL2 69.96, SL3 38.50, SL4 24.88, SL5 10.91, SL6 6.75, SL7 3.45, SL8 2.98, SL9 2.77 s;
  - NACKed: SL7 4.46, SL8 3.97, SL9 3.79, SL10 4.10 s;
  - silent: 11.23 s at OFDM (89 windows), modelled as the frame's airtime + 9.7 s.
- **Traffic is saturated:** there is always a 64-byte message (the on-air size). Every rung from SL1
  to SL11 sends one code block per frame for payloads up to 209 B (measured below), so a 209-byte
  message has the same airtime. Goodput scales by 209/64 and the ratios between rules do not change.
- **Each run lasts 1500 s** from a cold start at SL2. Goodput is reported for the first 600 s, and
  for the 600 s after the sender first sends at SL7 (the ceiling phase).
- **Fidelity of "today" against the air, f2k:**
  - first SL7 send: 304 s in the model, about 302 s on air;
  - SL8 : SL9 : SL10 frames: 121 : 124 : 120 in the model, 33 : 31 : 30 on air;
  - step-down cycles per OFDM minute: 2.0 in the model, 1.8 on air;
  - f2h muted failures: 89 in the model, 79 on air.

**Frame airtime per rung** (temporary modem probe, `transmit_with_fec_mode` on the `fast` profile,
release). Every rung's frame has the same length for 1, 64 and 200-byte payloads:
- SL1 16.99, SL2 66.56, SL3 33.78, SL4 21.11, SL5 8.44, SL6 4.18 s;
- SL7 1.51, SL8 1.01, SL9 0.79, SL10 0.65, SL11 0.54 s;
- SL12–14 0.25 / 0.22 / 0.18 s (up to 118 B).
- The largest payload that keeps one frame is 209 B on SL1–SL11. **So OFDM frames are dominated by
  the turnaround:** SL9's cycle is 2.77 s for 0.79 s of airtime. A rung above the ceiling buys
  little; a NACK at it costs a whole cycle.

### The variants

- **today:** the shipped rules.
- **C1-first:** change 1 as designed below. A failed probe is withdrawn **on its first failure**.
- **C1-thresh:** Fable's literal sketch. The probe is withdrawn at the NACK threshold, so it still
  takes three failures.
- **C2-T60:** change 2 alone, T = 60 s.
- **both:** C1-first + C2-T60.
- Memory constants: first 4, cap 16 (Fable's), no airtime expiry. The constants are swept below.

### Results (in-order model; ceiling = the rung that never decodes)

| run | rule | frames SL8 / SL9 / ceiling | budget exhaustions | first SL7 send (s) | B/min, first 600 s | B/min, ceiling phase | ceiling phase, i.i.d. |
|---|---|---|---|---|---|---|---|
| f1c | today | 120 / 145 / 99 | 33 | 291 | 448 | 755 | 741 |
| | C1-first | 6 / 387 / 7 | 0 | 291 | 563 | **1011** | 1005 |
| | C1-thresh | 6 / 364 / 21 | 7 | 291 | 512 | 941 | 918 |
| | C2-T60 | 133 / 162 / 114 | 38 | 151 | 595 | 755 | 740 |
| | both | 6 / 433 / 8 | 0 | 151 | **787** | 1011 | 1007 |
| f1d | today | 126 / 126 / 117 | 39 | 291 | 448 | 787 | 775 |
| | C1-first | 3 / 415 / 9 | 0 | 291 | 698 | **1280** | 1278 |
| | C1-thresh | 3 / 389 / 27 | 9 | 291 | 653 | 1197 | 1186 |
| | C2-T60 | 140 / 142 / 129 | 43 | 151 | 614 | 787 | 774 |
| | both | 3 / 463 / 10 | 0 | 151 | **992** | 1280 | 1278 |
| f2i | today | 137 / 121 / 102 | 34 | 304 | 429 | 755 | 737 |
| | C1-first | 5 / 400 / 8 | 0 | 304 | 634 | **1190** | 1194 |
| | C1-thresh | 5 / 374 / 24 | 8 | 304 | 570 | 1088 | 1109 |
| | C2-T60 | 155 / 133 / 114 | 38 | 164 | 589 | 755 | 735 |
| | both | 5 / 447 / 9 | 0 | 164 | **909** | 1190 | 1191 |
| f2j | today | 102 / 223 / 42 | 29 | 234 | 467 | 666 | 673 |
| | C1-first | 262 / 130 / 4 | 9 | 234 | 666 | **954** | 933 |
| | C1-thresh | 51 / 276 / 27 | 23 | 234 | 403 | 582 | 642 |
| | C2-T60 | 106 / 238 / 45 | 31 | 164 | 518 | 666 | 673 |
| | both | 280 / 138 / 4 | 10 | 164 | **800** | 954 | 936 |
| f2k | today | 121 / 124 / 120 | 40 | 304 | 454 | 787 | 789 |
| | C1-first | 18 / 391 / 9 | 0 | 304 | 672 | **1267** | 1273 |
| | C1-thresh | 3 / 378 / 27 | 9 | 304 | 621 | 1184 | 1180 |
| | C2-T60 | 135 / 137 / 132 | 44 | 164 | 608 | 787 | 789 |
| | both | 18 / 439 / 10 | 0 | 164 | **954** | 1267 | 1273 |
| f2h | today | 108 / 24 / 15 | 7 (89 muted) | 304 | 154 | 96 | 488 |
| | C1-first | 74 / 95 / 5 | 7 (58 muted) | 304 | 390 | 333 | 614 |
| | both | 88 / 95 / 5 | 7 (72 muted) | 164 | 378 | 333 | 616 |
| f2f, f2g | today, C1 | SL7 unheard: 8 sends | 0 | 304 | 294 | 499 | — |
| | C2-T60, both | same | 0 | 164 | 390 | 499 | — |

**What this shows (as a model):**
- **Change 1, withdrawn on the first failure:**
  - ceiling-phase goodput rises **+34 % to +63 %** (f1c +34, f2j +43, f2i +58, f2k +61, f1d +63);
    the i.i.d. model agrees within 3 %;
  - SL10 probes in 1500 s fall from 99–120 to 7–10;
  - NACK-budget exhaustions fall from 29–40 to 0 (f2j: 9, because SL9 itself fails there);
  - `NackStepDown` falls from 33–40 to 0 in f1c, f1d, f2i and f2k;
  - frames at SL8 under a proven SL9 fall from about a third to 1–5 %.
  - Fable's arithmetic said +45 %. The model's median is +58 %. The gain is the NACK cycles and the
    SL8 detour removed, not SL9's rate: SL9's cycle is only 7 % shorter than SL8's.
- **f2j is the case where SL9 itself is marginal** (30 of 61 SL9 frames failed on air). Here C1-first
  learns to sit at SL8: 262 SL8 frames, and 15 `ProbeWithdrawn` that include SL9 probes from SL8.
  It still gains +43 %.
- **Fable's literal sketch (withdraw at the threshold) is worse than C1-first everywhere, and worse
  than today in f2j** (582 against 666 B/min in order; 642 against 673 i.i.d.).
  - Three probe NACKs still spend the budget: 8–23 exhaustions.
  - The withdrawn-to rung's next failure is then muted.
  - **Rejected on this evidence.**
- **Change 2 saves 140 s per cold start** (first SL7 send: 291–304 → 151–164 s). It does nothing at
  the ceiling. T = 120 s saves 70 s.
- **f2h is weak evidence.** Its SL8 failures were a receiver trap (AGC SLOW, bursts opened on hiss),
  not the channel, and the in-order queue replays the trap as if it belonged to the rung. The f2h rows
  show only that the trap is reached later and with fewer probes behind it. They are not an
  estimate of the gain.
- **f2f and f2g: change 1 does nothing,** as it should. The receiver hears no SL7 frame, so it sees
  no failure. The sender's fall-back and its `tx_climb_backoff` carry those runs.

### The twin harness

- A temporary test in `crates/openpulse-daemon/tests` ran two real daemons through
  `spawn_bridged_pair`, `fast`, floor SL7, 40 dB AWGN.
- **The forward channel time-reverses every frame while B's last decision recommends SL9 or
  higher,** so every SL9 frame fails while carrying a frame's energy (the `ReversingChannel` of
  `muted_failures_move_no_ladder`).
- 40 of B's decisions per run, release, 33 s each.
- At 40 dB **every climb is `ClimbOnSnr`**, so the backoff counts frames here, not 3-frame streaks
  (on air, at OFDM, every climb was on evidence).

| rule | SL9 probes in 40 frames | decoded at SL7 (below the proven SL8) |
|---|---|---|
| today | **24** (8 cycles SL9 ×3 → SL7) | 8 |
| C1-first | **4** (frames 3, 8, 17, 34: backoffs 4, 8, 16) | 1 (the start) |
| C1-thresh | 9 | 1 |

- **The target for the gate is ≤ 4 probes in 40 frames, and no decode below SL8 after the first climb
  to SL8.** Today fails it at 24 and 8.
- Both existing twin tests were also run with C1 on and off (release):
  - `climb_into_silence_falls_back`: 2 passed either way.
  - `muted_failures_move_no_ladder`: passed off. **With C1 on it failed:**
    `[(Keyed, ProbeWithdrawn, 8, 7), (Keyed, NackHold, 7, 7) ×2, (Muted, NackHold, 7, 7) ×4]` against
    its assertion of exactly one `NackStepDown`. Its first failure after the climb is a probe, so it
    is now withdrawn. The verdict sequence and "no silent step" both still hold. The test must count
    announced moves (`NackStepDown` + `ProbeWithdrawn` = 1), not one variant (see Gates).

### The HF caveat, measured on a synthetic ceiling

- **Synthetic link:** rungs below the ceiling c always decode, rungs above it always fail, and c
  fails until t_rec and decodes after.
- **Latency** is from t_rec to the first decode at c, the worst over 25 values of t_rec from 400 to
  1600 s.

| ceiling | today | C1, no expiry | C1, 120 s expiry | both, 120 s expiry |
|---|---|---|---|---|
| SL3 (base SL2) | 301 s | **1927 s** | 209 s | 176 s |
| SL7 (base SL6) | 53 s | 344 s | 202 s | 174 s |
| SL10 (base SL9) | 81 s | 141 s | 141 s | 144 s |

- Counted in streaks alone, the memory pins a slow-rung link for half an hour after its ceiling
  recovers: 16 streaks × 3 × 70 s at SL2. **So the memory needs an airtime expiry.** With 120 s of
  clean airtime, no rung is pinned more than about 3.5 minutes.
- **The cost of the expiry, on a ceiling that never recovers** (3000 s):

| ceiling | rule | ceiling frames | B/min |
|---|---|---|---|
| SL3 | today | 24, plus 22 frames on the MFSK16 sub-floor | 58 (inflated: it credits each SL1 frame a full 64-byte message; the sub-floor carries one small block) |
| | C1, no expiry | 2 | 53 |
| | C1, 120 s expiry | 12 | 46 |
| | both, 120 s expiry | 17 | 43 |
| | **C2 alone** | 36, plus **36 frames on SL1** | 62 (inflated as above) |
| SL7 | today | 117 | 313 |
| | C1, 120 s expiry | 16 | 489 |
| SL10 | today | 258 | 683 |
| | C1, 120 s expiry | 19 | 1159 |

- **Change 2 must not ship without change 1.** Alone, an early climb that fails takes three NACKs,
  and the third steps the anchor below SL2, onto the MFSK16 sub-floor: the "C2 alone" row.
- **Sensitivity of the memory constants** (both, in order; ceiling-phase B/min, f1d / f2i / f2j /
  f2k):

| first, cap | B/min |
|---|---|
| 2, 16 | 1274 / 1190 / 890 / 1254 |
| **4, 16** | 1280 / 1190 / 954 / 1267 |
| 8, 16 | 1280 / 1197 / 1075 / 1274 |
| 4, 8 | 1254 / 1178 / 954 / 1248 |
| 4, 32 | 1293 / 1197 / 954 / 1280 |

  - A 120 s expiry changes nothing at OFDM: 48 frames there are 38 s of airtime.
  - A 30 s expiry costs 1–2 %.
  - **The gain is not sensitive to the constants**, except f2j, which prefers a longer first wait.
  - The constants are chosen for the time bound, not fitted to these runs.

## Part 2 — the design

### Change 1: the receiver remembers a failed probe (`OtaRateController`)

**State.** `rx_climb_backoff: Option<RxClimbBackoff { rung, anchor, left: u8, n: u8, airtime_s: f32 }>`,
the receiver's mirror of `tx_climb_backoff`.

**The probe.** A probe is outstanding when `rx_recommended > rx_confirmed`. The lockstep allows at
most one mapped step, so the probe rung is `rx_recommended`.

**On `Failed`, in this order:**
1. Locked: unchanged (`Locked`).
2. **Muted:** unchanged. `NackHold` at `rx_recommended`, no memory, nothing withdrawn. #1460's rule
   holds: a failure nobody hears moves no ladder.
3. **FastDownshift:** unchanged. It is dormant on air, because a failure carries no SNR. It sets no
   memory: a downshift the SNR explains is not evidence about the probe rung.
4. **Probe outstanding (verdict Keyed or Leaked): `ProbeWithdrawn`** (new).
   - `rx_recommended = rx_confirmed`, and the anchor stays.
   - `rx_consecutive_nack = 0`.
   - `rx_climb_backoff = { rung: probe, anchor: rx_confirmed, left: n, n, airtime_s: 0 }`, with
     `n = min(2·n_prev, RX_CLIMB_BACKOFF_MAX)` when the memory already holds this rung, else
     `RX_CLIMB_BACKOFF_FIRST`.
   - ACK: `Nack`, recommending the confirmed rung.
5. Otherwise: today's NACK hysteresis, `NackHold`, and at the threshold or on a leak `NackStepDown`.
   **A step that takes the anchor below `backoff.anchor` clears the memory:** the channel changed
   under it, and the evidence is stale.

**On `Decoded(level)`:**
- The streak, airtime and `rx_confirmed` update as today, plus change 2's airtime.
- **Clear the memory** on a decode at or above its rung (the observation wins), or below its anchor.
  Otherwise add the frame's airtime to `backoff.airtime_s`. When that reaches
  `RX_CLIMB_BACKOFF_MAX_AIRTIME_S`, clear it (the HF bound).
- **Decline:** when a climb would fire (`snr_clears_ceiling || proven_by_success`, not at the top),
  `next_mapped(rx_confirmed) == backoff.rung` and `left > 0`:
  - `left -= 1`; the streak and its airtime are spent (`rx_consecutive_ok = 0`);
  - `rx_recommended = rx_confirmed`; decision **`ClimbDeclined`** (new), ACK `AckOk`.
  - A climb on SNR is declined as well, one `left` per frame, so a rising SNR wears the backoff down
    3× faster than evidence does. The decode failure is an observation; the SNR reading is a model.
- **`lock_level` and changed bounds clear the memory**, as they clear the sender's.

**The state machine (receiver, one rung pair):**

```
              3 decodes / SNR clears, no memory                      decode at or above the rung
 CONFIRMED(r) ─────────────────────────────────► PROBING(r+1) ───────────────────────────► CONFIRMED(r+1)
     ▲   ▲                                          │  keyed or leaked failure
     │   │ memory expires (left = 0,                │  ProbeWithdrawn, NACK(r), anchor stays r
     │   │ airtime ≥ 120 s), or a decode ≥ r+1      ▼
     │   └──────────────────────────────── BACKED-OFF(r, rung r+1, left = n)
     │                                         │  each would-be climb: ClimbDeclined, left − 1
     │                                         │  left = 0 → the next climb is a probe again (n doubles, cap 16)
     │ nack_threshold failures at r            │
     └── NackStepDown(r−1); memory cleared if the anchor falls below r
```

**Withdraw on the first failure, not at the threshold.** The replay rejects the threshold form.
What the first-failure form gives up:
- **HARQ on the probe rung.** Today the three SL10 attempts of one message can combine their LLRs
  (`OTA_HARQ_MAX_ATTEMPTS` = 3). On this pair that rescued none: 118 failed SL10 frames across f1c,
  f1d, f2h, f2i, f2j and f2k, and 0 SL10 decodes in any run. On a link whose frames are
  turnaround-bound, a rung that decodes only after 2–3 combined copies delivers less than the rung
  below it delivers in one, so a HARQ-only rung is not worth holding. On HF, with longer frames, this
  is UNCHECKED.
- **A transient failure at a working probe rung.** It costs one backoff (4 streaks) before the
  re-probe, and it doubles only if the re-probe fails too.
- **A failure at the confirmed rung while a probe is outstanding** (a lost AckUp, or a sender
  declining on its own `tx_climb_backoff`). It is read as a failed probe: it is withdrawn and the
  NACK count restarts. This delays a real demotion by at most one frame per outstanding probe, and
  sets a backoff the sender was already honouring.

### Change 2: climb evidence counted by airtime

- `proven_by_success = rx_consecutive_ok >= ACK_CLIMB_THRESHOLD`
  `|| (rx_consecutive_ok >= 1 && rx_ok_airtime_s >= CLIMB_EVIDENCE_AIRTIME_S)`.
- `rx_ok_airtime_s` is the decoded airtime in the current streak. It resets wherever
  `rx_consecutive_ok` does: on a failure, a level change, a climb, a decline, or a muted failure.
- **Where the airtime comes from:** a new entry point,
  `on_rx_frame_timed(outcome, snr, verdict, airtime_s: Option<f32>)`.
  - `on_rx_frame` and `on_rx_frame_with` pass `None`, which is today's rule exactly, so linksim and
    the unit tests do not change.
  - The engine (`engine.rs:5161`) passes `tx_airtime_seconds(payload, mode, fec)` for the decoded
    frame. That function is held to the emitted frame by `tx_airtime_matches_the_emitted_frame` in
    the default run.
- **Effect on the `fast` ladder:**
  - SL2: one 66.56 s frame is enough.
  - SL3: 2 frames (67.6 s), but SL3–SL5 climbed on SNR in 25 of 25 sessions.
  - SL1: 17 s frames, so the count of 3 binds first.
  - SL4 and up: the count binds first. In practice, **change 2 changes SL2 only.**

### Constants — inventory and falsifier

| constant | value | fitted to | what falsifies it |
|---|---|---|---|
| `RX_CLIMB_BACKOFF_FIRST` | 4 climb permissions | Not fitted. It mirrors `TX_CLIMB_BACKOFF_FIRST` (review S2). The sweep over 2, 4 and 8 on f1d, f2i, f2j and f2k moves the ceiling goodput ≤ 1 %, except f2j (890 / 954 / 1075). | On air, the re-probe after a first backoff decodes in most cases: the wait then costs more than the probe it saves. |
| `RX_CLIMB_BACKOFF_MAX` | 16 | Fable's cap. The sweep over 8, 16 and 32 differs ≤ 3 %. 16 bounds a fast-rung pin to 16 × 3 × 2.77 s ≈ 133 s at SL9 (measured: 141 s). | An HF run in which a rung recovers and `ClimbDeclined` runs on at it for minutes, with the rung decoding as soon as it is probed. |
| `RX_CLIMB_BACKOFF_MAX_AIRTIME_S` | 120 s clean airtime at the anchor | The synthetic recovery table (latency ≤ 209 s at every rung; without it 1927 s at SL3). There is **no on-air inventory**: the 2 m ceiling never moved within a run (UNCHECKED on HF). The cost: a never-recovering SL3 ceiling, 53 → 46 B/min. | HF: a withdrawn rung that is still failing when its memory expires, in most expiries. Then the expiry is too short for HF's fade timescale; lengthen it. |
| `CLIMB_EVIDENCE_AIRTIME_S` | 60 s, with ≥ 1 decode | See below. | See below. |

**`CLIMB_EVIDENCE_AIRTIME_S` = 60 s.** It is just under one SL2 frame (66.56 s), so it means "one
SL2 frame". If the SL2 frame length changes, this constant must be re-read.

**The evidence for it** (35 on-air sessions whose first frame decoded at SL2, all evidence bundles in
`m3/onair`):
- **25 sessions: the second SL2 frame decoded too.** Their first decode read −16.3 to +3.7 dB (median
  −8.9). The SL3 frame after the climb decoded first time in 24 of 25. The one failure,
  20261103-065320 B, read −9.3, −7.3 and +0.5 dB at SL2, then NACK, then decoded.
- **9 sessions: the first SL2 frame decoded and the next failed** (2026-10-31 to 2026-11-02 10:58,
  builds before the capture-gap and station-ID fixes of task 17). Their first decode read −9.7 to
  +0.4 dB (median −7.1).
  - T = 60 s would have climbed on the lucky decode in all 9; today's rule and T = 120 s in none.
  - These were receive-chain defects since fixed, not a marginal rung. But they are the only on-air
    instances of "decoded once, then failed at the same rung".
- **The SNR does not separate the two groups** (medians −8.9 and −7.1, ranges overlapping). No SNR
  gate can make the one-frame climb safe.
- With change 1, a wrong one-frame climb costs one SL3 frame (about 39 s with its NACK) and one
  keyed NACK, then withdraws to SL2. The gain is 70 s against T = 120 s, or 140 s against today. The
  break-even fraction of wrong climbs is about 64 %, with the time alone.

**Falsified if,** on current builds, more than one cold session in three shows `ProbeWithdrawn` at
SL3 right after a one-frame SL2 climb. Each wrong climb also spends a NACK the budget may need, so
the threshold is set well below the time-only break-even. Then go to T = 120 s, which is two
frames.

### Interactions

- **The NACK budget (#1456).**
  - A probe now costs one keyed NACK, not three. The next decode at the confirmed rung resets the
    budget.
  - The replay: exhaustions 29–40 → 0–10 per 1500 s. f2h's mute (three SL10 NACKs, then decodable
    SL8 frames muted for 10 min) would begin with one NACK spent and two left.
  - The budget constants are unchanged.
- **The sender's `tx_climb_backoff` and fall-back after silence.** These are independent mechanisms:
  - The receiver's memory is set only by a heard failure. The sender's is set only by
    `fall_back_after_silence`.
  - In f2f and f2g (SL7 unheard) only the sender's acts; with a NACKed ceiling, only the receiver's.
  - A `ProbeWithdrawn` NACK recommends a lower level, so at the sender it is a step down, which
    clears `tx_before_climb` and `tx_climb_backoff`, as today's `NackStepDown` does.
  - The sender's backoff has the same HF caveat, unbounded in time (cap 32: 32 × 3 × 6.75 s ≈ 11 min
    at SL6). It is left as it is, and noted for the review.
- **The lockstep.**
  - `rx_recommended` is never above `next_mapped(rx_confirmed)`. `ProbeWithdrawn` and
    `ClimbDeclined` set it equal to the confirmed rung, so the candidates shrink to {confirmed}.
  - **A lost withdrawal NACK:** the sender stays at the probe rung, its next frame fails (not a
    candidate), and that NACK again recommends the confirmed rung. The anchor steps only after
    `nack_threshold` such failures, the same exposure as today.
  - `never_desyncs_under_arbitrary_ack_loss` passed with the prototype on.
- **HARQ:** see the first-failure section above.
- **`muted_failures_move_no_ladder`:** a muted probe failure withdraws nothing. The twin test's
  "exactly one `NackStepDown`" becomes "exactly one announced move". Measured: with C1 on it fails
  as written.
- **A2 scoring and the `OtaRateDecision` consumers.**
  - `ProbeWithdrawn` is on the failure path (`is_failure_path`). The anchor does not move, so for
    A2 clause 3 it is **not a demotion**.
  - `ClimbDeclined` is a hold.
  - Clause 2 needs `NackStepDown` or `FastDownshift`. A ceiling probe no longer supplies one, so
    demotion is shown, as now, by induced level changes.
  - Consumers (found with the grep under Consumer):
    - `openpulse-panel` `connection.rs:213` prints the variant by its `Debug` name, so new variants
      show with no change.
    - `openpulse-tui` `app.rs:98` reads only `from`/`to`: a withdrawal shows as a level change, a
      decline does not.
    - `openpulse-twinview` reads no decision.
    - `ladder_evidence_probe.rs` and the daemon twin tests match `NackStepDown`; they need review.
    - `scripts/run-onair-twin-ota.sh` only names the event in a comment.
  - The JSON names are `probe_withdrawn` and `climb_declined`. Panel and daemon ship together. An
    older panel against a newer daemon fails to parse that one event.
- **Wire format: none.**
  - The ACK carries `recommended_level` as an absolute level (`ack.rs` byte 3, bits 7:3), and both
    new decisions use existing ACK types (`Nack`, `AckOk`) with a level the sender already handles.
  - `RateDecision` crosses only the control protocol (JSON), not RF.
  - `WIRE_VERSION` and the handshake are untouched.

### Gates and sabotages (for the implementation)

**Controller unit tests (`openpulse-core`), each failing on today's code or under the named
sabotage:**
1. `a_failed_probe_withdraws_to_the_confirmed_rung_and_keeps_the_anchor`. Today gives `NackHold`,
   then `NackStepDown` to SL8.
2. `a_withdrawn_rung_is_declined_then_reprobed_and_the_wait_doubles_to_16`. Sabotage: no doubling.
3. `a_decode_at_or_above_the_rung_clears_the_memory` and `a_step_below_the_anchor_clears_it`.
4. `the_memory_expires_after_120_s_of_clean_airtime`. Sabotage: the expiry removed.
5. `a_muted_probe_failure_withdraws_nothing`. Sabotage: the withdrawal placed before the muted
   return.
6. `a_lock_or_new_bounds_drop_the_climb_memory`.
7. `one_sl2_frame_of_airtime_proves_the_rung`, `airtime_without_a_decode_proves_nothing`,
   `a_failure_resets_the_airtime` and `no_airtime_is_todays_rule`.
8. `never_desyncs_under_arbitrary_ack_loss`, extended to random outcomes with the memory and airtime
   on.
9. **`every_branch_reports_the_decision_that_actually_ran` must reach the two new variants.**
   Measured: with them added and unreachable, it fails ("ClimbDeclined is never exercised").
10. **Re-specify, not delete, the five tests that encode today's rule for a failure while a probe is
    outstanding.** With the prototype on, these failed:
    - `a_failed_decode_without_an_snr_reading_does_not_crash_the_ladder`;
    - `a_leaked_nack_steps_on_its_own`;
    - `muted_failures_move_nothing_but_the_climb_streak`;
    - `transient_failure_at_good_snr_keeps_the_nack_hysteresis`;
    - the variant sweep.

    Each starts from a climbed state (`rx_recommended > rx_confirmed`). Start them from a settled
    state, and add the probe case beside them.

**Daemon twin tests:**
11. **New `a_ceiling_rung_is_probed_with_a_backoff`:** the reversing channel at SL9, as in the probe
    above. Assert ≤ 4 SL9 probes in 40 frames, and no decode below SL8 after the first climb to SL8.
    Sabotage: memory off gives 24 and 8.
12. **Amend `muted_failures_move_no_ladder`** to count announced moves.

**Engine:**
13. **`the_decoded_frames_airtime_reaches_the_controller`**, through `ota_decode_burst`: one decoded
    SL2 frame answers `AckUp` / `ClimbOnEvidence`. Sabotage: pass `None`.

**Unchanged with the prototype on (release):**
- the `openpulse-core` integration tests (24 binaries);
- eight `openpulse-modem` OTA suites: `bidir_rate_adaptation`, `ota_rate_decision_events`,
  `ota_rate_lockstep`, `ota_channel_adaptation`, `ota_harq_combining`, `snr_rate_adaptation`,
  `symbol_snr_ladder_climb`, `snr_climb_at_production_alignment`;
- `openpulse-linksim` (19 + 3 tests).
- These passes do not show that a withdrawal ran in them. The switch was shown active only in the
  core lib run (5 failures) and the daemon twin run (1 failure).

**Before merge:** `scripts/gate.sh`, and `scripts/slow-tests.sh ota`, because CAP-33 drives the OTA
rate controller.

### Order and Release 1

- Change 1 first. Change 2 only with change 1 in place (the "C2 alone" row).
- **Does this block Release 1? No.** A2's three clauses are already shown both ways. This removes a
  sawtooth that the stability clause could be read against: ten anchor demotions on a stable SL9 in
  f2k. It also removes a third of the ceiling airtime spent on NACKs.
- Implementing change 1 is in scope for M3 as an A2 stability fix, on the maintainer's go. Change 2
  is a cold-start speed-up and can follow.

### Parked (work plan rows, post-release)

- the sender's ACK request on silence (a wire change);
- the PTT stall dead-man;
- learned per-rung ACK windows;
- the multi-level climb.

Each is cited to `docs/dev/research/xara-comparison.md` and the Fable comparison of 2026-11-07.

## What this could not establish

- **The gains are a model's.** No run has had either change on air. The in-order and i.i.d. models
  agree, but both assume that the rung's outcome does not depend on what was sent before it.
- **HF:** the expiry constant has no on-air inventory. 2 m ceilings did not move within a run.
- **Whether HARQ would rescue a probe rung on HF:** UNCHECKED.
- **Whether an SNR-cleared climb should wear the backoff down per frame:** chosen, not measured on
  air. On air no OFDM climb was on SNR.
- **The PTT stall bounds** quoted in the park row (the flush timeout, about 14 s at SL7 and 87 s at
  SL2) were given with the task, not measured here.

## Consumer

- **The production feed:** `ModemEngine`'s OTA decode (`engine.rs:5161`, `on_rx_frame` for a decode;
  `engine.rs:5165`, `on_rx_frame_with` for a failure) and its `EngineEvent::OtaRateDecision` at
  `engine.rs:5173`.
- **The other caller:** `apps/openpulse-linksim/src/lib.rs:968`. It gets change 1 automatically, and
  change 2 only if it passes an airtime.
- Found with `grep -rn "on_rx_frame_with\|on_rx_frame(" --include=*.rs crates apps plugins` (the
  same filter matched the known call sites in `tests/success_based_climb.rs`).
- **Decision consumers:** `apps/openpulse-panel/src/connection.rs`,
  `crates/openpulse-tui/src/app.rs`, `crates/openpulse-daemon/src/ladder_evidence_probe.rs`, the
  daemon tests `muted_failures_move_no_ladder`, `climb_into_silence_falls_back`,
  `ofdm_detect_twin`, and `scripts/run-onair-twin-ota.sh` (comment only).
  - Found with `grep -Rln "OtaRateDecision\|ota_rate_decision" --include=*.rs --include=*.py --include=*.sh .`
  - `grep -rn "decision\|rate" apps/openpulse-twinview/src` found no decision reader.

## Prior art

- **`docs/dev/design/ladder-climb-into-silence.md`, review S2:** the sender's `tx_climb_backoff`
  (4, doubling to 32). This is its receiver-side mirror.
- **xara** (`docs/dev/research/xara-comparison.md` §4): the receiver's climb margin grows after a
  failed climb and decays per good frame (`adapt.rs:83-131`). That is the same lesson, applied to a
  margin rather than a count.
- **`docs/dev/design/failed-burst-ownership.md` (#1460 round 4b):** a muted failure moves no ladder.
  The memory keeps that rule.
- **`docs/dev/design/nack-streak-decay.md`** (parked) and `nack_budget.rs` (#1456): the budget this
  stops spending.
- **#934:** the evidence climb, which change 2 widens.
- **`reply-window-evidence.md`:** the source of `tx_airtime_seconds`.
- Found with `grep -Rli "climb" docs/dev/design docs/dev/reviews` and
  `grep -Rn "airtime" docs/dev/design/*.md`.

## Twins

- **`apps/openpulse-linksim`** drives the same controller (`lib.rs:968`), so it gets change 1. Its
  tests passed with the prototype on.
- **`transmit_arq_ota`** (feature `instruments`) receives through the same engine path.
- **ARDOP and KISS** do not run the OTA ladder (`ladder-climb-into-silence.md`, Twins).
- **The sender's twin of this memory is `tx_climb_backoff`.** It is left unchanged, and its missing
  time bound is noted under Interactions.
- Found with the Consumer grep, and with
  `grep -rn "OtaRateController::new" --include=*.rs crates apps plugins`: outside tests, the only
  constructors are `engine.rs:2293` and `apps/openpulse-linksim/src/lib.rs:754`.
