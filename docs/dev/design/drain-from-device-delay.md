---
project: openpulsehf
doc: docs/dev/design/drain-from-device-delay.md
status: resolved
last_updated: 2026-10-02
---

# Release PTT when the device says the last sample has played (#1367)

Work plan M2, PTT row. REQ-PHY-05: "transmitter release must occur within 50 ms of the last
transmitted sample".

## Problem

`CpalOutputStream::flush` (`openpulse-audio/src/cpal_backend.rs`) does three things:
1. appends a 32 ms silence pad;
2. polls until the software queue is empty;
3. then **sleeps a fixed 200 ms** for the sound card's hardware buffer.

Every `PttKeyGuard` drops after `flush` returns, so PTT is released 200 ms after the queue empties,
whatever the device actually needed. The comment's own worst case is 2 688 samples at 48 kHz
(56 ms), which leaves about 140 ms of software-owned delay against a 50 ms bound.

The earlier plan (a dual-card measurement of the PTT edge) is withdrawn. The dual-card rig is two USB
sound cards with no radio and no PTT line.

## What the device reports

cpal 0.15 passes every output callback an `OutputStreamTimestamp { callback, playback }`. On ALSA
(`cpal-0.15.3/src/host/alsa/mod.rs`, `process_output`), `playback = callback + delay`, where
`delay` is the PCM's own reported delay in frames: what is already queued in the hardware. So the
sample at buffer offset `i` of that callback leaves the DAC at about `callback + delay + i / fs`.

## Decision (revised after review)

Review: [`reviews/review-drain-from-device-delay.md`](../reviews/review-drain-from-device-delay.md).

- **The callback latches a drain mark** when the queue goes from non-empty to empty, **in the same
  mutex as the queue** (review finding 2): `OutQueue { samples, drained }`. The mark is the instant
  the last queued sample leaves the device:

      Instant::now() + (playback − callback) + (last_index / channels + 1) / fs

  - Later callbacks pop nothing and never move it.
  - `write` and the pad append clear it.
  - Today every transmit opens its own stream, so a stream sees one `write` then one `flush`. A
    future caller that writes after a drain re-arms the mark by clearing it.
- **A reported delay of zero is not trusted** (review finding 1, blocking). cpal clamps an underrun's
  negative delay to 0. On a running stream a callback always has something queued, so 0 is the report
  that lies, and a PipeWire or Pulse ALSA plugin after an xrun produces it. Zero, `None`, or a zero
  sample rate → the full 200 ms.
- **`flush`**, once the queue is empty, sleeps `drain_wait(mark − now)`, which is the remaining time
  plus 10 ms, capped at 200 ms, and the full 200 ms with no trustworthy mark. It is never longer than
  today.
- The pad stays. The mark is for the pad's last sample, about 32 ms after the last data sample, so
  release lands about 42 ms after the last data sample plus the device's residual error. For USB
  audio, `snd_pcm_delay` counts submitted URBs and leaves out only the codec FIFO, about 1–2 ms
  (review 5).
- The arithmetic lives in the ungated `flush.rs` (`drain_after_callback`, `drain_wait`), unit-tested
  in the default gate. The callback wiring is behind `cpal-backend`.

## Risks and how each is checked

- **The reported delay is wrong** (USB audio's ALSA delay omits the USB and codec pipeline: a few
  ms). The +10 ms margin covers small errors. If a device under-reports by more than that, the tail
  of the pad is cut. The pad is silence, so the frame survives unless the error exceeds 42 ms.
  - **Checked** at the hardware-loopback stage (decode the frame tail on the dual-card rung) and on
    air at G1: the last frames decode, the same way the first frames check the PTT leader.
  - **UNCHECKED** before then; stated in the PR.
- **Other hosts** (PulseAudio or PipeWire via ALSA, macOS): their `delay` may include large server
  buffers. Then `deadline` is later, which is safe (the cap still bounds it at 200 ms).
- **A release earlier than today on a device whose buffer is genuinely large but under-reported.**
  The cap only bounds the wait from above. The danger is in the other direction, and the G1 tail
  check is what catches it.
- **The PTT leader (#1257)** is unaffected: it acts at key-up, this acts at release.

## Tests

- **`drain_wait` (unit, default gate):**
  - delay 20 ms, offset 0 → about 30 ms;
  - `None` → 200 ms;
  - a large delay → capped at 200 ms;
  - zero channels → no panic.
- **Compile check** with `--features cpal-backend` (the gate does not build it).
- **Hardware:** the dual-card rung (`scripts/run-loopback-dualcard.sh --quick`) on the maintainer's
  machine, and G1. **UNCHECKED** here.

## Consumer

Nothing else depends on the 200 ms (review 6):
- the post-transmit capture drop keys on `frames_transmitted`;
- the OTA send drops PTT before its ACK listen;
- the station-ID timers and `flush_timeout_seconds` precede the sleep.

Every `PttKeyGuard` drop that follows `transmit*` → `stage_emit_output` → `AudioOutputStream::flush`,
on the daemon, CLI and ARDOP paths. Found by `grep -n "fn flush" crates/openpulse-audio/src/cpal_backend.rs`
and `grep -rn "\.flush()" crates/openpulse-modem/src/engine.rs`.

## Prior art

- #1367 (the issue);
- `flush::flush_timeout_seconds` (#997, the adaptive drain timeout);
- the 32 ms pad;
- cpal's `OutputStreamTimestamp`.

Found by `grep -n "flush_timeout_seconds\|trailing-silence" crates/openpulse-audio/src/*.rs`.

## Twins

None. No cpal IQ output stream exists; both loopback flushes return at once (review 8). Found by
`grep -n "from_millis(200)" crates/openpulse-audio/src/*.rs` (one site) and `grep -n "impl AudioIqOutputStream for"
crates/openpulse-audio/src/*.rs` (loopback only).
