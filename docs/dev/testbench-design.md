---
project: openpulsehf
doc: docs/testbench-design.md
status: living
last_updated: 2026-05-02
---

# Testbench Design

This document specifies the design and architecture of `openpulse-testbench`, a local Linux GUI application for performance and regression testing of OpenPulseHF modulation modes under realistic HF channel conditions.

## Purpose and scope

The testbench provides:

- Real-time spectrum and waterfall visualisation for four signal paths simultaneously: TX (clean), Noise (isolated), Mixed (TX + noise, what the receiver sees), and RX (reconstructed from decoded output).
- A configurable noise injection pipeline supporting QRN, QRM, QSB, chirp, AWGN, and composite channel models, each with both named profiles and free-form parameter sliders.
- Reproducible and random noise runs via an explicit seed parameter.
- Regression testing across modulation modes (BPSK31/63/100/250, QPSK125/250/500), FEC codecs, speed-reduction ladders, and fallback modulation paths.
- Integration with the channel models defined in `docs/benchmark-harness.md` (Watterson and Gilbert-Elliott parameter sets).

The testbench runs entirely locally on Linux. It does not require a real radio or soundcard; it uses an in-process audio tap architecture. Optional integration with the ALSA `snd-aloop` virtual loopback driver is planned for a later phase (see Phase E in the implementation plan).

---

## Related research

The following analysis documents inform testbench design decisions, in particular the SNR sweep range, channel model selection, and regression test mode design. They are maintained separately from this document and should be consulted for the underlying rationale behind parameter choices made here.

- `docs/wsjtx-analysis.md` — WSJTX weak-signal techniques: LDPC codes, Costas array synchronisation, 40th-percentile noise floor estimation, multi-pass decoding, SNR thresholds for FT8/JT65/Q65
- `docs/js8call-analysis.md` — JS8Call speed ladder parameters (source-derived), ARQ protocol commands, store-and-forward relay design, SNR floor comparison between JS8 modes and HPX modes
- `docs/vara-research.md` — VARA HF OFDM structure, 11-level adaptive rate ladder, ACK taxonomy, Turbo FEC, KISS/TCP interface, single-carrier vs OFDM comparison
- `docs/pactor-research.md` — PACTOR Memory-ARQ, 1.25 s ARQ cycle, concatenated convolutional FEC, CAZAC training sequences, stride-based block interleaver

---

## UI framework: egui/eframe

The testbench uses **egui** (immediate-mode GUI) via **eframe** rather than iced.

**Reason**: The waterfall display writes a new row of FFT data (~512 bins) every 100–200 ms. egui's immediate-mode model updates a GPU texture with a single `TextureHandle::set()` call per frame. iced's Elm architecture diffs retained widget trees on every message; a 10 fps waterfall at 512 bins generates approximately one million float comparisons per second as overhead with no signal-processing value. egui is strictly better for real-time continuous-data visualisation.

egui's `egui_plot` crate provides `Plot::new()` + `Line::new()` for spectrum panels. The input format (`Vec<[f64; 2]>`) is produced directly from FFT output by pairing bin index (as Hz) with power (as dB). No adapter layer is required.

---

## Architecture overview

### Crate layout

Two new crates are introduced:

```
crates/openpulse-channel/     channel simulation library (reusable by benchmark harness)
apps/openpulse-testbench/     GUI application binary
```

`openpulse-channel` is placed in `crates/` because Phase 1.4 of the roadmap requires channel models in the benchmark harness; this crate directly fills that item and will be consumed by `openpulse-modem` independently of the testbench.

### Four-tap signal path

The testbench drives the signal pipeline directly through `ModulationPlugin` and `ChannelModel` traits, bypassing `ModemEngine`. This gives access to all four intermediate signal points that `ModemEngine` correctly hides from callers.

```
input payload (bytes)
   │
   ▼
ModulationPlugin::modulate()     ──────────────────────► [TX tap]
   │
   ▼
ChannelModel::generate_noise()   ──────────────────────► [Noise tap]
   │
   ▼
sample_add(tx, noise)            ──────────────────────► [Mixed tap]
   │
   ▼
ModulationPlugin::demodulate()
   │
   ├─ Ok(decoded_bytes)
   │    └─ ModulationPlugin::modulate(decoded_bytes)  ─► [RX tap]
   │
   └─ Err(_)
        └─ vec![0.0; tx_samples.len()]               ─► [RX tap]  ← flat noise floor when decode fails
```

