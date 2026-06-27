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
  passband sized to the active mode's occupied bandwidth.

## Productionized (engine integration)

The receiver notch is now wired into `ModemEngine`, opt-in and off by default:

- `ModulationPlugin::occupied_bandwidth_hz(mode)` reports a mode's occupied bandwidth. Implemented
  for **every** modulation plugin: 2×baud for BPSK/QPSK/8PSK/64QAM/pilot, subcarrier-span for
  OFDM/SC-FDMA, 300 Hz for FSK4-ACK. The engine sizes the protected band `center ± bw/2` per
  captured block, with a configurable fallback when a mode can't report it.
- `ModemEngine::enable_notch()` / `disable_notch()` / `configure_notch(max, q, fallback_bw)`;
  applied in `stage_capture_input` so every receive path (incl. OTA/burst) is covered.
- Config: `[modem] notch_enabled` (default false), `notch_max`, `notch_q`, `notch_persistence`.
- **User controls**: a `Notch: ON/OFF` toggle on the operator panel, an `openpulse daemon
  set-notch <bool>` CLI command, and the `SetNotch { enabled }` control-protocol command.

### Persistence & QSY trigger

`NotchBank` persistence (opt-in via `set_persistence(n)` / `[modem] notch_persistence`)
distinguishes a genuine external interferer from the modem's own spectral lines by *occupancy*:
while the receiver's own wideband signal fills the protected band the block is skipped; a lone CW
tone (however loud) does not fill it, so it is tracked across blocks. A tone confirmed over `n`
such blocks is external — notched if out of band, or surfaced via
`ModemEngine::in_band_interferers()` (and a warn log) as a **QSY** candidate if in band, since a
notch can't remove it without harming the signal.

Gate tests (`crates/openpulse-modem/tests/notch_loopback.rs`):
`notch_recovers_decode_against_out_of_band_qrm` (off fails, on decodes through a strong tone) and
`persistence_surfaces_in_band_interferer_for_qsy`; plus `NotchBank` persistence unit tests.

### Auto-QSY on in-band interference

The QSY hint is now wired into `openpulse-qsy`. When notch persistence confirms an in-band
interferer, the daemon auto-initiates a QSY negotiation (opt-in via `[qsy]
auto_qsy_on_interference`, which needs `[modem] notch_enabled` + `notch_persistence > 0` and
`candidate_freqs_hz`):

- The notch + persistence `observe` now also run on the daemon's streaming capture path
  (`ModemEngine::accumulate_capture`, mode-threaded) — previously only the `receive()` family
  applied them, so the daemon never populated the hint.
- `maybe_qsy_on_interference()` (daemon main loop, every tick) reuses the standard initiator path
  (`QsySession::initiate` + `execute_qsy_actions`), so the peer responds over RF as usual. It
  self-gates on config / candidates / an in-flight session, and clears the hint
  (`ModemEngine::clear_in_band_interferers`) after firing so it doesn't re-trigger.

Gate tests: `auto_qsy_on_interference_initiates_session_and_transmits_req` and
`auto_qsy_noop_when_disabled_or_session_in_flight` (daemon).
