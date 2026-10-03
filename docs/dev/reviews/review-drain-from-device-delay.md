---
project: openpulsehf
doc: docs/dev/reviews/review-drain-from-device-delay.md
status: resolved
last_updated: 2026-10-02
---

# Adversarial review — release PTT from the device's reported drain (#1367, M2)

One round by Fable, held before implementation. **Mandatory** (decision 8): the change moves PTT
release. The reviewer was read-only and was prompted for falsification. Target: the draft
[`design/drain-from-device-delay.md`](../design/drain-from-device-delay.md), with the vendored cpal
0.15.3 ALSA host as the evidence base.

## Consumer

`CpalOutputStream::flush` → every `PttKeyGuard` drop after a transmit. Found by
`grep -n "fn flush" crates/openpulse-audio/src/cpal_backend.rs`. The reviewer grepped `from_millis(200)`
outside the backend: only test sleeps.

## Prior art

`flush::flush_timeout_seconds` (#997), the 32 ms pad, cpal's `OutputStreamTimestamp`. Found by
`grep -n "flush_timeout_seconds\|trailing-silence" crates/openpulse-audio/src/*.rs`.

## Twins

None: no cpal IQ output stream; the loopback flushes return at once (`loopback.rs`).

## Prompt

Falsify, do not agree, with file:line evidence:
1. Is `playback − callback` on ALSA the right quantity, and is the per-buffer offset term right?
2. What does USB audio leave out of the delay, and is the margin enough?
3. Pulse/PipeWire plugins: a huge or zero delay? Are clocks ever mixed?
4. Concurrency between the callback and `flush`; staleness across frames; lazy `play()`.
5. Does anything else depend on the 200 ms?
6. Is there a simpler or safer alternative?
7. Twins.

Does this block Release 1? If not, say so.

## Verdict

**Release 1:** #1367 is M2 scope, but today's 200 ms is safe, just slow. Finding 1 had to be fixed
before merge.

| # | Finding | Class | Outcome |
|---|---|---|---|
| 1 | cpal clamps an underrun's negative delay to 0, so `duration_since` is `Some(0)`, never `None`; trusting it releases PTT with a whole hardware buffer playing | BLOCKING | A zero delay → the 200 ms fallback; unit test `a_zero_delay_is_not_trusted_and_falls_back_to_the_full_wait` |
| 2 | A separate mutex lets `flush` see "empty" before the mark is stored; later empty callbacks would overwrite it | FIX | The mark lives in the queue's mutex and latches on the non-empty → empty transition |
| 3 | The arithmetic is right: `delay` is read before this callback's write; all errors are late (safe) | NOTE | Kept |
| 4 | Lazy `play()` is a no-op on ALSA (it auto-starts on silence) | NOTE | Reinforces finding 2's latch rule; the misleading comment is noted |
| 5 | USB audio: the delay counts submitted URBs; only the codec FIFO is uncounted, about 1–2 ms | NOTE | The 10 ms margin holds |
| 6 | Nothing else depends on the 200 ms | NOTE | Recorded in Consumer |
| 7 | No simpler alternative: `BufferSize` is `Default` everywhere and cpal hides the PCM | NOTE | Kept |
| 8 | No twins | NOTE | Design corrected |
