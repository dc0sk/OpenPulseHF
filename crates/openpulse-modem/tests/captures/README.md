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
| `ic9700-idle-500hz.wav` | Same session, **500 Hz filter** (measured −20 dB band **554 Hz**) | mean-square **5.09e-3**; BPSK250 peak ρ **0.413** over 45 s, 0.319 over 3 s | **The #1060 fixture.** ρ exceeds the then-shipped `--++` threshold of 0.40 (now `ALT_PREAMBLE_RHO_THRESHOLD`; the PN-63 preamble's `PREAMBLE_RHO_THRESHOLD` is 0.315) within an ordinary listening window, so the correlation veto stops discriminating for a narrow-filter station. Also the missing narrow-bandwidth noise column #1059 asks for. Carries this rig's conducted-RFI line at ~1651 Hz; notching it *raises* ρ to 0.433, so the filter is the mechanism, not the birdie. |
| `ic9700-idle-250hz.wav` | Same session, **250 Hz filter** (measured −20 dB band **309 Hz**) | mean-square **2.06e-3**; BPSK250 peak ρ **0.579** over 45 s, 0.505 over 3 s | The second point on the bandwidth axis: the ρ ceiling keeps climbing as the filter narrows, and at 250 Hz it exceeds 0.40 even in a 3 s listen. |
| `sdr-ic9700tx-bpsk250-rs-1.wav` | **SDR** (RSPdx, off-air, Antenna A, centre 144.600 MHz, fs 192 kHz, RFGR 22 / IFGR 40) receiving an **IC-9700** `BPSK250\|rs` transmission over 2 m at 5 W, payload `RFGAINFIX RS TEST`, 2026-07-30 | carrier **1513.1 Hz**, SNR ≈55.7 dB, burst t≈9.0–19.0 s | **The IC-9700's TRANSMIT chain, proven.** Recorded during the same keyed run whose FT-991A rig-audio capture FAILED. Decodes. Asserted by `the_ic9700_transmit_chain_decodes_off_air_from_an_independent_receiver`. |
| `sdr-ic9700tx-bpsk250-rs-2.wav` | Same SDR setup and link, payload `TRIMSIGN RS TEST`, B's dial at 144 599 936, 2026-07-30 | carrier **1512.2 Hz**, SNR ≈47.4 dB | Second of three. The **distinct payload is the anti-vacuity control** — a test decoding one fixed string could pass by returning a constant. |
| `sdr-ic9700tx-bpsk250-rs-3.wav` | Same SDR setup and link, payload `TRIMEMP RS TEST`, B's dial at 144 599 812, 2026-07-30 | carrier **1511.4 Hz**, SNR ≈53.1 dB | Third of three. Together they show the transmitted carrier is stable at 1511–1513 Hz while the FT-991A reported ~1372 Hz **regardless of its own verified dial**. |
| `ft991a-ota-frame-bpsk31-rs.wav` | FT-991A (station B, the remote 2 m station) receiving an IC-9700 **OTA data frame**, `BPSK31\|rs` (SL2 of `robust`), 64-byte payload, 144.640 MHz with the FT-991A at OTHER SHIFT +1500 and dial +1564, recorded with `arecord -D pulse` beside its running daemon, 2026-10-31 (on-air task 12, run B2, `rx-b-b2-8k.wav`, t = 10–86 s). Decimated 48→8 kHz with `resample_poly(x, 1, 6)` | idle mean-square ≈2.7e-4 (first 5 s); carrier **1503.2 Hz** by the 2fc line (+3.2 Hz); the frame flushes as a 538 048-sample burst with a 2848-sample lead | **The OTA scan-order defect, captured.** The trigger settle lands at −12.8 Hz, so the pass-major scan spent 144 decode attempts (8.1 s x86) before decoding at onset 2560; onset-major spends 26. Asserted by `tests/ota_scan_onset_major.rs`; design `docs/dev/design/ota-scan-onset-major.md`. |
| `ft991a-ota-fragment-5s.wav` | FT-991A (station B) beside its daemon, 16 s around a 5.1 s burst it gathered between two IC-9700 SL2 OTA frames, 144.640 MHz, 2026-11-01 (on-air task 13, run b3, `rx-b-b3-8k.wav`, t = 236–252 s), decimated as above | idle rms ≈0.017, burst rms ≈0.06; flushes as a 40 448-sample burst | **A burst no coded candidate can fit.** Before the short-of-block guard it cost 1088 coded attempts (79 s x86) at SL2/SL3; with it and the fallback-rescue cap 133 attempts on a fresh receiver (511 without the cap, 888 with the guard disabled too). Asserted by `tests/ota_scan_onset_major.rs`. |
| `ft991a-ota-bpsk31-at-sl1.wav` | FT-991A (station B) beside its daemon, 32 s while the IC-9700 sent an SL2 `BPSK31\|rs` OTA frame and B's receiver had stepped down to SL1, 144.640 MHz, 2026-11-01 (on-air task 15, run b5, `rx-b-b5-8k.wav`, t = 146–178 s), decimated as above | signal rms ≈0.06, a 5 s silence while B transmits; in 2 s reads flushes as a 216 192-sample burst | **A burst MFSK16 cannot lock.** Before the bound the scan cost 171 attempts (41.7 s x86) at SL1; with it 69. Asserted by `tests/ota_scan_onset_major.rs`. |
| `ic9700-ofdm52-f2g-agc-1.wav` | IC-9700 (station A, AGC FAST) beside its daemon, receiving FT-991A (station B) OTA traffic on the `fast` profile, 144.640 MHz, 2026-11-06 (on-air task 27, run f2g), cut by sample index from `docs/dev/test-reports/on-air/rx-audio-2026-11-01/rx-a-f2g-8k.wav.gz`, samples 2 766 984–2 950 336 (345.873–368.792 s from the recording's start at 07:07:00.85 UTC, i.e. 07:12:46.72–07:13:09.64 UTC); no resampling | two SL7 (`OFDM52`, soft-concatenated) frames, preambles at 9.200 and 21.007 s into the file; each frame's total power ~1.15× the gap hiss (≈0.0053 RMS); ahead of the first, B's QPSK250-D frame (~2.4–6.5 s) and A's own two keyed ACKs (0.527–1.527 s, 7.427–8.427 s, the rig's muted level) | **The case for the OFDM preamble detect** (`docs/dev/design/ofdm-preamble-detect.md`). Under the AGC neither total power nor the spectral test sees these frames; A gathered none on air. Asserted by `tests/ofdm_preamble_detect.rs` (gates 1, 2, 6) and `tests/ofdm_detect_fade.rs`. A's own-ACK intervals are the keyed intervals of task 27's analysis of the run; the tests cut them out as the daemon drops its stream there. |
| `ic9700-ofdm52-f2g-agc-2.wav` | As above, samples 3 752 288–3 935 320 (469.036–491.915 s; 07:14:49.89–07:15:12.77 UTC) | SL7 preambles at 9.200 and 20.967 s; A's own ACKs at 0.514–1.614 and 7.564–7.864 s | Second of three, same purpose. |
| `ic9700-ofdm52-f2g-agc-3.wav` | As above, samples 5 427 432–5 610 816 (678.429–701.352 s; 07:18:19.28–07:18:42.20 UTC) | SL7 preambles at 9.200 and 21.011 s; A's own ACKs at 0.321–1.371 and 7.121–8.171 s | Third of three. |
| `ft991a-ofdm52-f1d-sl8-sl9.wav` | FT-991A (station B, AGC FAST) beside its daemon, receiving IC-9700 OTA traffic, run f1d, cut from `rx-b-f1d-8k.wav.gz`, 2026-11-05, samples 2 759 200–2 823 200 (344.900–352.900 s from the recording's start at 16:40:46.70 UTC per `docs/dev/test-reports/onair-twin-ota-20261105-174133-evidence/NOTES.md`, i.e. 16:46:31.60–16:46:39.60 UTC), no resampling | an SL8 (`OFDM52-8PSK`) frame at 1.833 s and an SL9 (`OFDM52-16QAM`) frame at 5.058 s into the file, ~2× the hiss; B's own transmissions are digital silence | **A second rig** for the preamble detect: loud enough that total power gathers it, so P must cost it nothing. Asserted by `ofdm_frames_gather_on_the_second_rig`. |
| `ic9700-ofdm52-f2i-sl9-undecodable.wav` | IC-9700 (station A, AGC FAST) beside its daemon, receiving FT-991A (station B) OTA traffic on the `fast` profile with the preamble detect (#1518), 144.640 MHz, 2026-11-06 (on-air task 29, run f2i), cut by sample index from `rx-a-f2i-8k.wav.gz` (committed on `m3/onair` in 7e1dd5f0, `docs/dev/test-reports/on-air/rx-audio-2026-11-01/`), samples 3 976 000–4 004 500 (497.000–500.5625 s from the recording's start at ≈17:05:16.6 UTC, i.e. ≈17:13:33.6–17:13:37.2 UTC), no resampling | 2 s of the recording's own hiss, then from 499.00 s an SL9 (`OFDM52-16QAM`) frame at the gap's level (rms ≈0.006 throughout); `acquire` puts it at **−7.8 Hz**, coherence (refined) 0.915 at every told; at the coarse grid point 0.871 at told 0, 0.915 at −6.7, 0.822–0.917 across tolds. It did not decode on air or offline at any told | **A genuine frame the decoder cannot decode**: P judged it at told 0 with A's AFC at −6.7 Hz and the coarse coherence, read it under the 0.9 evidence threshold, and A keyed no NACK (22 such frames in f2i). Asserted by the in-crate `preamble_detect_told` tests (`engine.rs`). The 2 s lead is there because with the frame's window alone (from 499.00 s) the burst never flushed inside the file; with 1 s or more of the hiss ahead it flushes as one 9 087-sample burst. |

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

## What the SDR captures say about key-up and release (measured 2026-10-02)

Read for #1257 (PTT leader) and #1367 (drain before release), from a 5 ms RMS envelope of each
`sdr-ic9700tx-*` file:

| capture | clicks before the frame (ms before first sample) | frame length |
|---|---|---|
| 1 | −1 305, −1 175, −155 | 8.319 s |
| 2 | −1 350, −1 220, −195 | 8.319 s |
| 3 | −1 335, −1 210, −55 | 8.319 s |

- Each event is a 5–45 ms click, 13–24 dB above the noise floor. Between the clicks the floor does
  not move: a keyed SSB rig with no audio radiates nothing the SDR can see.
- The frame then starts at full level within a millisecond, with no ALC ramp.
- **Reading, UNCHECKED** (no PTT timestamp was logged on that run):
  - the pair of clicks 130 ms apart is PTT assert and the IC-9700's T/R sequencing;
  - the late click is the audio stream opening.

  If that is right, the rig was keyed about 1.3 s before its first audio sample on every frame.
  The source supports it: every keyed path keys first and then modulates inside the guard
  (`keyed_transmit` in the daemon, `commands/transmit.rs` in the CLI, `bridge.rs` in ARDOP).
- **What this cannot settle.**
  - Key-up: with 1.3 s of lead, any key-up of 1.3 s or less is hidden. These files bound it from
    above only.
  - Release: nothing is visible after the last sample, so the drain-to-release interval #1367 asks
    about cannot be read from them.

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

## Wire-format epoch (added 2026-08-21, #1148)

The `*-whitened.wav` frame captures were recorded against the **pre-#1148 wire format**, whose
whitening LFSR ran at a 21-bit period (the taps implemented `x⁹+x⁸+x⁴`, not the documented
`x⁹+x⁵+1`). They do **not** decode against the current format, and the four tests that decode them
are `#[ignore]`d until the corpus is re-recorded:

* `a_real_on_air_frame_decodes_end_to_end`
* `the_real_on_air_frame_decodes`
* `the_ic9700_transmit_chain_decodes_off_air_from_an_independent_receiver`
* `the_settle_recovery_reaches_the_frame_without_crawling`

They also carry the **pre-#1062 preamble**. The #1062 flag day landed on 2026-10-10: BPSK63, BPSK100
and BPSK250 now send a PN-63 preamble (BPSK31 keeps `--++`), so these BPSK250 frames are two wire
formats behind. The re-record is item **F8** of work plan milestone M3, on the 2 m pair
(rpi51 + IC-9700, rpi53 + FT-818); see `docs/dev/project/workplan.md`.

The ρ figures in the corpus table were measured against the `--++` BPSK250 template, before the
flag day. Read them as the record that motivated #1062, not as the current template's numbers.

**Do not delete these files.** The corpus-integrity gates still run against them (levels, carrier
placement, sample rate), and the idle and tone captures carry no wire content at all — they are
unaffected by any format change and still back the #1060 ρ work, the squelch/noise-floor gates and
every "synthesize a frame inside real recorded noise" test, all of which stay green.

The FT-991A captures come from a rig that is no longer in the 2 m station pair (2026-10-02); they
stay as recorded evidence.

Any `#[ignore = "verification"]` harness that prints decode or condemnation columns from these
frame captures (G9 in `preamble_veto_interference.rs`, `snr_lead_in_bias`, the H1/H2 probes in
`sweep_cycle_idempotence`) is measuring the old format and its output should be read as historical
until the re-record.
