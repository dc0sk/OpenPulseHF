# Adaptive equalizer & automatic notch — advantage experiment

**Question.** Would adding (a) an adaptive equalizer and (b) an automatic multi-notch filter
(up to 10 notches) buy us anything, measured by `openpulse-linksim`'s effective two-way
throughput?

## Adaptive equalizer — already present, no new gain

We already ship `openpulse_dsp::equalizer::LmsEqualizer` (LMS forward taps + DFE), wired into
every coherent single-carrier demod:

- BPSK — `plugins/bpsk/src/demodulate.rs` (frozen-DD; 250-baud modes add a 2-tap DFE)
- QPSK — `plugins/qpsk/src/demodulate.rs` (adaptive DD)
- 8PSK — `plugins/psk8/src/demodulate.rs` (adaptive DD)

OFDM and SC-FDMA do per-subcarrier **ZF / MMSE** equalization in the frequency domain
(`plugins/ofdm/src/channel.rs`, `plugins/scfdma/src/channel.rs`); a time-domain LMS there is
redundant and fights the pilot-based estimator. The only frequency-selective channel linksim
exposes (Watterson F1) is therefore already equalized on the modes that run it. **Adding a
second equalizer wins nothing measurable**; the only lever is *tuning* the existing one.

## Automatic notch — a clear win against out-of-band QRM, with one hard constraint

No notch existed. We added `openpulse_dsp::notch::NotchBank`: FFT prominence detection of up to
N narrowband CW interferers + a cascade of RBJ IIR notch biquads. linksim gained a
`ChannelSpec::Qrm { snr_floor_db, tones }` channel and a receiver-side `LinkNotch` (off / auto /
oracle) on the forward path.

Reproduce:

```
cargo run -p openpulse-linksim --no-default-features --example notch_experiment
```

### Results (24 frames × 200 B, RS FEC, 20 dB noise floor)

| scenario | off | auto | oracle | oracle vs off |
|---|---|---|---|---|
| RRC ladder, **out-of-band** tone @2900 Hz | 726 bps (avgSL 9.2) | **955 (12.0)** | **955 (12.0)** | **+32 %** |
| rectangular QPSK500, out-of-band tone @800 Hz | 502 (8.0) | 500 (8.6) | 676 (9.0) | +35 % |
| RRC ladder, **in-band** tone @1800 Hz | 0 % deliver | 0 % | 4 % | n/a |

### Findings

1. **Out-of-band QRM: the notch is worth it.** Removing an interfering carrier just outside the
   occupied band lifts effective goodput ~30–35 % and lets the rate ladder climb ~3 speed
   levels (SL9 → SL12).

2. **With the receiver's own band protected, blind `auto` matches the `oracle` exactly** on a
   band-limited (RRC) signal — full benefit with no knowledge of the interferer's frequency.

3. **In-band QRM cannot be notched.** A tone inside the signal's occupied band destroys the link
   (0 % delivered), and notching it removes the signal too (oracle barely 4 %). That is a
   **frequency-agility (QSY)** case, not a notch case.

4. **The load-bearing constraint: protect the receiver's occupied band.** A modem signal is not
   spectrally flat — rectangular-pulse modes have sidelobe combs spaced at baud/2 (50–72 dB
   prominent), and even RRC modes have preamble periodicity lines. A naive blind per-frame FFT
   notches these *own-signal* lines and roughly halves throughput (the rectangular-QPSK500 row:
   `auto` ≈ `off` because the narrow 1100–1900 Hz protect band left the combs exposed). Detection
   must never touch the band centred on the receiver's own carrier (`NotchParams::protect_lo_hz`
   / `protect_hi_hz`).

## Recommendation

- **Equalizer:** nothing to add. Optionally revisit per-mode LMS tuning separately.
- **Notch:** worth productionizing **for out-of-band interference**, gated on a protected
  passband sized to the active mode's occupied bandwidth. Open design choices before wiring it
  into the live demod path: (a) source the protect band from the running mode's bandwidth;
  (b) consider temporal-persistence detection (a CW interferer persists across frames; own-signal
  data lines do not) so protection can be relaxed; (c) in-band interference should trigger QSY,
  not a notch.