The difference between TX and RX spectra makes channel degradation directly visible. When SNR is too low to decode, the RX panel shows a flat noise floor; as SNR increases toward the mode's operating threshold, the RX spectrum recovers to match TX.

Each tap is an `Arc<RwLock<WaterfallBuffer>>`. The background signal thread writes at approximately 10 Hz. The egui render thread reads at 30 fps. `RwLock` is used because the render thread is read-only and must never block the signal thread.

### Background thread lifetime

The signal thread is created on Run and destroyed on Stop. Each Run press creates a fresh thread with the current configuration and seed, ensuring that reproducible runs are strictly identical. The signal thread communicates via a `crossbeam_channel::Receiver<RunCommand>` for Stop signals and writes statistics back via an `Arc<RwLock<TestStats>>`.

---

## Channel simulation: `crates/openpulse-channel`

### Core trait

```rust
pub trait ChannelModel: Send {
    /// Apply the full channel (signal distortion + additive noise) to a signal block.
    fn apply(&mut self, input: &[f32]) -> Vec<f32>;

    /// Generate the additive noise component alone, without input signal.
    /// Used to populate the standalone Noise tap for visualisation.
    fn generate_noise(&mut self, length: usize) -> Vec<f32>;
}
```

`generate_noise` is separate from `apply` so that multiplicative distortions (QSB fading, Watterson multipath) contribute to the Mixed tap via `apply` but show no independent contribution to the Noise tap. The Noise tap visualises only the additive interference injected into the path.

### Reproducibility

All channel model constructors accept `seed: Option<u64>`.

- `Some(seed)`: uses `rand::rngs::StdRng::seed_from_u64(seed)`. Behaviour is identical across runs and platforms.
- `None`: uses `rand::thread_rng()`. Each run produces different noise.

Each channel model stores its own `Box<dyn RngCore + Send>` derived from the top-level seed via `SeedableRng::from_rng(&mut parent_rng)`. Different models receive independent but deterministic RNG streams from a single top-level seed. The `CompositeChannel` allocates sub-RNGs in construction order, so adding or removing a model changes subsequent models' streams; this is expected and documented.

### Configuration types

All channel models expose a `Config` struct that is `serde::Serialize + Deserialize`. The `ChannelModelConfig` enum wraps all variants and supports saving/loading test configurations to JSON.

```rust
pub enum ChannelModelConfig {
    Awgn(AwgnConfig),
    GilbertElliott(GilbertElliottConfig),
    Watterson(WattersonConfig),
    Qrn(QrnConfig),
    Qrm(QrmConfig),
    Qsb(QsbConfig),
    Chirp(ChirpConfig),
    Composite(Vec<ChannelModelConfig>),
}

pub fn build_channel(config: &ChannelModelConfig, seed: Option<u64>) -> Box<dyn ChannelModel>;
```

---

## Channel models

Each model supports two modes of configuration in the UI: **named profile** (a preset from a drop-down) and **parameter sliders** (full free-form control). Selecting a named profile populates the sliders with its values; sliders can then be adjusted independently. This avoids two separate configuration paths in the code — named profiles are simply `Config` values with memorable names.

### AWGN

Standard additive white Gaussian noise.

**Behaviour**: measures the RMS of the input block, computes the required noise standard deviation for the configured SNR, draws samples from `Normal(0, σ_noise)` using `rand_distr::Normal<f32>`.

For `generate_noise` (standalone Noise tap): noise is generated at the configured absolute power level rather than relative to the signal, since there is no signal to be relative to.

**Config struct**:

| Field | Type | Description |
|-------|------|-------------|
| `snr_db` | `f32` | Signal-to-noise ratio in dB |

**Named profiles**:

| Profile name | `snr_db` |
|---|---|
| Strong (20 dB) | 20.0 |
| Good (15 dB) | 15.0 |
| Moderate (10 dB) | 10.0 |
| Weak (5 dB) | 5.0 |
| Near-floor (0 dB) | 0.0 |
| Below floor (−10 dB) | −10.0 |

---

### Gilbert-Elliott burst error model

Two-state Markov model for burst interference. See `docs/benchmark-harness.md` for the full model definition. The Good state injects low-level AWGN; the Bad state injects high-level AWGN.

