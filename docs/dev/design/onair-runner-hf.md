---
project: openpulsehf
doc: docs/dev/design/onair-runner-hf.md
status: draft
last_updated: 2026-11-10
---

# The on-air runner on HF: the signal's edges inside the segment, the AGC and passband read back, and profiles for the HF pair

Work plan M4, the HF run with the release candidate (`onair-hf-plan.md`, "Runner"). It changes where
and how the rigs transmit, so it is reviewed before it is built. Review: pending.

## Problem

`scripts/run-onair-twin-ota.sh` was built for the 2 m pair, and some of its checks only hold there.
- **The band guard checks the dial frequency, not the signal.**
  - It refuses a dial outside `BAND_MIN_HZ`–`BAND_MAX_HZ`; on 2 m that was 144.500–144.750 MHz.
  - **On HF the data segments are narrow:** for example 14.099–14.112 MHz (13 kHz), or 7.040–7.060.
  - A `fast` signal (up to about 2 031 Hz occupied) sits at the dial + the audio centre (1 500 Hz,
    `plugin.rs`) ± half its width, so USB puts it at about the dial + 485 … 2 515 Hz.
  - A dial set 1 kHz below the segment's top edge passes today's guard, while the signal crosses the
    edge.
  - **The software checks no band edge on transmit** (`docs/regulatory.md`; the bandplan applies to QSY
    only), so the runner's guard is the only check.
- **The rig's AGC is neither set nor read back.** AGC FAST on the receiving rig is a Release 1 operating
  requirement (`dcd-raised-floor.md`). The one place a profile names an expected AGC
  (`onair-ic9700-ft818.example.sh`, `A_EXPECT_AGC=3`) expects **SLOW**, and only
  `onair-station-check.sh` reads it.
- **The passband is sent with the mode** (`M <mode> RIG_PASSBAND_HZ`, default 3 000) **but never read
  back.** A filter narrower than about 2.4 kHz decodes nothing (#1060).
- **There are no profiles for the HF pair.** That pair is decision 28: an Elecraft K4 (Hamlib 2047) and
  an IC-7300MK2 (Hamlib 3073, where the MK2 is UNCHECKED).

## Design

1. **The signal's edges inside the segment** (preflight, before anything keys).
   - For each station, the run computes the signal's lowest and highest RF frequency: the station's
     dial + `AUDIO_CENTER_HZ` (1 500) ± `OCCUPIED_BW_HZ` / 2. It refuses if either edge is outside
     `BAND_MIN_HZ`–`BAND_MAX_HZ`. The refusal names the station, the edge, and the overrun in Hz.
   - **`OCCUPIED_BW_HZ` is taken from the profile:** `fast` 2 100 and `robust` 600. Those are the
     regulatory doc's ≈ 2 031 and ≤ 500, rounded up. It can be overridden only to a *larger* value.
   - The sideband is `USB`, `PKTUSB` or `USB-D`: the signal sits above the dial. An LSB-family mode is
     refused on HF: the `hpx_hf` audio is defined for USB.
   - **The 2 m defaults stay.** An HF run must set `TEST_FREQ_HZ`, `BAND_MIN_HZ` and `BAND_MAX_HZ`
     explicitly. With the 2 m guard left at its default, an HF dial is refused, as today.
2. **AGC read back, both stations** (in `tune`, before any keying, and after the run with the
   evidence).
   - `rigctl l AGC` is read and compared with `A_EXPECT_AGC` / `B_EXPECT_AGC`. **The default is FAST**,
     in Hamlib's AGC value for each rig, which differs per backend and is UNCHECKED for the K4 and the
     IC-7300MK2. The profile states it.
   - A mismatch refuses the run. **Only an explicit `EXPECT_AGC=any` skips the check**, and the report
     then says so.
   - The runner does **not** set the AGC. Setting it is the operator's (it is a per-band, per-mode
     setting on both rigs). The runner only refuses to key on the wrong one.
   - The FT-818 profile's `A_EXPECT_AGC=3` (SLOW) becomes FAST, to match the Release 1 requirement.
3. **The passband read back.** After the mode is set, `rigctl m`'s second line (the passband) is read.
   - A passband under `MIN_PASSBAND_HZ` (2 400) refuses the run.
   - A `keep` station (its mode set on the rig) gets the same check.
   - `A_PASSBAND_HZ` / `B_PASSBAND_HZ` set it per station, defaulting to `RIG_PASSBAND_HZ`.
