---
project: openpulsehf
doc: docs/dev/onair-ic9700-ft818-setup.md
status: living
last_updated: 2026-10-14
---

# On-air setup — IC-9700 (rpi51) ↔ FT-818 (rpi53)

A second two-station 2 m pairing, on the same 144.640 MHz used before. Station A is the IC-9700 on
`dc0sk-rpi51`, reached over the VPN tunnel; its configuration is unchanged from the earlier
IC-9700 ↔ FT-991A campaign. Station B is new: a **Yaesu FT-818 + SCU-17 USB interface + LDG Z817
tuner**. It was set up portable on a laptop; since the station pair was fixed (2026-10-02) it runs on
**`dc0sk-rpi53`**.

**Station owner's rules for the FT-818 (2026-10-14), which this document and the tooling follow:**
- **Low power, never increased.** Set **L1 (0.5 W)** on the front panel. Hamlib's FT-818 driver cannot
  set power, so software cannot do it; the runners only ever *lower* a rig's power
  (`scripts/lib/rig-power.sh`) and leave the FT-818's alone.
- **The FRONT antenna (BNC) is selected and stays selected.** No tooling sends an antenna command.
- **Mode is set on the rig** (DIG, DIG MODE = USER-U) and the runners send none for the FT-818
  (`TEST_MODE_RIG_B=keep`).

**Read over CAT on rpi53, 2026-10-14** (read-only; nothing keyed or changed): Hamlib 4.6.2, model 1041;
CAT on the SCU-17's CP2105 first port (`…CP2105…-if00-port0`, `/dev/ttyUSB0` that boot) at **38400**
baud; 144.600 MHz, mode USB 2200 Hz at the time; `l RFPOWER` reads `10` (not Hamlib's 0–1 scale, so
not a power setting), `l RFPOWER_METER` is unsupported, and the antenna is not readable (`y`).
Setting any level other than `BAND_SELECT` is unsupported. Audio: ALSA card `CODEC`
(`plughw:CARD=CODEC,DEV=0`).

Config profile: [`docs/config/onair-ic9700-ft818.example.sh`](../config/onair-ic9700-ft818.example.sh).
Runner: `scripts/run-onair-ic9700-ft991a.sh` (rig-B-agnostic — the filename is historical). The
execution order and the *why* (fix the receive path first) live in
[onair-execution-plan.md](onair-execution-plan.md); this document is the station-specific setup.

---

## ⚠️ Read first — two things about this specific station

1. **The LDG Z817 does not work on 2 m.** It is an HF (≈1.8–54 MHz) autotuner; at 144 MHz it does
   not tune and passes straight through in bypass. For this test the 2 m antenna on the front BNC
   **must present a usable SWR on its own** — the tuner cannot correct it here. If the Z817 is
   physically inline, confirm it is in bypass (it will not engage on 2 m) and check SWR on the FT-818
   before any keyed run.
2. **The front BNC at 2 m.** The station owner's choice (2026-10-14), replacing the earlier request for
   the rear SO-239: the front BNC is the FT-818's intended VHF/UHF jack. Confirm the per-band antenna
   menu reads FRONT for 144 MHz before any keyed run; it cannot be read over CAT.

---

## Station A — IC-9700 on rpi51 (unchanged)

