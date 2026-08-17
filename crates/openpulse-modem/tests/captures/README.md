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
| `ic9700-idle-wide-500hz-control.wav` | IC-9700 on `dc0sk-rpi51`, 144.600 MHz PKTUSB, 2026-08-17, 45 s, wide filter (measured −20 dB band **2470 Hz**), CAT-set via `rigctl -m 3081` | mean-square **1.36e-2**; BPSK250 peak ρ **0.227** over 45 s, 0.173 over its first 3 s | **The same-session control for the two narrow captures below.** Same rig, same level, same minute — the only variable is the filter width, which is what makes the narrow rows interpretable. |
| `ic9700-idle-500hz.wav` | Same session, **500 Hz filter** (measured −20 dB band **554 Hz**) | mean-square **5.09e-3**; BPSK250 peak ρ **0.413** over 45 s, 0.319 over 3 s | **The #1060 fixture.** ρ exceeds the shipped `PREAMBLE_RHO_THRESHOLD = 0.40` within an ordinary listening window, so the correlation veto stops discriminating for a narrow-filter station. Also the missing narrow-bandwidth noise column #1059 asks for. Carries this rig's conducted-RFI line at ~1651 Hz; notching it *raises* ρ to 0.433, so the filter is the mechanism, not the birdie. |
| `ic9700-idle-250hz.wav` | Same session, **250 Hz filter** (measured −20 dB band **309 Hz**) | mean-square **2.06e-3**; BPSK250 peak ρ **0.579** over 45 s, 0.505 over 3 s | The second point on the bandwidth axis: the ρ ceiling keeps climbing as the filter narrows, and at 250 Hz it exceeds 0.40 even in a 3 s listen. |
| `sdr-ic9700tx-bpsk250-rs-1.wav` | **SDR** (RSPdx, off-air, Antenna A, centre 144.600 MHz, fs 192 kHz, RFGR 22 / IFGR 40) receiving an **IC-9700** `BPSK250\|rs` transmission over 2 m at 5 W, payload `RFGAINFIX RS TEST`, 2026-07-30 | carrier **1513.1 Hz**, SNR ≈55.7 dB, burst t≈9.0–19.0 s | **The IC-9700's TRANSMIT chain, proven.** Recorded during the same keyed run whose FT-991A rig-audio capture FAILED. Decodes. Asserted by `the_ic9700_transmit_chain_decodes_off_air_from_an_independent_receiver`. |
| `sdr-ic9700tx-bpsk250-rs-2.wav` | Same SDR setup and link, payload `TRIMSIGN RS TEST`, B's dial at 144 599 936, 2026-07-30 | carrier **1512.2 Hz**, SNR ≈47.4 dB | Second of three. The **distinct payload is the anti-vacuity control** — a test decoding one fixed string could pass by returning a constant. |
| `sdr-ic9700tx-bpsk250-rs-3.wav` | Same SDR setup and link, payload `TRIMEMP RS TEST`, B's dial at 144 599 812, 2026-07-30 | carrier **1511.4 Hz**, SNR ≈53.1 dB | Third of three. Together they show the transmitted carrier is stable at 1511–1513 Hz while the FT-991A reported ~1372 Hz **regardless of its own verified dial**. |

## What the SDR captures settle that a rig capture cannot

Every other frame here was recorded from a *rig's* USB audio, which measures the transmitter and the
receiver **together** — when such a capture fails you cannot say which end is at fault. The three
`sdr-ic9700tx-*` files were recorded off-air by an independent receiver during the same keyed
transmissions whose rig-side captures failed, which makes the split a direct observation:

* SDR decodes **and** rig audio does not → the fault is in that rig's RECEIVE chain.

That is what closed the A→B question of 2026-07-30. Four keyed `BPSK250|rs` runs IC-9700 → FT-991A
all failed; the FT-991A's received carrier sat at ~1372 Hz **regardless of its dial**, verified by
raw CAT readback before and after each capture across a 128 Hz dial change that moved the measured
offset by ~0 Hz. The SDR heard the same transmissions at 1511–1513 Hz and decoded all three, in
0.5 s with no settle recovery at all. Transmitter exonerated, modem exonerated, one receiver
indicted — and no further on-air time needed to establish it.

One gain note worth keeping: at **RFGR 12 the RSPdx front end saturated** (peak > 1.0, ~1 % clipping)
and produced an undecodable 523–808 Hz smear that looks exactly like a modulation defect. RFGR 22
gives 0.00 % clipping and clean decodes. An overloaded SDR is not a neutral witness.

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
