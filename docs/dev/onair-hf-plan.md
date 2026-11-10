---
project: openpulsehf
doc: docs/dev/onair-hf-plan.md
status: draft
last_updated: 2026-11-10
---

# On-air HF run plan: A1 + A2 with the release candidate

Work plan **M4**, "HF run with the release candidate" (decision 13: HF only with the release candidate).
**Not yet scheduled.** The maintainer, 2026-11-10: "we cannot do HF test at the moment, but we can prepare
them". This plan is the preparation, and it is kept current until the run.

## What the run must show

From `project/release-1.0-criteria.md`, §A1 and §A2, unchanged from the 2 m campaign:
- **A1:** a two-station on-air QSO completes over HF on the `hpx_hf` ladder, with logs retained
  (`onair-bundle-evidence.sh`).
- **A2:** in **both directions**, scored as on 2 m (tasks 32–33). Each needs:
  - **a climb** whose next frame at the new mode decodes;
  - **a demotion** after a logged decode failure;
  - **stability**: no demotion over a window of clean decodes.
- **Corroboration:** in both stations' logs, and in the capture.
  - Single-carrier rungs are told apart by baud and bandwidth; the OFDM rungs by burst duration.
  - Rungs closer than the envelope's resolution are "not separable", not corroborated.
  - A transition the SNR estimate alone supports does not count.
- **Induced level changes count and are recorded** (`lock_level` does not), with `audit_mode` on.
- **What it cannot show, as stated in §A2's Residual:** induced runs exercise the SNR-explained
  branches. The evidence climb and the adequate-SNR hysteresis count only if natural fading happens to
  be captured.
- **A failure attributable to propagation is recorded, not chased** (M4).

## The station pair (the maintainer, 2026-11-10: "most likely")

| | station 1 | station 2 |
|---|---|---|
| rig | **Elecraft K4** | **Icom IC-7300MK2** |
| antenna | random wire, **1:9 unun** | end-fed, **1:49** (40 / 20 / 15 / 10 m) |
| power limit | 100 W | 100 W |
| Hamlib model | 2047 (`RIG_MODEL_K4`, Stable) | 3073 (`RIG_MODEL_IC7300`, Stable). There is no separate MK2 entry: UNCHECKED that it answers as 3073 |
| repo profile | none yet (an "Elecraft K4 support" work stream exists separately) | `crates/openpulse-radio/rigs/icom-ic7300.toml`, `docs/config/rig-icom-ic7300.toml` (the original IC-7300) |

Neither rig has been on this project's air yet. Both are new to the runner (see Runner).

**Power.** 100 W is each station's limit, not the operating level. The rule stays "minimum necessary
power" (`docs/regulatory.md`), and the runners only ever lower a rig's power. **Start low** (proposed
10–20 W), and raise it **by hand on the rig** only if A1 needs it. The software never raises power,
and that does not change.

## Band and segment

Chosen when the RC is cut. It depends on the stations' distance and the time of day (open question 2).
- **`fast` profile** (up to about 2 031 Hz occupied): a wide-digital segment, for example 20 m **14.101–
  14.112 MHz**. Not from 14.099: **14.099–14.101 holds the IBP beacons** (NCDXF at 14.100), and
  `docs/regulatory.md`'s "14.099" is misleading there. Wide-digital segments on 40, 15 and 10 m: UNCHECKED in that doc.
  Check them against the IARU R1 bandplan before choosing.
- **`robust` profile** (≤ 500 Hz): a narrow-digital segment, for example 40 m 7.040–7.060, or 30 m
  10.140–10.150 (`docs/regulatory.md`). 30 m is not on the IC-7300MK2's antenna list.
- **The software does not check band edges on transmit**: the bandplan applies to QSY only. The
  runner's band guard (`BAND_MIN_HZ` / `BAND_MAX_HZ`) is the only check, and it must be set to the
  chosen segment.
- **Since `onair-runner-hf.md`, the guard checks the transmission's edges, not the dial:** the dial
  minus any display-only shift, plus the rig's TX passband (default 0–3 000 Hz). The rig's TX filter
  bounds what the OFDM clipping regrows.
- **Identification:** every 10 min and at the end (the default of 600 s).

## Receive settings (Release 1 operating requirements)

- **AGC FAST on both rigs, read back before and after every run** (`design/dcd-raised-floor.md`). FAST is
  Hamlib's value 2 on the K4 and the IC-7300. The runner reads it and refuses anything else; it does not
  set it.
  - **An open conflict, the maintainer's:** on 2 m the IC-9700 ran SLOW by the maintainer's measured choice
    at Gate 3 (task 3: FAST 4.9x, SLOW 7.9x), while `dcd-raised-floor.md` requires FAST. Tasks 29–36 ran
    FAST.
- **The receive filter is ≥ 2.4 kHz.** No narrow filter: a 250 Hz filter decodes 0 of 180 (#1060,
  `release-1.0-criteria.md`). The FT-991A's known-good width was 2 400 Hz; for the K4 and the
  IC-7300MK2, UNCHECKED.