**Behaviour**: maintains a state variable (Good/Bad). On each sample, draws a uniform random value and transitions states with the configured probabilities. In the current state, generates a noise sample from the corresponding AWGN at `snr_good_db` (Good) or `snr_bad_db` (Bad).

**Config struct**:

| Field | Type | Description |
|-------|------|-------------|
| `p_g` | `f32` | Bit error probability in Good state (typically 0.001) |
| `p_b` | `f32` | Bit error probability in Bad state (typically 0.1–0.8) |
| `p_gb` | `f32` | Transition probability Good → Bad per symbol |
| `p_bg` | `f32` | Transition probability Bad → Good per symbol |
| `snr_good_db` | `f32` | Background AWGN SNR in Good state (default: 20.0 dB) |
| `snr_bad_db` | `f32` | AWGN SNR in Bad state (default: 3.0 dB) |

**Named profiles** (directly from `docs/benchmark-harness.md`):

| Profile name | `p_g` | `p_b` | `p_gb` | `p_bg` | Mean burst (symbols) | Mean gap (symbols) |
|---|---|---|---|---|---|---|
| Light burst | 0.001 | 0.1 | 0.01 | 0.1 | 10 | 100 |
| Moderate burst | 0.001 | 0.2 | 0.05 | 0.05 | 20 | 20 |
| Heavy burst | 0.001 | 0.5 | 0.05 | 0.02 | 50 | 20 |
| Severe burst | 0.001 | 0.8 | 0.1 | 0.01 | 100 | 10 |

---

### Watterson HF channel model

ITU-R F.1487 two-ray ionospheric channel model. See `docs/benchmark-harness.md` for the full model definition.

**Behaviour**: The channel produces a fading multipath signal by summing two independently fading signal replicas. Each replica's complex fading envelope is generated by:

1. Generate a block of complex Gaussian samples (i.i.d. ℂN(0,1)).
2. Apply a Gaussian-shaped spectral shaping filter in the frequency domain with one-sided bandwidth equal to `doppler_spread_hz`. This requires one forward FFT, a pointwise multiply, and one inverse FFT per block (using `rustfft`).
3. The result is a complex fading envelope `h(t)` with the correct Doppler spread.

Ray 2 is delayed by `round(delay_spread_ms × sample_rate / 1000.0)` samples via a ring buffer. The output sample is `Re[s(t) × h1(t) + s(t − τ) × h2(t)]` plus AWGN at `snr_db`.

**Config struct**:

| Field | Type | Description |
|-------|------|-------------|
| `doppler_spread_hz` | `f32` | One-sided Gaussian Doppler spread per ray (Hz) |
| `delay_spread_ms` | `f32` | Differential delay between the two rays (ms) |
| `snr_db` | `f32` | Background AWGN SNR at receiver input (dB) |
| `snr_variation_db` | `f32` | Optional ±random SNR variation per block (0 = disabled) |

**Named profiles** (directly from `docs/benchmark-harness.md`):

| Profile name | `doppler_spread_hz` | `delay_spread_ms` | Typical `snr_db` |
|---|---|---|---|
| AWGN (baseline) | 0.0 | 0.0 | 15.0 |
| Good F1 | 0.1 | 0.5 | 20.0 |
| Good F2 | 0.5 | 1.0 | 18.0 |
| Moderate M1 | 1.0 | 1.0 | 12.0 |
| Moderate M2 | 1.0 | 2.0 | 10.0 |
| Poor P1 | 1.0 | 2.0 | 5.0 |
| Poor P2 | 2.0 | 4.0 | 3.0 |

---

### QRN — Atmospheric noise

Models impulsive atmospheric noise (lightning static and ionospheric crackle). Based on a Middleton Class A approximation: a mixture of background Gaussian noise plus Poisson-distributed impulsive spikes.

**Behaviour**: each sample is the sum of:
- Background Gaussian noise at `gaussian_snr_db`.
- With probability `impulse_rate / sample_rate` per sample: a spike of duration 1–3 samples drawn from `Uniform(1..=3)`, with amplitude `impulse_amplitude_ratio × RMS_signal`.

Spikes are inserted into a carry-over buffer so multi-sample spikes extend correctly across block boundaries.

**Config struct**:

