---
project: openpulsehf
doc: docs/dev/design/key-to-audio-gap.md
status: draft
last_updated: 2026-10-12
---

# The key-to-audio gap: open the output device outside the keyed window

> **Round 1 (2026-10-11): not implemented**
> ([`reviews/review-key-to-audio-gap.md`](../reviews/review-key-to-audio-gap.md)).
> - **B** cannot be built as written: a cpal stream is `!Send`, and a session-long stream can play
>   stale audio or key the rig silently.
> - **A** holds the card's capture PCM, so a single-card receiver goes deaf.
> - **The measured cost was not the production path.**
>
> Round 2 waits on the extended station probe (pass 1, pass 2, open by name, capture held). It will
> combine A′ (the resolved name cached, pass 1 skipped), the same fix on the input side, and a
> measured leader default. **Not a Release 1 blocker.**

Work plan M2, *Key-to-audio gap*. Every keyed turn carries dead air between PTT and the first sample.
It is about 1.3 s on the 2026-07-30 IC-9700 captures (`crates/openpulse-modem/tests/captures/README.md`).
That is airtime on every turn and an uncontrolled implicit leader. **The fix moves work out of the
keyed window, and it changes when PTT is released relative to the audio (the #1367 drain), so it is
reviewed before it is built (decision 8).**

## Measured

**Synthesis is ruled out** (`tx_synthesis_cost`, 2026-10-05): ≤ 12 ms through SL11, and 19–26 ms at
SL12–14.

**The device path, rpi51** (`key_to_audio_by_stage`, `09762a3a`, 2026-10-11 and 2026-10-04, the
system default device):

| stage | runs 1–5 |
|---|---|
| enumerate output devices | 1 084 / 532 / 494 / 464 / 595 ms (16, then 9 devices) |
| `open_output` | 95 / 32 / 32 / 52 / 47 ms |
| write → drained − audio length | 133–144 ms |

`CpalBackend::open_output` enumerates every output device on each open whenever a device **name** is
configured (`select_cpal_device`, two passes), and the on-air config always names one. So a station
pays roughly 0.65–1.3 s per keyed turn, which matches the captures.

The probe's own runs show a hazard: **the device count fell from 16 to 9 after the first run.**
`select_cpal_device` already notes that retaining a device handle truncates ALSA's enumeration. That
constrains option A below.

**The production path, measured** (2026-10-12, rpi51, `plughw:CARD=CODEC,DEV=0`, the station's
configured device, with its capture stream held open as on a running station; five runs):

| stage | ms |
|---|---|
| pass 1: every output device, named (16 devices every run) | 525–540 |
| pass 2: `find` the resolved name | 42–45 |
| `open_output` by name (both passes plus the open) | 567–684 |
| write → drained − audio (start-up + device latency) | 138–144 |
| **key-to-audio** | **707–828** |

What the measurement settles:
- **The cost is enumeration.** `open_output` is pass 1 + pass 2, so the stream build is negligible.
- **Pass 1 alone is about 75 % of the gap.**
- **Holding the capture stream hid nothing.** All 16 devices were listed and the name was found in
  every run, so round 1's finding 6 does not occur on this station's stack.

## Where the time sits

`keyed_transmit` asserts PTT and then runs the transmit closure. Inside it come
`modulate` → `stage_emit_output` → `open_output` (enumerate + build + start) → `write` → `flush`
(the device-reported drain, #1367) → return → PTT release. Everything before `write` is dead air.

## Options

**A. Resolve the device once.** The backend keeps the resolved `cpal::Device` per name. A later open
skips the enumeration, and on a failure it re-enumerates once (hotplug).
- **Removes:** the enumeration (0.46–1.08 s).
- **Keeps:** `open_output` and stream start-up, about 0.2 s.
- **Risk:** a retained handle truncates later ALSA enumerations. On a station the receiver's
  `open_input` or a QSY re-open could then not find its device. That has to be measured on the
  station before choosing A.

**B. Keep the output stream open for the session.** The engine opens the output once, when the
station starts, and keeps it playing silence. A frame is written into it, and `flush` waits for that
frame's samples to drain.
- **Removes:** the enumeration, the open and the start-up. What is left is the device's buffering,
  about 0.1 s.
- **Costs:**
  - stream lifecycle: an error or unplug mid-session needs a reopen, without crashing the station;
  - the drain must count this frame's samples, not the silence stream's;
  - the DAC idles into the rig's input while the station is not keyed. That is silence, so VOX does
    not trip (the station probe already writes silence).

**C. Open before keying** (the work plan's first idea). This reorders the open before the PTT
assert. It shortens nothing that A or B do not, and it moves the PTT assert, so it is not proposed.

**Proposed: B,** if the station probe below confirms its two numbers, falling back to A if B's drain
cannot be made per-frame. Neither changes when PTT is asserted. Both shorten the time from assert to
the first sample, and the release still waits on the device-reported drain of the frame's own samples.

## Station probe (pre-registered; silence only, PTT untouched)

`key_to_audio_by_stage` gains two arms, each run 5× on the configured named device.
- **A:** enumerate once and retain the device. Re-open the output 5× from the retained handle,
  timing each open. After each open, count `input_devices()` and `output_devices()` and compare
  them with a fresh process's counts.
- **B:** open once and play silence. Five times: write 0.5 s of silence and time `flush`, minus the
  audio length.

**Rule:**
- B is chosen if its flush overhead is ≤ 200 ms in all 5 runs.
- A is acceptable only if no device count drops after the first retain.
- If neither holds, this goes back to the design.

## Tests (with the implementation)

- **Unit, backend:** a persistent stream's `flush` returns only after the samples written since the
  last flush have played. Under the loopback backend the drain counter moves.
- **The #1367 drain-from-device tests** and `keyed_transmit`'s release tests stay green. The tail
  and release checks run again at the hardware-loopback stage.
- **Stream loss:** a stream error between frames reopens on the next write. Tested with a fault
  injected into the loopback backend.

## Consumer

`ModemEngine::stage_emit_output` (every audio TX seam) and its callers inside `keyed_transmit` in
`crates/openpulse-daemon/src/server.rs` (OTA send, ACKs, filexfer, beacons). Found by
`grep -rn "open_output(" crates apps --include=*.rs` and `grep -n "keyed_transmit" crates/openpulse-daemon/src/server.rs`.

## Prior art

`docs/dev/design/drain-from-device-delay.md` (#1367, how release waits for the drain);
`select_cpal_device`'s two-pass enumeration and its truncation note; the persistent capture stream
the receiver already holds (`open_input` once per session). Found by
`grep -n "fn select_cpal_device\|fn open_input" crates/openpulse-audio/src/cpal_backend.rs`.

## Twins

- The repeater's `open_output` wrapper (`crates/openpulse-repeater/src/lib.rs`).
- The testbench's signal path.
- linksim's GUI player.

These open their own outputs and are not on the Release 1 keyed path. Each is stated, not changed.
Found by the same `open_output(` grep.