- **PKTUSB (both backends' name for the data mode).** Plain USB on the IC-7300MK2 keys the microphone, a
  dead carrier, and the runner refuses it there.
- **Split off** (the dial read back is the RX VFO); the runner refuses split.
- **The rig's own processing off:** NR, NB, its own notch, and any TX
  compression or EQ. The exact menu items per rig: UNCHECKED.
- **Record `notch_enabled`, `agc_enabled` and `cessb_enabled` in every bundle.** The notch ships on,
  and REQ-QRM-01 is unproven since `884d96ed` (#1457).

## Run schedule (per direction; the pattern of tasks 32–36)

Each run goes under the runner watchdog (#1534, #1535), with `RUN_CAP_SECS` set for the run. One run
is one go.

1. **Part A, nothing transmits:**
   - check the hashes (both stations on one build: the RC);
   - read back the rigs: dial, mode, passband, AGC, power;
   - set the audit files aside (the runner does this);
   - run `watchdog-self-test`.
2. **A1 + A2 climb and stability**, in the A → B direction:
   - `fast` profile, a traffic volume the window drains (the task 36 pattern);
   - keep the run going long enough to see a stable ceiling.
3. **An induced-demotion step** (R4 of `ota-climb-memory.md`): with the climb memory, a stable ceiling
   gives no natural step-down.
   - Lower the sending station's drive in about 1.6 dB steps once its ceiling has held, as in task 33.
   - Stop rule (i): a receiver silent while preambles arrive. Stop rule (ii): nothing arriving.
   - Then restore.
4. **The same in the B → A direction.**
5. **An idle tail of 15 min after the sign-offs, under the watchdog**, to observe the idle gate.

## Observations owed to the HF run

These are deferred here by earlier reviews; each is a line in the run's report.

| what | from |
|---|---|
| fade-split NACKs (#1452): count them during A1 / A2 | the task 35 review, a parked row |
| `NackStepDown` with no peer frame on air (#1460 costing a session) | the work plan, the #1460 row |
| `no live peer` lines, and NACKs after 10 min quiet (#1532) | the task 36 review |
| the preamble detect's false-open rate, read beside `carrier offset acquired` | `ofdm-preamble-detect.md`; the decision 4 parked row |
| frames too weak to acquire, silent, with no demotion: decide whether a demotion path is needed | the decision 4 parked row |
| whether the climb memory's backoff and expiry constants hold ("UNCHECKED on HF") | `ota-climb-memory.md` |
| the noise floor under each rig's AGC (a raised floor stalled A2's clause 2 on 2 m) | `dcd-raised-floor.md`; `onair-tasks.md` task 21 |
| `capture read holds` lengths on the station Pis | the uncoded-fallback stall (below) |

## Before the trip (engineering)

1. **The uncoded-fallback stall:** each failed burst costs about 1.5 s on a Pi 5, which is above the
   0.5 s evidence floor.
   - **For union-ACK pieces it is fixed** (`ack-piece-skips-fallback.md`: 82–86 ms against
     830–1 160 ms, x86).
   - **Bounding the fallback for other failed bursts** (fade-split fragments on HF) is still open.
2. **The runner, generalised for HF:** done, see Runner. The first `check` on the HF pair reads every
   UNCHECKED item in the profile.
3. **A 2 m run per direction on the RC**, if engine changes land after `a1dafca7` that an exchange
   enters (the task 36 review).
4. **The BPSK31 sideband fix** (#1520) is merged; its review's "blocks … before the HF confirmation" is
   therefore met. Confirm when the RC is cut.

## Runner: what is 2 m-specific today (`scripts/run-onair-twin-ota.sh`)

- **The frequency and band guard:** `TEST_FREQ_HZ = 144640000`, and the guard is 144.500–144.750 MHz.
- **The per-station dial offsets:** B's `+1564 Hz` relies on the FT-991A's menu 065. It has to be
  measured per rig on HF.
- **The station profiles** (CAT model, audio device, levels, mode) name the IC-9700, FT-991A and
  FT-818. **New profiles are needed for the K4 and the IC-7300MK2.**
- **Rig AGC is neither set nor checked.** The FT-818 profile expects the IC-9700 at AGC SLOW, the
  maintainer's measured 2 m choice (see Receive settings).
- **`RFPOWER` defaults and the remote station's 5 W cap** belong to the 2 m pair.

**Done (`design/onair-runner-hf.md`, reviewed):**
- the transmission's edges inside the segment;
- the display shift;
- split, AGC and passband read back and refused when wrong;
- plain USB refused on an Icom;
- a keyless `check` action (for PTT type CAT);
- `band-self-test`;
- the profile `docs/config/onair-k4-ic7300mk2.example.sh`.

There is no SWR stop: the K4 has no SWR level, and polling during TX toggled a rig's VFOs before. SWR is
recorded as evidence.

## Witness

On 2 m the RSPdx sat beside station A. For HF:
- **Which station it sits at: UNCHECKED.**
- **Its HF coverage:** INFERRED from the RSPdx's specification, and not in the project docs.
- **Its input protection:** a fixed attenuator is needed beside a 100 W station, and its rating is
  UNCHECKED.

## Open questions (the maintainer's)

1. **Where are the two stations, and how far apart?** That decides the band (NVIS on 40 m, or skip on
   20 m and up), and whether a random wire suits it.
2. **Which band(s) and profile first?**
3. **The tuners.**
   - The random wire with a 1:9 unun normally needs a tuner. Does the K4 use its internal ATU?
   - The 1:49 end-fed is resonant on 40/20/15/10 m. Does the IC-7300MK2 run it without its tuner?
   - The daemon's `allow_integrated_tuner_on_high_swr` is off by default. Should a run allow it?
4. **CAT:**
   - Does the IC-7300MK2 answer to Hamlib model 3073, and on which CI-V address and baud?
   - The K4's CAT and audio path: USB, or LAN plus K4-remote?
5. **Who attends each station during a run?** The watchdog bounds a run, but a run is attended.
6. **The operating power** for the first run, within 100 W.
7. **The witness receiver:** where, and with which attenuator?

## Consumer

The on-air session that runs M4's HF task, and `scripts/run-onair-twin-ota.sh`, once it is generalised.

## Prior art

The 2 m campaign: `onair-tasks.md` tasks 6–36, `onair-execution-plan.md`, and the A2 scoring in tasks
32–33.

## Twins

UNCHECKED: there are no other HF plans. `docs/on-air_testplan.md` is an older general test plan.