| Field | Type | Description | Default |
|-------|------|-------------|---------|
| `gaussian_snr_db` | `f32` | Background Gaussian noise SNR | 20.0 |
| `impulse_rate_hz` | `f32` | Mean spike rate (spikes per second) | 5.0 |
| `impulse_amplitude_ratio` | `f32` | Spike amplitude as multiple of signal RMS | 8.0 |
| `max_spike_duration_samples` | `u8` | Maximum spike width in samples | 3 |

**Named profiles**:

| Profile name | `gaussian_snr_db` | `impulse_rate_hz` | `impulse_amplitude_ratio` | Notes |
|---|---|---|---|---|
| Quiet (low QRN) | 25.0 | 1.0 | 5.0 | Daytime summer, mid-latitude |
| Moderate QRN | 18.0 | 5.0 | 8.0 | Typical HF band activity |
| Heavy QRN | 10.0 | 20.0 | 12.0 | Nighttime tropical path |
| Severe QRN | 3.0 | 50.0 | 20.0 | Storm path; near-unusable |

---

### QRM — Man-made interference

Models deliberate or accidental narrow-band interference: carrier tones, nearby digital mode signals, broadcast splatter.

**Behaviour**: maintains a list of sine wave oscillators, one per configured tone. Each oscillator maintains a phase accumulator, updated as `φ += 2π × freq × N / sample_rate` after each block of N samples. The generated block is the sum of all oscillator outputs. Phase coherence is preserved across block boundaries.

For `generate_noise`, the tone block is returned directly. For `apply`, the tones are added to the (optionally otherwise-processed) input.

**Config struct**:

| Field | Type | Description |
|-------|------|-------------|
| `tones` | `Vec<ToneConfig>` | List of interfering tones |
| `noise_floor_snr_db` | `f32` | Optional broadband noise floor under the tones |

**`ToneConfig`**:

| Field | Type | Description |
|-------|------|-------------|
| `frequency_hz` | `f32` | Tone frequency |
| `amplitude` | `f32` | Linear amplitude (1.0 = signal RMS) |
| `bandwidth_hz` | `f32` | If > 0, tone is modulated as narrow-band noise with this bandwidth |

**Named profiles**:

| Profile name | Description | Tones |
|---|---|---|
| Single carrier | One unmodulated carrier | 1500 Hz, amplitude 1.0 |
| PSK31 interference | Adjacent BPSK31 station | 1600 Hz, bandwidth 62 Hz, amplitude 0.5 |
| Two carriers | Two carriers, same amplitude | 1200 Hz + 1800 Hz, amplitude 0.8 each |
| Broadcast splatter | Multiple broadband noise clusters | 1000 Hz bw=300, 2000 Hz bw=300, amplitude 0.3 each |
| RTTY interference | Two FSK tones | 1615 Hz + 1785 Hz, amplitude 0.6 each |

---

### QSB — Propagation fading

Models slow amplitude fading of the signal path. QSB is multiplicative, not additive: the signal is amplitude-modulated by a slowly varying envelope. It therefore contributes to the Mixed tap via `apply` but contributes nothing to the standalone Noise tap.

**Behaviour**: `apply` multiplies each sample by `envelope(t) = 1.0 − depth × 0.5 × (1.0 − cos(2π × fade_rate_hz × t + phase_offset))`, where `phase_offset` is uniformly randomised at construction. The envelope oscillates between `1.0 − depth` (fade null) and `1.0` (peak).

For `generate_noise`: returns `vec![0.0; length]`. QSB does not inject noise; its effect is visible in the Mixed and RX panels as time-varying signal strength.

**Config struct**:

| Field | Type | Description | Range |
|-------|------|-------------|-------|
| `fade_rate_hz` | `f32` | Fading cycle frequency | 0.01–2.0 Hz |
| `fade_depth` | `f32` | Fractional depth of fade (0 = no fading, 1 = full null) | 0.0–0.95 |

**Named profiles**:

| Profile name | `fade_rate_hz` | `fade_depth` | Notes |
|---|---|---|---|
| Slow shallow | 0.05 | 0.3 | Long-path slow drift |
| Slow deep | 0.05 | 0.8 | Long-path deep selective fade |
| Moderate | 0.3 | 0.5 | Typical HF fade rate |
| Fast deep | 1.0 | 0.85 | Near-local scatter path; rapid deep fading |

---

### Chirp interference

Models swept-frequency interference (ionospheric sounder backscatter, radar sweep, broadband noise sweep).

