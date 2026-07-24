---
project: openpulsehf
doc: docs/dev/onair-ic9700-ft818-setup.md
status: living
last_updated: 2026-07-24
---

# On-air setup — IC-9700 (rpi51, stationary) ↔ FT-818 (laptop, portable)

A second two-station 2 m pairing, on the same 144.640 MHz used before. Station A is the IC-9700 on
`dc0sk-rpi51`, reached over the VPN tunnel; its configuration is unchanged from the earlier
IC-9700 ↔ FT-991A campaign. Station B is new: a **Yaesu FT-818 + SCU-17 USB interface + LDG Z817
tuner**, connected to this laptop and used portable.

Config profile: [`docs/config/onair-ic9700-ft818.example.sh`](../config/onair-ic9700-ft818.example.sh).
Runner: `scripts/run-onair-ic9700-ft991a.sh` (rig-B-agnostic — the filename is historical). The
execution order and the *why* (fix the receive path first) live in
[onair-execution-plan.md](onair-execution-plan.md); this document is the station-specific setup.

---

## ⚠️ Read first — two things about this specific station

1. **The LDG Z817 does not work on 2 m.** It is an HF (≈1.8–54 MHz) autotuner; at 144 MHz it does
   not tune and passes straight through in bypass. For this test the 2 m antenna on the rear SO-239
   **must present a usable SWR on its own** — the tuner cannot correct it here. If the Z817 is
   physically inline, confirm it is in bypass (it will not engage on 2 m) and check SWR on the FT-818
   before any keyed run.
2. **The rear SO-239 at 2 m.** The FT-818's rear SO-239 is primarily an HF/6 m connector; the front
   BNC is the intended VHF/UHF jack. The radio *can* be menu-set to route 2 m to the rear socket, but
   SO-239/PL-259 is a lossy connector at 144 MHz — expect a little more feedline loss than the front
   BNC would give. This is an operational choice (requested), not the factory default; just be aware
   the link budget is slightly worse than a BNC 2 m feed.

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

## Station B — FT-818 + SCU-17, on this laptop

The SCU-17 presents, over one USB cable: a **USB Audio CODEC** (TX/RX audio via the FT-818 DATA/ACC
jack) and a **virtual COM port for CAT** (to the FT-818 CAT jack). PTT is either CAT keying over that
COM port or the SCU-17's hardware RTS line.

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
`TODO-CONFIRM` there because the interface was not connected when the profile was written — do not run
until they are real values.** (Offer: with the SCU-17 plugged in, this can be filled in interactively.)

### B.2 — FT-818 radio settings (set on the radio; not configurable from software)

| Setting | Where | Value | Why |
|---|---|---|---|
| **Antenna (2 m)** | Menu → antenna-select item (the FRONT/REAR per-band setting; confirm the item number on your firmware) | **REAR (SO-239)** | Requested. Routes 144.640 MHz out the rear socket. See the caveat above. |
| Mode | mode button | **DIG** | The FT-818's data mode (there is no "PKT" mode as on the FT-991A) |
| DIG MODE | Menu → "DIG MODE" | **USER-U** | Upper-sideband user-defined digital = the FT-818 equivalent of PKTUSB; puts the 1500 Hz audio carrier on USB |
| DIG GAIN | Menu → "DIG GAIN" | ~50 (start) | Data input gain from the SCU-17 CODEC; sets ALC. Start mid, adjust to keep ALC just deflecting |
| CAT RATE | Menu → "CAT RATE" | **38400** | Match `B_CAT_BAUD` in the profile |
| CAT/DIG DATA source | Menu (DIG audio routing) | via DATA/ACC jack | The SCU-17 injects/receives audio on the DATA jack |
| NB (noise blanker) | front panel / menu | **Off** | Distorts BPSK |
| DSP NR | menu | **Off** | Distorts BPSK |
| RF power | Menu → power / or the profile's `B_RFPOWER` | ~2–3 W | Low power for the test; the FT-818 is 6 W max |
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
   ssh dc0sk@localhost   'echo laptop ok'       # Station B is local; needs sshd on the laptop
   ```
2. **cpal audio really works on the laptop** (presence of the binary is not capability):
   ```bash
   ./scripts/onair-preflight.sh --strict        # now includes a `--backend cpal devices` probe
   ```
3. **Phase G0 — the FT-818 receive floor is clean** (the conducted-RFI check; this laptop is exactly
   where the earlier campaign found birdies). With **no TX anywhere and the SDR stopped**:
   ```bash
   # once B_AUDIO_DEVICE is known, point the ALSA capture at the SCU-17 CODEC:
   scripts/onair-rx-idle-floor.sh plughw:CARD=<SCU17-CODEC>,DEV=0
   ```
   Exit 0 = clean. If it fails with birdies in 300–2600 Hz, this laptop is conducting RFI into the
   SCU-17 audio (the documented rig→rig blocker) — apply galvanic USB isolation on the SCU-17 link
   and re-run until it passes. Do the same idle check on the IC-9700 side over SSH.
4. **Signal-chain gates 1–6** from [onair-signal-chain-verification.md](onair-signal-chain-verification.md),
   with Station B now the FT-818: CAT connectivity, a tone deflecting ALC, a synchronized RX capture
   ≥ 0.005 mean-sq (Gate 3, both directions), carrier at 1500 Hz, one BPSK250 frame decoded.
5. **SDR monitor up** (`scripts/onair-sdr/sdr_capture.py`) — the RSP2pro on this laptop is co-located
   with the portable FT-818, so it independently monitors every burst and is the arbiter of any
   rig-RX result.

---

## Run

```bash
source docs/config/onair-ic9700-ft818.example.sh
# fill B_CAT_PORT and B_AUDIO_DEVICE first (they are TODO-CONFIRM)
./scripts/run-onair-ic9700-ft991a.sh supervise --quick     # BPSK250 x {none, rs, soft-concatenated}
```

Evidence bundles to `docs/dev/test-reports/on-air/` via `scripts/onair-bundle-evidence.sh`. Do not
trust a rig-RX PASS unless the SDR corroborates it or gates 1–6 passed clean in the same session
(onair-execution-plan.md §7).

---

## What is new vs. the FT-991A pairing (so nothing silently drifts)

- Station B rig: FT-818 (hamlib **1041**), DIG/**USER-U** mode (not PKT-U), **DIG GAIN** not PKT MIC
  GAIN, and the **rear SO-239** antenna at 2 m.
- Station B host: **this laptop, local** (`B_SSH=dc0sk@localhost`, needs sshd) — not a remote Pi.
- The **LDG Z817 is HF-only** and inert on 2 m; the antenna must be resonant on its own.
- The **SDR is co-located** with the portable station, which is ideal for monitoring.
- Everything on the IC-9700/rpi51 side is unchanged, including its unresolved USB-capture-during-TX
  caveat (Gate 3 covers it).
