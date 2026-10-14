---
project: openpulsehf
doc: docs/dev/design/key-to-audio-gap.md
status: draft
last_updated: 2026-10-14
---

# The key-to-audio gap: open the output device outside the keyed window

> **Round 1 (2026-10-11): not implemented**
> ([`reviews/review-key-to-audio-gap.md`](../reviews/review-key-to-audio-gap.md)).
> - **B** cannot be built as written: a cpal stream is `!Send`, and a session-long stream can play
>   stale audio or key the rig silently.
> - **A** holds the card's capture PCM, so a single-card receiver goes deaf.
> - **The measured cost was not the production path.**
>
> **Round 2 (2026-10-12): reviewed, no blocking finding; revised for the review; implemented
> 2026-10-13** (`device_cache.rs`).
> It caches the resolved name (option A′) on both directions and holds no device handle and no
> stream. The leader is checked at G1 by SDR capture. **Not a Release 1 blocker.**

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

**The B station, measured** (2026-10-14, rpi53, the FT-818's USB sound interface,
`plughw:CARD=CODEC,DEV=0`, `main` after #1510; FT-818 off, interface only; five runs each):

| stage | capture not held | capture held | rpi51, held (above) |
|---|---|---|---|
| pass 1: every output device, named | 384.0 (8 devices) | 192.2–192.7 (8 devices) | 525–540 (16) |
| pass 2: `find` the resolved name | 16–24 | 16–31 | 42–45 |
| `open_output` by name | 425–457 | 239–256 | 567–684 |
| write → drained − audio | 142.4–142.6 | 142.4–142.6 | 138–144 |
| **key-to-audio** | **567–599** | **382–399** | **707–828** |

- **The B station is not a copy of rpi51.** It lists 8 output devices to rpi51's 16, and its pass 1
  is 192 ms against 525–540 ms under the same condition. The cache removes pass 1 on both, so the
  saving is station-specific: about 0.19 s here, about 0.53 s on rpi51.
- **Holding the capture stream halved pass 1** on rpi53 (384 → 192 ms) and hid no device: 8 listed
  and the name found in every run. The mechanism is not established; the timings repeat to 0.1 ms
  run to run, which points at fixed waits inside ALSA's device probing rather than computation.
- **Pass 2 is 16 ms** although the CODEC is listed after both HDMI cards in `aplay -L`, so round 2's
  ~30 ms-per-hint estimate (from rpi51) does not carry over as stated.
- **Expected with the cache** (derived, not run): `open_output` ≈ pass 2 + the open, about 47–64 ms,
  so key-to-audio about 0.19–0.21 s on rpi53, held. The post-merge probe settles it.

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

## Round 2 (2026-10-12): cache the resolved name, never a handle

**Decision.** `CpalBackend` keeps a cache from (direction, selector) to the resolved
system name.
- **Miss:** `open_output(Some(sel))` and `open_input(Some(sel))` name every device, resolve the
  selector, scan to the match, then store the resolved name (the old two-pass `select_cpal_device`).
- **Hit:** they run **pass 2 only**: scan for the cached name, stopping at it. The scan stays on the
  direction-filtered `output_devices()` / `input_devices()`, not `host.devices()` (review finding 3's
  suggestion): a failed scan's names feed the fallback, and an unfiltered list would let an
  input-only device resolve for output. What the per-device config query adds is UNCHECKED; the
  probe re-run shows the whole scan. Each hit logs the cached name at `debug!`, so
  it can be set beside the diagnostics' fresh resolve.
- **A failed hit** (the name not found): evict the entry. The failed `find` has already named every
  device, so those names go to `resolve_name_from` directly, and the fallback costs no second
  *full* enumeration (one scan to the new match follows, since nothing is retained). A renamed or replugged card therefore costs about one ordinary open, as today. A
  `build_*_stream` or `play` failing on a found device keeps the entry and returns the error, as
  today: the name was just matched exactly, so the failure (EBUSY, a bad rate) says nothing about
  it, and evicting would only make the next keyed open pay a full enumeration (implementation
  review, finding 3).
- **A warm cache is stable:** a substring or `CARD=` selector keeps its pick when a second matching
  device appears later, where a fresh resolve would say Ambiguous. `resolve_output_name`
  (diagnostics) can then disagree with the keyed path; each hit logs its name at `debug!`.
- **What is held:** the cache stores a `String`, behind a `Mutex`, so `CpalBackend` stays
  `Send + Sync`. The lock is held for the get and the insert only, never across an enumeration, and
  a poisoned lock is recovered (`unwrap_or_else(|p| p.into_inner())`, as the file already does). **No `cpal::Device` and no stream is held between opens**, so round 1's findings
  1–4 (`!Send`, stale queued audio, silent keying, the capture PCM held) cannot arise. Every open
  still builds a fresh stream and drops it after its `flush`, exactly as today.
- **`resolve_output_name` / `resolve_input_name`** (diagnostics) keep the full pass and ignore the
  cache, so they always show the live enumeration.

**Expected effect, from the measurement above:**
- **Per keyed turn:** pass 1 (525–540 ms) leaves the keyed window.
- **What is left is not yet measured as key-to-first-sample.** Write → drained − audio (138–144 ms)
  includes the silence pad, the drain margin and a poll, so it bounds the *end* of the stream. The
  probe gains a first-sample stamp (the first callback that pops a non-zero sample), and the post-change
  number is quoted from that, never from pass 2 + drain.
- **Pass 2's cost is station-specific.** cpal opens a PCM pair per hint up to the match: 42 ms on
  rpi51 means the CODEC hint sits near the front of `aplay -L`. Behind another card it grows by
  ~30 ms per hint, and a selector that is itself a software hint (`default`, `pulse`) costs as much
  as pass 1. The probe prints the match's hint index.
- **The first open after start-up** still pays the full resolve, as today.
- **Not the fix, though cheaper:** an empty `[audio] device` with ALSA's `default` pointed at the
  CODEC skips enumeration today with no code. It is operator config, and the Pi's HDMI cards make
  the default unsafe, so it stays a workaround.
- **Fuller, parked:** a hint-based pass 1 (`alsa::device_name::HintIter`, no PCM opens). Pass 2
  would remain, so the cache is the smaller change.

**Input side (round 1, finding 9).** The same cache serves `open_input`, so the ACK listener's
capture open and the RX tick's reopen after a transmit lose pass 1 too.
- **Before:** the peer's ~0.7 s of dead air roughly cancelled the ~0.6 s the listener spent opening
  by name.
- **After:** both shrink by the same pass-1 time on both stations, so the ACK still lands after the
  listener is open.
- **The pinned claim** is relative: the listener's open must not grow relative to the sender's
  key-to-audio. `listen_for_ack` sets its deadline before the capture open. The hardware-loopback
  stage records **ACK onset minus listener open**, not only "ACK decoded", with both stations on the
  new build.
- **Known regression, mixed builds:** an old ISS with a new IRS loses ~0.5 s of ACK margin, since
  the ACK starts earlier into an ISS still enumerating. The A1/A2 stations are rebuilt separately,
  so both are updated before a session ("rebuild BOTH ends").

**Leader (round 1, finding 10).**
- **What changes:** the implicit leader shrinks from ~0.7 s to the key-to-first-sample time plus
  `ptt_leader_ms`, which defaults to 0. That time is below 0.19 s and is measured, not assumed.
- **The IC-9700:** its T/R sequencing reads ~130 ms on the captures (UNCHECKED). The margin is
  unknown until both numbers are measured.
- **No new default constant ships.**
- **The check is at G1, on the rig,** because a hardware loopback has no T/R switch and a decode
  ratio at 2 m SNR cannot see a clip that fails at HF SNR: an SDR capture, PTT click to first audio,
  set against the T/R click pair. `ptt_leader_ms` is set from that measurement.
- **This goes in the maintainer to-do with the change,** not as a guess in the code.

**Tests (with the implementation):**
- **The cache logic is factored over two closures** (pass-1 names, pass-2 find-by-name), so it is
  tested without hardware:
  - N opens of one selector run pass 1 once and pass 2 N times;
  - a hit whose name has vanished evicts and runs the full resolve once, then caches the new name;
  - output and input selectors with the same text are cached apart;
  - an ambiguous selector is never cached;
  - a failed find feeds its names to the fallback, with no second enumeration;
  - a miss after eviction caches the *new* name.
- **The existing `select_cpal_device` resolver tests stay green.**
- **The station probe re-runs** with the cache, expecting `open_output` ≈ pass 2 from the second run
  on, and reports key-to-first-sample and the match's hint index.

**Reviewed** (round 2, [`reviews/review-key-to-audio-gap.md`](../reviews/review-key-to-audio-gap.md)):
no blocking finding. The text above carries findings 1–6 and 9; hotplug, the wrong-device case and
the cache key were checked and found fine.

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
