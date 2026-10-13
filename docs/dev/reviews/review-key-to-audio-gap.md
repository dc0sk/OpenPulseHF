---
project: openpulsehf
doc: docs/dev/reviews/review-key-to-audio-gap.md
status: resolved
last_updated: 2026-10-12
---

# Review: the key-to-audio gap (round 1, 2026-10-11)

This is the adversarial review (Fable) of [`design/key-to-audio-gap.md`](../design/key-to-audio-gap.md)
as first drafted. It was read-only. The review is mandatory, because the design changes the TX path
inside the keyed window and the PTT release timing (the #1367 drain).

## Prompt

Falsify, do not agree. Read the design, `cpal_backend.rs` (`open_output`, `select_cpal_device`,
`CpalOutputStream` write/flush, the #1367 drain report, the probe), `loopback.rs`, the audio traits,
`stage_emit_output` and every other `open_output` caller, the daemon's `keyed_transmit` and PTT
release, and `drain-from-device-delay.md`. The questions:

1. Is the measured breakdown correctly attributed to the production path?
2. What breaks under option B (a persistent stream)?
3. Is option A's enumeration-truncation hazard real, and does the probe detect it?
4. Is option C correctly dismissed?
5. Is the station probe sufficient to choose?
6. Could either option key the transmitter longer, shorter, or with garbage audio?
7. Does this block Release 1?

## Verdict

**The gap does not block Release 1.** The link works with the dead air, and the cost is airtime plus
an unmeasured implicit leader. **Option B as written would introduce blocking defects into the keyed
path.**

The review recommends **A′**, combined with:
- caching the resolved name and taking pass 1 without opening devices;
- the same fix on the input side;
- a measured leader default.

## Findings

### Blocking

1. **B cannot be built as written.** cpal's `Stream` is `!Send`, while the engine runs under
   `Arc<Mutex<ModemEngine>>` in the ARDOP/KISS bridges and `AudioBackend: Send + Sync`. B needs a
   stream-owner thread with a command/ack protocol, which is a trait change.
2. **B: stale queued audio.**
   - A flush timeout leaves samples queued. Today the stream is dropped with them.
   - Under B they would play after PTT release, or at the head of the next frame.
   - B must discard the queue on every flush error and every PTT release.
3. **B: a dead stream keys the rig silently.**
   - The output error callback only warns; there is no fault latch.
   - `flush` waits out its timeout (≥ 5 s) with PTT asserted.
   - `record_tx_frame` then still logs the frame as aired (#1334's premise holds only for a fresh
     stream per transmit).
4. **A: retaining a `cpal::Device` holds the card's capture PCM open.** ALSA's `DeviceHandles::open`
   opens both directions. After the first transmit, the receiver's `open_capture_stream(name)` gets
   EBUSY on an exclusive `hw:` device, so a single-card station goes deaf. The A probe counted
   devices only.

### Should fix

5. **The production path was never measured.** The probe ran without `PROBE_DEVICE`, and the default
   device skips enumeration on open. Pass 1 transfers to the named path. Pass 2 is an early-exit
   `find`, so "twice" is an upper bound. **Rerun with `PROBE_DEVICE` before quoting a per-turn cost.**
6. **The 16 → 9 device drop is misattributed.** The probe retains nothing across runs; opening a
   stream perturbs later enumeration. The B arm must count devices with the streams held.
7. **B: enumeration while both streams are open hides the card.** A reopen must drop the dead stream
   before it enumerates.
8. **B: cpal's `writei` retry loop can hang `Stream::drop`** on an unplug. Prove the reopen with a
   real USB unplug.
9. **The receive side pays the same enumeration twice per turn.**
   - `listen_for_ack` sets its deadline before it opens capture by name.
   - The RX tick reopens by name after every transmit.
   - Fixing TX alone moves the peer's ACK about 1 s earlier, into a listener that is still
     enumerating.
   - Fix the input side in the same change, or show the ACK window still brackets the ACK.
10. **The effective leader shrinks** from ~1.3 s to `ptt_leader_ms` (default 0) + ~0.1 s, inside
    the IC-9700's ~130 ms T/R sequencing. The change must ship with a measured leader default and a
    first-frames decode check.
11. **The 200 ms rule is a sanity bound, not a choice rule.** Missing arms:
    - idle for minutes, then write;
    - the capture stream held open;
    - per-frame drain marks under zero-fill;
    - device counts with the stream held.
12. **Option A′ is missing:**
    - cache the resolved **name** and skip pass 1;
    - or take pass-1 names from ALSA hints without opening a PCM per hint, since the cost is the
      per-hint `PCM::new`;
    - pass 2 still opens hints up to the match, so it is measured, not assumed.
13. **Option C's dismissal gives the weak reason.**
    - The strong one: opening before the key puts ~1 s between `csma_check` and PTT.
    - The CLI opens a fresh engine per command, so only C helps there.
    - The design should state which paths each option serves.
14. **Twins: the repeater is a consumer.** It transmits through its own `ModemEngine`. Under B, two
    engines would hold one exclusive device. Not Release 1.

### Park

- B's per-period zero-fill cost on a Pi.
- ALSA `play()` is a no-op.
- Holding playback blocks only other playback openers.
- JS8 raw audio and IQ go through the same seam.

## Disposition

Not implemented. The station probe was extended in this branch (`key_to_audio_by_stage`):
- pass 1;
- pass 2 alone (`find` the resolved name);
- production `open_output` by name;
- an optional held capture stream (`PROBE_INPUT_DEVICE`).

It is to be run with the station's configured name. Round 2 of the design follows from those numbers:
- A′, or hint-based names;
- the input-side fix;
- a measured leader default.

## Consumer (of the design reviewed)

`ModemEngine::stage_emit_output` (every audio TX seam) and its callers inside `keyed_transmit` in
`crates/openpulse-daemon/src/server.rs` (OTA send, ACKs, filexfer, beacons). Found by
`grep -rn "open_output(" crates apps --include=*.rs` and `grep -n "keyed_transmit" crates/openpulse-daemon/src/server.rs`.

## Prior art (of the design reviewed)

`docs/dev/design/drain-from-device-delay.md` (#1367, how release waits for the drain);
`select_cpal_device`'s two-pass enumeration and its truncation note; the persistent capture stream
the receiver already holds (`open_input` once per session). Found by
`grep -n "fn select_cpal_device\|fn open_input" crates/openpulse-audio/src/cpal_backend.rs`.

## Twins (of the design reviewed)

- The repeater's `open_output` wrapper (`crates/openpulse-repeater/src/lib.rs`).
- The testbench's signal path.
- linksim's GUI player.

These open their own outputs and are not on the Release 1 keyed path. Each is stated, not changed.
Found by the same `open_output(` grep.

---

# Round 2 (2026-10-12): cache the resolved name, never a handle

The adversarial review (Fable) of the round-2 section of
[`design/key-to-audio-gap.md`](../design/key-to-audio-gap.md). It was read-only, against the
station measurement of 2026-10-12 (rpi51, `plughw:CARD=CODEC,DEV=0`, capture held: pass 1
525–540 ms, pass 2 42–45 ms, open by name 567–684 ms, write → drained − audio 138–144 ms).

## Prompt (round 2)

Falsify round 2 with file:line in `cpal_backend.rs`, `engine.rs` (every `open_input`/`open_output`
site, `listen_for_ack`, `stage_emit_output`), `server.rs` (rx tick, `keyed_transmit`) and cpal
0.15's ALSA host:
1. Does pass 2's early-exit `find` avoid the PCM opens of devices before the match? Can pass 2 be as
   slow as pass 1?
2. Hotplug, rename, renumbering, two identical cards: is "evict and retry once" enough, and can it
   pick the wrong device silently?
3. Does every keyed path use the same selector string for the same device?
4. The `Mutex` across ALSA calls: deadlock or contention?
5. The input side against `listen_for_ack`'s deadline.
6. Is deferring the leader to a first-frames check safe, and what check is needed?
7. Can anything key longer or shorter, or play the wrong audio?
8. Does this block Release 1?

## Verdict (round 2)

**It does not block Release 1.** Round 2 removes round 1's four blocking defects, because every
open still builds and drops a fresh stream. It changes neither when PTT is asserted or released nor
what audio is written. Two claims in its reasoning are wrong (findings 1 and 2) and are corrected
before it is built. Nothing is blocking.

## Findings (round 2)

### Should fix

1. **"≈ 190 ms key-to-audio" is the wrong number.** Write → drained − audio runs from `write` to
   `flush` returning: it includes the ~32 ms silence pad, the device-reported drain plus
   `DRAIN_MARGIN`, and a 10 ms poll. It bounds the *end* of the stream, not the *first* sample, so it
   must not be set against the IC-9700's ~130 ms. Measure the first sample: timestamp the first
   callback that pops a non-zero sample, or read it off an SDR capture.
2. **The first-frames check cannot see a T/R clip where the design put it.** A hardware loopback has
   no T/R switch, and an equal decode ratio at 2 m SNR does not clear a clip that fails at HF SNR. The
   check belongs at G1, on the rig: SDR capture, PTT click to first audio, against the T/R click pair,
   and `ptt_leader_ms` is set from that.
3. **Pass 2 depends on hint order.** cpal opens a playback and a capture PCM per hint, and
   `output_devices()` runs an hw_params query per device, before the match. `find` drops each
   non-match, so nothing is held, but every earlier hint pays. 42 ms means the CODEC hint sits near
   the front; behind another card it grows by ~30 ms per hint, and a software-hint selector
   (`default`, `pulse`) costs as much as pass 1. State it, print the match's hint index in the probe,
   and use `host.devices()` in pass 2 (no per-device config query; the direction is already settled).
4. **The not-found fallback is dearer than today, inside the keyed window.** A vanished name runs
   pass 2 to exhaustion (≈ pass 1) and then the full resolve again, about twice today's error path.
   Fix: a failed `find` has already named every device, so feed those names to `resolve_name_from`.
5. **The ACK-window argument holds only with both stations on one build.** An old ISS and a new IRS
   lose ~0.5 s of margin: the ACK starts earlier into an ISS still enumerating. Say so (the A1/A2
   stations are rebuilt separately), and record ACK onset minus listener open at hardware loopback,
   not just "ACK decoded". `listen_for_ack` sets its deadline before the capture open.
6. **The `Mutex` is held for get and insert only**, never across the enumeration, and recovers from
   poisoning (`unwrap_or_else(|p| p.into_inner())`, as the file already does). No two threads share a
   backend on the Release 1 path today, so this is about not leaving a trap.

### Park

7. **Hotplug and wrong device:** no risk beyond `resolve_device`'s own. `CARD=` names survive
   renumbering, unplug evicts and errors as today, and two identical cards behave as the exact match
   does. A substring selector can keep an old pick where a fresh resolve would now say Ambiguous;
   log the cached name at `debug!` on every hit.
8. **`build_output_stream` failing on a hit** costs one extra pass 2, then fails as today. Our own
   previous stream cannot cause EBUSY: `Stream::drop` joins its worker.
9. **The cheaper alternative,** an empty `[audio] device` with ALSA's `default` pointed at the
   CODEC, skips enumeration with no code. It is operator config, and the Pi's HDMI cards make the
   default unsafe, so it is not the fix. The design says why.
10. **A hint-based pass 1** (`alsa::device_name::HintIter`, no PCM opens) is still the fuller fix.
    Pass 2 remains, so it does not remove finding 3.

**Checked and fine:** the cache key (every daemon and ARDOP path uses one selector per direction);
keys per direction are necessary; `AudioConfig` stays out of the key; PTT order and the #1367 drain
are unchanged; no handle is held, so round 1's capture-PCM finding is closed; holding the capture
stream hid nothing, consistent with `DeviceHandles::open`; diagnostics bypass the cache; the test
shape (add: a failed find feeds the fallback, and a miss after eviction caches the new name); the
CLI gains nothing, as the design says.

## Disposition (round 2)

The design text is revised for findings 1–6 and 9 (2026-10-12). Implementation follows in its own
PR. It goes ahead without a further review round: no finding is blocking, and the corrections
concern the claims and the error path, not the mechanism.