4. **Power, unchanged in rule, and stated for 100 W rigs.**
   - `*_RFPOWER` is Hamlib's 0–1 fraction, and **only ever lowers** (`scripts/lib/rig-power.sh`).
   - On a 100 W rig, 0.10 is 10 W. The HF profiles default to **0.10**, so a run starts at about 10 W.
     Going above that is a hand change on the rig, never a runner action.
   - The 2 m remote station's 5 W cap stays in its own profile.
5. **The tuner.** The runner keeps `ALLOW_TUNER_ON_HIGH_SWR=0`.
   - The random wire with a 1:9 unun normally needs a tuner, and the operator tunes on the rig before
     the run.
   - The runner reads the SWR once at the start of the first keying: `rigctl l SWR`, where supported,
     UNCHECKED for both rigs. Above `MAX_SWR` (2.0), it runs `cleanup` and stops.
   - This reading is UNCHECKED-capable. If a backend has no SWR reading, the report says "not read".
6. **Profiles for the HF pair:** `docs/config/onair-k4-ic7300mk2.example.sh`.
   - **K4:** Hamlib 2047, CAT and PTT over CAT.
   - **IC-7300MK2:** Hamlib 3073 and CI-V. The address and baud are UNCHECKED.
   - Both: `PKTUSB` or `USB-D`, as each backend names it (UNCHECKED); the expected AGC; the passband;
     `RFPOWER = 0.10`.
   - A commented 20 m example segment.
   - The audio devices and levels are placeholders, to be measured at the station as for the 2 m pair.
   - **The profile carries no host details**, as the 2 m profiles do not.

## Verification

- **`run-onair-twin-ota.sh band-self-test`** (new, keys nothing). Cases:
  - **the edge check:** a dial whose `fast` signal ends 1 Hz inside the top edge passes; 1 Hz outside
    fails; the same at the bottom edge; `robust` at the same dial passes where `fast` fails;
  - **an LSB-family mode** is refused;
  - **the 2 m defaults** with an HF dial are refused;
  - **an `OCCUPIED_BW_HZ` override below the profile's value** is refused;
  - **the passband:** 2 400 passes, 2 399 fails;
  - **the AGC:** match passes, mismatch fails, `any` passes with a note.
  - The checks are pure functions, called with numbers, as the watchdog's are.
- **Sabotage:** each check removed in turn turns exactly its cases red.
- **On the stations, with nothing keyed:** a `preflight`-only call (a new `check` action that runs
  preflight and `tune`'s read-backs, then stops before any `launch_daemon`).
  - Use the 2 m pair now: an HF dial must be refused, and a 2 m dial accepted.
  - Use the HF pair later.

## What it does not do

- **It does not choose the band, the power or the tuner** (the maintainer's, per the plan).
- **It does not make the software check band edges on transmit.** The daemon still has no edge check;
  that would be a product feature, and post-release.
- **It does not set AGC or the rig's DSP** (NR, NB, notch, TX processing). It reads the AGC only.

## Questions for the review

1. Is the edge check's model right? Dial + 1 500 Hz ± half the occupied width, on USB. Does any rung or
   the K=3 ACK, the FSK4 ACK or the ID occupy more than the profile's figure? The ACK waveforms are
   narrower (500 Hz), and the ID is BPSK250.
2. Should the runner *refuse* a missing AGC or passband reading (fail closed), or only warn when the
   backend cannot read it?
3. Is the SWR stop worth having, given that the rig's own protection exists? And can reading SWR on
   these backends key or interfere?

## Consumer

`scripts/run-onair-twin-ota.sh` (`preflight`, `tune`, a new `check` action and `band-self-test`), run
by the station session. The 2 m and HF profiles in `docs/config/`.

## Prior art

- The runner's dial guard (`BAND_MIN_HZ`).
- `onair-station-check.sh`, which reads `EXPECT_AGC`, optional and read-only.
- `scripts/lib/rig-power.sh` (lower-only).
- The watchdog's pure-function self-test (#1534).

## Twins

The other on-air runners:
- **`run-onair-tx500-kx3.sh`** is the HF-capable twin. Its band guard and AGC handling are
  UNCHECKED; the review should name what it lacks.
- **`run-onair-ic9700-ft991a.sh`** is the 2 m single-run runner.
- **`onair-station-check.sh`** carries the optional AGC reading.