**Behaviour**: generates a linear frequency sweep: `s(t) = amplitude × sin(φ(t))` where `φ(t)` is updated as `φ += 2π × f(t) / sample_rate` with `f(t) = f_start + (f_end − f_start) × mod(t, period_s) / period_s`. The chirp wraps at `period_s` and restarts from `f_start`. Phase continuity is maintained across block boundaries via a persistent phase accumulator.

**Config struct**:

| Field | Type | Description |
|-------|------|-------------|
| `f_start_hz` | `f32` | Start frequency of each sweep (Hz) |
| `f_end_hz` | `f32` | End frequency of each sweep (Hz) |
| `period_s` | `f32` | Duration of one complete sweep (seconds) |
| `amplitude` | `f32` | Linear amplitude (1.0 = signal RMS) |

**Named profiles**:

| Profile name | `f_start_hz` | `f_end_hz` | `period_s` | `amplitude` | Notes |
|---|---|---|---|---|---|
| Slow narrow | 1400.0 | 1600.0 | 10.0 | 0.3 | Slow narrowband sweep |
| Fast wideband | 300.0 | 3000.0 | 2.0 | 0.5 | Ionospheric sounder-style |
| Descending | 2000.0 | 500.0 | 5.0 | 0.4 | Descending sweep |
| Rapid burst | 800.0 | 2200.0 | 0.5 | 0.8 | Short fast sweeps; near-impulse |

---

### Composite channel

`CompositeChannel` wraps a `Vec<Box<dyn ChannelModel>>` and pipes through all models in series.

- `apply`: passes the signal block through each model's `apply` in order.
- `generate_noise`: returns the **sum** of each model's `generate_noise` output, except for multiplicative models (QSB), which contribute zero to the noise-only path.

The Composite model is what the testbench uses for mixed-conditions scenarios (e.g. Watterson M1 + Gilbert-Elliott moderate burst, as specified in scenario HF500-BURST-03).

---

## DSP: power spectrum and waterfall

DSP functions live in `crates/openpulse-channel/src/dsp.rs` to make them reusable by the benchmark harness for SNR estimation and spectral analysis.

### Constants

At the project's standard sample rate of 8000 Hz:

| Constant | Value | Notes |
|---|---|---|
| `FFT_SIZE` | 1024 | Frequency resolution: 7.8 Hz/bin |
| `FREQ_BINS` | 512 | Positive frequencies: 0–4000 Hz |
| `WATERFALL_ROWS` | 200 | Depth of scrolling waterfall history |

### `PowerSpectrum`

Holds a cached `rustfft` plan and precomputed Hann window coefficients. The Hann window is `w[i] = 0.5 × (1.0 − cos(2π × i / (N − 1)))`. Window normalisation: coefficients are scaled so that `Σ w[i]² = N / 2` (power-normalised), ensuring that the measured power of a sine wave is independent of window position.

`compute(samples: &[f32]) -> Vec<f32>`:

1. Copy up to `FFT_SIZE` samples into a `Vec<Complex<f32>>`, zero-padding if shorter.
2. Multiply by the Hann window.
3. Run FFT in-place.
4. Return `Vec<f32>` of length `FREQ_BINS`: `power_db[k] = 10 × log10(|X[k]|² + 1e-10)`.

### `WaterfallBuffer`

```
pub struct WaterfallBuffer {
    rows:            VecDeque<Vec<f32>>,   // each row: FREQ_BINS dB values
    max_rows:        usize,                // = WATERFALL_ROWS
    latest_spectrum: Vec<f32>,             // most recent row, for spectrum panel
    generation:      u64,                  // incremented on each push; used by UI to detect updates
}
```

`push_samples(spectrum: &PowerSpectrum, samples: &[f32])`: computes the power spectrum of the given samples and appends a new row. If the sample block is longer than `FFT_SIZE`, it is segmented: one row is pushed per `FFT_SIZE` chunk. If shorter than `FFT_SIZE`, one row is pushed with zero-padding.

`to_rgba_flat(min_db: f32, max_db: f32) -> Vec<u8>`: maps dB values to RGBA pixels using the plasma colormap. The output is a flat RGBA byte array of dimensions `FREQ_BINS × WATERFALL_ROWS`, suitable for `egui::TextureHandle`. Row 0 is the oldest entry (top of display); row `WATERFALL_ROWS − 1` is the newest (bottom).