Reached over the VPN as `dc0sk@dc0sk-rpi51`. Use the IC-9700 required-settings table and the Side-A
audio notes in [onair-signal-chain-verification.md](onair-signal-chain-verification.md) verbatim —
DATA MOD = USB, USB AF Output = AF, PKTUSB mode, NR/NB off, squelch open, the PulseAudio sink for
audio (`hw:`/`plughw:` is blocked by PulseAudio's exclusive hold and produces no RF). Nothing about
Side A changes for this pairing.

**Carry-over caveat:** the earlier campaign's unresolved Side-A item — the IC-9700 USB capture reading
flat during the far station's TX — is a property of this same rig and may recur. Signal-chain **Gate 3**
(a synchronized capture during a real TX must show peak mean-sq ≥ 0.005) is exactly the check for it;
do not skip it.

---

## Station B — FT-818 + SCU-17, on rpi53

The SCU-17 presents, over one USB cable: a **USB Audio CODEC** (TX/RX audio via the FT-818 DATA/ACC
jack) and a **CP2105 dual USB serial bridge**: two ports, of which the **first** (`…-if00-port0`)
answered CAT at 38400 baud on rpi53 (2026-10-14). PTT is either CAT keying over that port or the
SCU-17's hardware RTS line.

### B.1 — Discover the device names (do this once the SCU-17 is plugged in)

```bash
# Audio CODEC card (fill B_AUDIO_DEVICE):
arecord -l | grep -i codec
./target/release/openpulse --backend cpal --log error devices | grep -i codec   # exact cpal name

# CAT serial port (fill B_CAT_PORT):
ls -l /dev/serial/by-id/     # the SCU-17 CAT port; prefer the by-id path (survives re-enumeration)

# lsusb should show a TI/Burr-Brown USB Audio CODEC and a Silicon-Labs/FTDI serial bridge:
lsusb | grep -iE 'burr|texas|silicon|ftdi'
```

Put the results into `B_AUDIO_DEVICE` and `B_CAT_PORT` in the config profile. **These are marked
`TODO-CONFIRM` there until read from the hardware — do not run
until they are real values.** (Offer: with the SCU-17 plugged in, this can be filled in interactively.)

### B.2 — FT-818 radio settings (set on the radio; not configurable from software)

| Setting | Where | Value | Why |
|---|---|---|---|
| **Antenna (2 m)** | Menu → antenna-select item (the FRONT/REAR per-band setting; confirm the item number on your firmware) | **FRONT (BNC)** | The owner's rule: selected and stays selected. Not readable over CAT, so check it on the radio. |
| Mode | mode button | **DIG** | The FT-818's data mode (there is no "PKT" mode as on the FT-991A) |
| DIG MODE | Menu → "DIG MODE" | **USER-U** | Upper-sideband user-defined digital = the FT-818 equivalent of PKTUSB; puts the 1500 Hz audio carrier on USB |
| DIG GAIN | Menu → "DIG GAIN" | ~50 (start) | Data input gain from the SCU-17 CODEC; sets ALC. Start mid, adjust to keep ALC just deflecting |
| CAT RATE | Menu → "CAT RATE" | **38400** | Match `B_CAT_BAUD` in the profile |
| CAT/DIG DATA source | Menu (DIG audio routing) | via DATA/ACC jack | The SCU-17 injects/receives audio on the DATA jack |
| NB (noise blanker) | front panel / menu | **Off** | Distorts BPSK |
| DSP NR | menu | **Off** | Distorts BPSK |
| RF power | Front panel → PWR | **L1 (0.5 W)** | The owner's rule: low power, never increased. Not settable over CAT; the runners never raise power |
| Clarifier (RIT) | CLAR | **Off / 0 Hz** | Offsets RX |
| Squelch | menu / knob | open (min) | Squelch gates the USB capture audio |

The exact menu item **numbers** vary by firmware — set them by function against the FT-818 operating
manual, and read back what you can (`FA;` VFO, `MD;` mode) over CAT to confirm.

### B.3 — PTT choice

Two options; pick one and prove it with `openpulse calibrate ptt` (asserts/releases and measures
latency against the 50 ms target):

- **CAT PTT via hamlib (recommended, and what the profile defaults to):** `B_PTT_TYPE="CAT"`. The
  FT-818 CAT command set keys the radio; robust and survives the setup. Requires rigctld on the CAT
  port. This matches Side A.
- **SCU-17 hardware RTS-PTT:** `B_PTT_TYPE="RTS"` + `B_PTT_PORT=<the SCU-17 serial port>`. The classic
  Yaesu-interface method (RTS on the SCU-17 routes to the DATA-jack PTT). Use only if CAT PTT is
  unreliable on the unit.

---

## Preflight — before any keyed run

Run in this order; stop on the first failure.

1. **VPN + SSH reachability.**
   ```bash
   ssh dc0sk@dc0sk-rpi51 'echo rpi51 ok'        # over the VPN
   ssh dc0sk@dc0sk-rpi53 'echo rpi53 ok'        # Station B
   ```
2. **cpal audio really works on rpi53** (presence of the binary is not capability):
   ```bash
   ./scripts/onair-preflight.sh --strict        # now includes a `--backend cpal devices` probe
   ```
3. **Phase G0 — the FT-818 receive floor is clean** (the conducted-RFI check; the earlier campaign
   found birdies on the laptop that hosted this station then). With **no TX anywhere and the SDR stopped**:
   ```bash
   # once B_AUDIO_DEVICE is known, point the ALSA capture at the SCU-17 CODEC:
   scripts/onair-rx-idle-floor.sh plughw:CARD=CODEC,DEV=0   # the SCU-17 CODEC on rpi53
   ```
   Exit 0 = clean. If it fails with birdies in 300–2600 Hz, the host is conducting RFI into the
   SCU-17 audio (the documented rig→rig blocker) — apply galvanic USB isolation on the SCU-17 link
   and re-run until it passes. Do the same idle check on the IC-9700 side over SSH.
4. **Signal-chain gates 1–6** from [onair-signal-chain-verification.md](onair-signal-chain-verification.md),
   with Station B now the FT-818: CAT connectivity, a tone deflecting ALC, a synchronized RX capture
   ≥ 0.005 mean-sq (Gate 3, both directions), carrier at 1500 Hz, one BPSK250 frame decoded.
5. **SDR monitor up** (`scripts/onair-sdr/sdr_capture.py`): the RSP2pro independently monitors every
   burst and is the arbiter of any rig-RX result.

---

## Run

```bash
source docs/config/onair-ic9700-ft818.example.sh
# fill B_CAT_PORT first (TODO-CONFIRM); set the FT-818 to DIG/USER-U on the radio, read it with
# `rigctl -m 2 -r 127.0.0.1:4532 m`, and put that reading in TEST_MODE_EXPECT_B, or the run stops
./scripts/run-onair-ic9700-ft991a.sh supervise --quick     # BPSK250 x {none, rs, soft-concatenated}
```

Evidence bundles to `docs/dev/test-reports/on-air/` via `scripts/onair-bundle-evidence.sh`. Do not
trust a rig-RX PASS unless the SDR corroborates it or gates 1–6 passed clean in the same session
(onair-execution-plan.md §7).

---

## What is new vs. the FT-991A pairing (so nothing silently drifts)

- Station B rig: FT-818 (hamlib **1041**), DIG/**USER-U** mode (not PKT-U), **DIG GAIN** not PKT MIC
  GAIN, and the **front BNC** antenna at 2 m, at **L1 (0.5 W)**.
- Station B host: **`dc0sk-rpi53`** (`B_SSH=dc0sk@dc0sk-rpi53`).
- The **LDG Z817 is HF-only** and inert on 2 m; the antenna must be resonant on its own.
- The **SDR is co-located** with the portable station, which is ideal for monitoring.
- Everything on the IC-9700/rpi51 side is unchanged, including its unresolved USB-capture-during-TX
  caveat (Gate 3 covers it).
