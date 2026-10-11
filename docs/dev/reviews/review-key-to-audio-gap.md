---
project: openpulsehf
doc: docs/dev/reviews/review-key-to-audio-gap.md
status: resolved
last_updated: 2026-10-11
---

# Review: the key-to-audio gap (round 1, 2026-10-11)

This is the adversarial review (Fable) of [`design/key-to-audio-gap.md`](../design/key-to-audio-gap.md)
as first drafted. It was read-only. The review is mandatory, because the design changes the TX path
inside the keyed window and the PTT release timing (the #1367 drain).

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