**Plasma colormap**: 256 RGBA entries mapping normalised [0.0, 1.0] to the matplotlib plasma palette. Stored as a `const` lookup table of 256 × 4 bytes. Normalised value = `(power_db − min_db) / (max_db − min_db)`, clamped to [0.0, 1.0].

---

## UI design

### Layout

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ TopPanel                                                                                  │
│ [▶ Run] [■ Stop]  Mode: [BPSK31     ▼]  Noise: [Composite ▼]  SNR: [15.0 dB ───────]    │
│                   FEC: [☐]             Seed:  [42      ] [☐ Random]  [⚙ Noise params]   │
├───────────────────┬───────────────────┬───────────────────┬───────────────────────────────┤
│  TX (clean)       │  Noise channel    │  Mixed (TX+noise) │  RX (decoded)                 │
│                   │                   │                   │                               │
│  spectrum         │  spectrum         │  spectrum         │  spectrum                     │
│  ───────────────  │  ───────────────  │  ───────────────  │  ───────────────              │
│  waterfall        │  waterfall        │  waterfall        │  waterfall                    │
│                   │                   │                   │                               │
├───────────────────┴───────────────────┴───────────────────┴───────────────────────────────┤
│ BottomPanel                                                                               │
│ Runs: 142   OK: 139   Fail: 3   BER: 0.0021   Throughput: ~248 bps   [Event log ▼]       │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

### TopPanel controls

| Control | Type | Description |
|---|---|---|
| Run / Stop | Button | Starts or stops the signal path background thread |
| Mode | ComboBox | BPSK31, BPSK63, BPSK100, BPSK250, QPSK125, QPSK250, QPSK500 |
| Noise | ComboBox | AWGN, Gilbert-Elliott, Watterson, QRN, QRM, QSB, Chirp, Composite |
| SNR | Slider | −30 to 30 dB; displayed to one decimal place |
| FEC | Checkbox | Enables Reed-Solomon FEC (ECC_LEN=32) on the payload |
| Seed | TextEdit | Integer seed for reproducible noise (empty = random mode) |
| Random | Checkbox | Overrides seed with `rand::thread_rng()` |
| Noise params | Button | Opens the noise parameter panel (collapsible or floating window) |

### Noise parameter panel

Opened via the "⚙ Noise params" button. Contains:

1. **Profile selector**: drop-down of named profiles for the selected noise model. Selecting a profile populates all sliders below with the profile's values. Sliders remain independently adjustable after profile selection.
2. **Parameter sliders**: one slider per `Config` field. Ranges and step sizes are fixed per field as described in each model's Config section above.
3. **Composite builder**: when Composite mode is selected, a list of enabled sub-models with a checkbox for each. Each enabled sub-model has its own profile selector and sliders in a collapsible section.

### Signal path panels

Each of the four panels contains:

- **Label**: "TX (clean)", "Noise channel", "Mixed (TX+noise)", "RX (decoded)".
- **Spectrum**: `egui_plot::Plot` with the latest FFT row as a line. X axis: frequency in Hz (0–4000). Y axis: power in dBFS. Fixed height: 130 px.
- **Waterfall**: `egui::Image` displaying the latest RGBA texture from `WaterfallBuffer::to_rgba_flat`. Fixed height: 200 px. The texture is uploaded once per new generation (detected via the `generation` counter). The plasma colormap range (`min_db`, `max_db`) is shared across all four panels and controllable via two sliders in the noise parameter panel.

### BottomPanel statistics

| Field | Description |
|---|---|
| Runs | Total modulate–demodulate–compare cycles completed |
| OK | Cycles where decoded output matches input payload |
| Fail | Cycles where decoded output differs or demodulation returned an error |
| BER | Bit error rate: total bit errors / total bits sent across all runs |
| Throughput | Estimated bits per second based on modulated sample count and sample rate |
| Event log | Scrollable list of mode transitions, FEC correction events, and decode failures |

---

## Regression testing modes

### Continuous mode (default)

The signal path loops indefinitely: modulate → inject noise → demodulate → compare → record stats. The user watches spectrum/waterfall and statistics update in real time and stops manually.

### SNR sweep mode

A configurable SNR sweep: runs N decodes at each SNR step from `snr_start` to `snr_end` in `snr_step` increments. Default range: −30 dB to +30 dB in 1 dB steps, 20 decodes per step. For each step, records the decode success rate, BER, and decode pass (first or second pass). Results are logged to the event log and optionally saved as a JSON report. This produces the empirical SNR floor for each mode (see `docs/js8call-analysis.md` for expected floor estimates by mode).

