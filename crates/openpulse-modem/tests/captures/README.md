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
| `ic9700-frame-bpsk250-rs-whitened.wav` | IC-9700 receiving an FT-991A `BPSK250\|rs` frame over 2 m at 5 W from a **whitening** build, payload `DUALCAP TEST 1`, 144.600 MHz, 2026-07-29 | burst t≈10.3–18.6 s (8.3 s), 7.2 dB above floor; carrier **1502.42 Hz** (+2.42, drift −0.3 Hz over the burst); spectral spread ≥0.481 in **every** window | **The artifact that closed #1021.** It arrived byte-perfect (0 errors in all 255 when diffed against the reconstructed transmitted wire) and still would not decode: the receiver had settled AFC on idle noise at sample 96 and the recovery re-settled there forever. Asserted by `the_real_on_air_frame_decodes`. |
| `ic9700-frame-bpsk250-none-whitened.wav` | Same link, same whitening build, minutes after the `rs` capture — `BPSK250\|none`, 144.600 MHz, 5 W, 2026-07-29 | 0.9 s burst, 7.3 dB above floor, carrier **1502.44 Hz** (+2.44) | **The control, and the corpus's former missing piece.** Holds margin, propagation, frequency and levels constant against the failing `rs` capture. It **decodes** (`DUALCAP TEST 1`) and is asserted by `a_real_on_air_frame_decodes_end_to_end` — the first end-to-end decode in this suite against audio a radio actually produced. |

## The gap is closed, and it closed a defect

A real modem frame now decodes end to end from recorded radio audio — both an uncoded one
(`ic9700-frame-bpsk250-none-whitened.wav`) and the **coded** one that defeated four rounds of on-air
debugging (`ic9700-frame-bpsk250-rs-whitened.wav`). That was the one thing neither the emulations nor
the earlier corpus could do.

Taking these two captures is what closed **#1021**, and not in the way anyone expected. The plan was
that whitening (a transmit-side fix) needed a fresh capture to demonstrate closure. The fresh coded
capture failed identically — which was the useful result, because it made the defect reproducible
offline. Diffing the demodulated bytes against the reconstructed transmitted wire then showed **0
errors in all 255**: the frame was perfect on the air, and the receiver was looking in the wrong
place. It had settled AFC on idle noise 82 000 samples early and its own recovery kept re-settling
on the same sample. See `the_real_on_air_frame_decodes` for the full account.

The lesson worth keeping: **every physical hypothesis was wrong** — frequency, level, margin,
fading, frame location were each measured and refuted — and the artifact is what made measuring them
cheap. The next defect of this class costs no radio, no second operator and no on-air window.
