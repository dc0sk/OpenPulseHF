# Real on-air capture corpus

Recorded radio audio, replayed through the modem by `tests/capture_replay_corpus.rs`.

## Why this exists

The harness can emulate an idle noise floor, a capture level, a carrier offset, an AGC and a read
cadence — but each of those is a **model of a radio**, and a model can be wrong in exactly the way
that hides a bug. A recorded capture cannot: it is the signal a rig actually produced, with its real
noise floor, real level, real spurs and real offset.

The trade is coverage for fidelity. A capture only covers the conditions that were recorded, and it
goes stale as the DSP changes. So replay **complements** the emulations; it does not replace them.

## Format

8 kHz mono 16-bit PCM — the modem's working rate, and small enough to live in the repository.
Recorded at 48 kHz stereo from the rig's USB CODEC and decimated with a proper anti-aliasing filter
(`scipy.signal.resample_poly`); naive decimation would alias the noise floor and quietly change the
very property these files exist to preserve.

## Corpus

| file | provenance | measured | why it is here |
|---|---|---|---|
| `ic9700-idle-hot.wav` | IC-9700 on `dc0sk-rpi51`, 144.600 MHz PKTUSB, squelch open, PipeWire source volume **1.00**, 2026-07-28 | mean-square **≈0.0158** | The capture level that broke issue #1020: **4.9× the energy gate's 0.0032 ceiling**, so the gate could never shut, fired on noise, and settled AFC on it. This is the real floor, not a synthetic stand-in. |
| `ft991a-idle.wav` | FT-991A on `dd2zm-landline`, 144.600 MHz PKTUSB, source volume 1.00, 2026-07-28 | mean-square **≈3.7e-7** | The opposite failure: so quiet that a signal well above the noise still sits under the gate's **absolute** 1e-4 floor. Demonstrates that "set the level low" is not a universal fix. |
| `ic9700-tone-1501hz.wav` | IC-9700 receiving an FT-991A 1500 Hz tone over 2 m at 5 W, after the +64 Hz rig trim, 2026-07-28 | carrier **≈1501.5 Hz** | A real received signal with an independently measured carrier. Pins the measurement chain against a known on-air truth. |
| `ic9700-frame-bpsk250-rs.wav` | IC-9700 receiving an FT-991A `BPSK250\|rs` frame over 2 m at 5 W, payload `DUALCAP TEST 1`, 144.600 MHz, 2026-07-29 | burst at t≈10.5–18.8 s, 8.3 dB above floor; mean-square ≈0.00104 | **The open #1021 defect, captured.** The SDR recorded the same transmission independently and shows a correctly-formed 8.4 s burst, so the frame WAS on the air — yet replaying this audio through the modem fails with `RS correction failed at block 0: TooManyErrors`, exactly as on air. Receive-side, and now reproducible with no radio. |
| `ic9700-frame-bpsk250-none.wav` | Same link, same payload, minutes after the `rs` capture — `BPSK250\|none`, 144.600 MHz, 5 W, 2026-07-29 | 0.9 s burst, continuous modulation | **The control that isolated #1021.** Same rigs, levels and frequency as the failing `rs` capture, so margin and propagation are held constant. This one **decodes** (`DUALCAP TEST 1`); the only difference is frame structure. |

## Known gap — now partly filled

A real modem frame **has** been captured (`ic9700-frame-bpsk250-rs.wav`, 2026-07-29, via
`scripts/onair-dual-capture.sh`). It is a *failing* case: it reproduces the open #1021 defect
offline. A **passing** frame capture now exists too (`ic9700-frame-bpsk250-none.wav`), recorded minutes later
over the same link. The pair is what isolated the cause: with margin and propagation held constant,
the uncoded frame decodes and the coded one does not, and the only difference is that RS zero-pads a
small payload to a full 223-byte block — 195 bytes of zeros, which in differentially-encoded BPSK is
**6.2 seconds of unmodulated carrier** (predicted 6.24 s, measured 6.2 s). Timing recovery and the
carrier loop have no transitions to track across it.

To fill it, capture the receiver's audio during a transmission and save both the WAV and the payload
that was sent:

```bash
# on the receiving host, while the far station transmits a known payload
parecord --device=<rig CODEC source> --channels=2 --rate=48000 --format=s16le \
         --file-format=wav /tmp/frame-capture.wav
# then decimate to 8 kHz mono and add a row above with the mode, FEC and expected payload
```

A frame capture would let the suite assert an end-to-end **decode** against real audio, which is the
one thing neither the emulations nor the present corpus can do.