### Speed reduction ladder

Tests the HPX rate ladder under degrading channel conditions. The ladder is:

`BPSK31 → BPSK63 → BPSK100 → BPSK250 → QPSK125 → QPSK250 → QPSK500`

The test starts at the fastest configured mode. When the decode success rate drops below a configurable threshold (default: 80% over 10 consecutive runs), the mode steps down one level. The event log records each transition and the SNR at transition time.

### FEC stress test

Runs the mode at a fixed SNR with FEC enabled. Injects progressively longer burst errors (via the Gilbert-Elliott model) to find the burst length at which FEC fails. Compares the FEC-enabled BER against the uncoded BER at the same channel conditions.

---

## Implementation phases

### Phase A — `crates/openpulse-channel` (no UI dependency)

A1. Crate scaffold: `Cargo.toml`, `src/lib.rs`, `ChannelModel` trait, `ChannelError` type.  
A2. `AwgnChannel` + unit tests (RMS power at SNR=0; seeded reproducibility).  
A3. `GilbertElliottChannel` + unit tests (mean burst length within 10% over 100 k symbols).  
A4. `QrnModel`, `QrmModel`, `QsbModel`, `ChirpModel`.  
A5. `WattersonChannel` using `rustfft` for Doppler envelope shaping.  
A6. `CompositeChannel` + `build_channel` factory + serde for all config types.  
A7. `PowerSpectrum` + `WaterfallBuffer` in `src/dsp.rs` + unit tests (tone peak location, Hann normalisation).  
A8. Update root `Cargo.toml` to add `crates/openpulse-channel` as a workspace member.

### Phase B — MVP application (TX + Mixed panels, BPSK100, AWGN)

B1. `apps/openpulse-testbench/Cargo.toml` + workspace registration.  
B2. `src/state.rs`: `AppConfig`, `AppState`, `TestStats`, `RunCommand`, four `Arc<RwLock<WaterfallBuffer>>`.  
B3. `src/signal_path.rs`: background thread with TX and Mixed taps, BPSK100, `AwgnChannel` at 15 dB, fixed 16-byte payload.  
B4. `src/app.rs`: `eframe::App` implementation with toolbar (Run/Stop) and two panels.  
B5. `src/main.rs`: tracing initialisation, `eframe::run_native` entry point.  
B6. Smoke test: compile and verify spectrum shows BPSK100 signal at 1500 Hz.

### Phase C — All four panels, all noise models, all modes

C1. Extend `signal_path.rs`: Noise and RX taps, stats accumulation.  
C2. `src/ui/toolbar.rs`: mode selector, noise selector, SNR slider, seed control, FEC checkbox.  
C3. `src/ui/spectrum.rs`: reusable `draw_spectrum_panel()` using `egui_plot`.  
C4. `src/ui/waterfall.rs`: reusable `draw_waterfall_panel()` with texture upload and plasma colormap.  
C5. `src/ui/controls.rs`: noise parameter panel with profile selector + sliders + composite builder.  
C6. `src/ui/results.rs`: stats bar and event log.  
C7. Layout expansion to four columns.

### Phase D — Regression test modes

D1. FEC path in `signal_path.rs` (encode/decode with `FecCodec`, FEC correction tracking).  
D2. SNR sweep mode with JSON report output.  
D3. Speed reduction ladder with event log.  
D4. FEC stress test (progressive burst injection).

### Phase E — ALSA `snd-aloop` integration (future)

A `SndAloopTapBackend` that routes the four signal path taps to named ALSA virtual devices via the `snd-aloop` kernel module, using the `alsa` crate. This enables other ALSA applications (e.g. a second instance of OpenPulseHF CLI) to source or sink audio from the testbench signal path. Gated behind a `snd-aloop` Cargo feature flag so the binary compiles without ALSA development headers when the feature is not enabled.

---

## New workspace dependencies

The following additions to `[workspace.dependencies]` in `Cargo.toml` are required. Version numbers should be verified against crates.io at implementation time.

```toml
openpulse-channel = { path = "crates/openpulse-channel" }
rustfft           = "6.2"
rand              = { version = "0.8", features = ["std_rng"] }
rand_distr        = "0.4"
eframe            = { version = "0.29", default-features = false, features = ["default_fonts", "glow"] }
egui              = "0.29"
egui_plot         = "0.29"
```

