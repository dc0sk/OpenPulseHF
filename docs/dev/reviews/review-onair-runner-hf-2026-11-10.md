---
project: openpulsehf
doc: docs/dev/reviews/review-onair-runner-hf-2026-11-10.md
status: resolved
last_updated: 2026-11-10
---

# Review: the on-air runner on HF (2026-11-10)

- **Subject:** `docs/dev/design/onair-runner-hf.md` (draft), on branch `design/runner-hf`.
- **This is the mandatory review** (keying class: where and how the rigs transmit).
- **Reviewer:** Fable, read-only. It checked against the host's Hamlib 4.6.5 (`rigctl -m N -u`).

## Prompt

The reviewer was asked to falsify seven things:
1. the edge model (dial + 1 500 ± width / 2 on USB), across every waveform, the dial offsets, the AFC,
   and the rig's TX filter;
2. the AGC check and Hamlib's values per backend;
3. the passband read-back;
4. power on 100 W rigs and the lower-only check;
5. the SWR stop;
6. whether `check` is keyless;
7. what `run-onair-tx500-kx3.sh` lacks.

It was also asked: **"Does this block Release 1?"**

## Verdict

**APPROVE WITH CHANGES.** Nothing blocks the RC or the 2 m work; the changes block the HF run.

**Framing errors:**
- **FAST = 2** on the K4, the IC-7300 and the IC-9700 (Hamlib's enum). The IC-7300 also reads
  `AGC_TIME`.
- **Flipping the IC-9700 profile to FAST would reverse a measured maintainer decision** (task 3). Leave
  it, and put the conflict to the maintainer.
- **Both backends name `PKTUSB`.** Plain USB on the IC-7300 keys the microphone.
- **The 20 m example overlaps the IBP beacons** (14.099–14.101). It must start at 14.101, and
  `regulatory.md` is the misleading source.

**Findings:**
1. **The widths hold at the waveform level** (OFDM52 2 031.25 Hz centred on 1 500; MFSK16, the ID and
   QPSK250-D 500; FSK4-ACK 300), and nothing moves at runtime.
   - **But the emission is not the waveform.** The OFDM clip and CESSB have no post-filter, so the rig's
     TX filter bounds the emission.
   - **The dial offset is not one thing:** the FT-991A's +1 564 = a 64 Hz trim + a 1 500 Hz display
     shift.
   - **The dial read back is the RX VFO:** read split.
2. **AGC:** refuse on a mismatch, with an `any` opt-out. An unsupported reading fails closed, with an
   "unreadable" message.
3. **The passband is meaningful on both HF rigs.** But the FT-818 in `keep` reads 0, so the design's own
   2 m check would fail. Width is not coverage: PBT can shift a 2 400 Hz filter off 1 500.
4. **Power:** both backends map RFPOWER 0.05–1.00 to 0–100 W. `rig_power_lower_only` is correct and has
   no raise path. The K4 refuses values under 0.05.
5. **The SWR stop: drop it.** The K4 has no SWR level, and the IC-7300 reads 0 / 1.0 in RX. "At the start
   of the first keying" is not implementable, and polling during TX toggled the FT-818's VFOs.
6. **`check`:** setting F, M and RFPOWER on a receiving rig emits nothing. rigctld opening an RTS/DTR PTT
   port can key, so `check` is keyless only for CAT.
7. **`run-onair-tx500-kx3.sh` lacks** a band guard, read-back failures, lower-only power, AGC, passband
   and split checks, PTT-off at cleanup, a watchdog, the build check, and an evidence read-back. It is
   not an HF runner for Release 1.

**The changes:**
1. edges from the rig's TX passband (or a ≥ 300 Hz guard);
2. a per-station display shift;
3. a split read;
4. a per-station passband expectation (`any` for the FT-818), failing closed otherwise;
5. `check` names the PTT type;
6. `EXPECT_AGC` defaults to 2, with the IC-9700 profile unchanged pending the maintainer;
7. the SWR stop dropped, and SWR read once as evidence;
8. the example segment starts at 14.101;
9. plain USB refused on an Icom.

Each goes into `band-self-test`, with sabotage.

## Disposition

All nine are applied (see the design's "As built"). `docs/regulatory.md`'s 20 m row and its example are
corrected in place. The AGC conflict on the IC-9700 is put to the maintainer, not decided here. PBT, the
IC-7300's `AGC_TIME`, and the K4's TX filter are left as UNCHECKED items in the HF profile, to be read
at the first `check`.

## Consumer

`scripts/run-onair-twin-ota.sh` and its profiles in `docs/config/`.

## Prior art

The dial guard, `onair-station-check.sh`, `rig-power.sh`, and the watchdog's self-test.

## Twins

`run-onair-tx500-kx3.sh` (not an HF runner for Release 1), `run-onair-ic9700-ft991a.sh`, and
`onair-station-check.sh`.