`num-complex = "0.4"` and `crossbeam-channel = "0.5"` are already declared. The `glow` feature for eframe uses OpenGL, which is the correct backend for Linux. The `wgpu` backend is an alternative but adds significant compile time; the OpenGL path is more mature on Linux with ALSA/PipeWire audio stacks.

---

## Known implementation challenges

**FFT size vs. modulated block length.** BPSK31 at 8000 Hz sample rate produces approximately 256 samples per symbol at 31.25 baud. A 16-byte payload (128 bits) produces roughly 33,000 samples, far larger than `FFT_SIZE = 1024`. `WaterfallBuffer::push_samples` handles this by segmenting the block: one waterfall row is pushed per `FFT_SIZE`-sample chunk. The spectrum panel displays the most recent chunk; the waterfall shows the full block history.

**FEC + short payloads.** `FecCodec::encode` with a 16-byte payload produces 255 bytes (one full RS block). At BPSK31, 255 bytes × 8 bits = 2040 symbols = approximately 65,000 samples = 8 seconds of audio. For FEC testing, use BPSK250 or higher, or a short-block FEC mode. This is documented in Phase D and a configurable payload length slider is added at that phase.

**Watterson Doppler envelope frequency resolution.** For short blocks (1024 samples at 8000 Hz = 128 ms), the frequency resolution for the Doppler shaping filter is 7.8 Hz/bin, which is coarser than the Doppler spread of the Good F1 profile (0.1 Hz). For Good F1, the filter bandwidth is sub-bin; the envelope generator will exhibit only one non-zero bin and degrade to a constant-amplitude modulation rather than true diffuse fading. This is acceptable for Good profiles. Moderate and Poor profiles (Doppler ≥ 1.0 Hz) are correctly represented at 1024-sample block size.

**QRM phase coherence across blocks.** The `QrmModel` maintains per-tone phase accumulators. These must be updated at the end of each `generate_noise` call: `φ_i += 2π × f_i × N / sample_rate`, where N is the block length. If the block length varies (it does — different modes produce different sample counts), the phase update must use the actual N from the last call, not a fixed block size. This is handled by the signature `generate_noise(&mut self, length: usize) -> Vec<f32>`.

**RwLock contention.** The egui render thread reads all four waterfall buffers at 30 fps. The signal thread writes at ~10 Hz. With `RwLock`, the render thread (read-only) almost never blocks. If contention is observed in profiling, switch to `arc-swap` (the `ArcSwap<WaterfallBuffer>` pattern): the signal thread atomically swaps in a new buffer; the render thread reads the current Arc with zero locking. This is a straightforward refactor if needed.

---

## File manifest

```
crates/openpulse-channel/
  Cargo.toml
  src/
    lib.rs               ChannelModel trait, ChannelError, build_channel()
    config.rs            ChannelModelConfig enum + all Config structs (serde)
    awgn.rs              AwgnChannel
    gilbert_elliott.rs   GilbertElliottChannel + 4 named profiles
    watterson.rs         WattersonChannel (two-ray, complex Doppler envelope via rustfft)
    noise/
      mod.rs
      qrn.rs             QrnModel (Gaussian + impulsive)
      qrm.rs             QrmModel (phase-coherent discrete tones)
      qsb.rs             QsbModel (sinusoidal amplitude fading)
      chirp.rs           ChirpModel (linear frequency sweep)
    composite.rs         CompositeChannel
    dsp.rs               PowerSpectrum, WaterfallBuffer, plasma colormap

apps/openpulse-testbench/
  Cargo.toml
  src/
    main.rs              eframe::run_native entry point, tracing init
    app.rs               TestbenchApp: eframe::App, update() UI loop
    state.rs             AppConfig, AppState, TestStats, RunCommand, tap Arc<RwLock<>>
    signal_path.rs       background thread: 4-tap pipeline via ModulationPlugin directly
    ui/
      mod.rs
      toolbar.rs         mode/noise/SNR/seed/FEC controls
      spectrum.rs        draw_spectrum_panel() via egui_plot
      waterfall.rs       draw_waterfall_panel() via TextureHandle + plasma colormap
      controls.rs        noise param panel: profile selector + sliders + composite builder
      results.rs         stats bar: BER, decode rate, event log
```
