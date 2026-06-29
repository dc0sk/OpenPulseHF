---
project: openpulsehf
doc: docs/dev/research/ofdm-research.md
status: living
last_updated: 2026-05-08
---

# OFDM Research: Simulation Study for OpenPulseHF

## 1. Motivation and Background

VARA HF achieves up to 7536 bps by using OFDM with many narrowband subcarriers spread across the 2.4 kHz SSB passband. Each subcarrier carries QPSK or higher-order QAM, and a cyclic prefix (CP) handles inter-symbol interference from multipath propagation.

The key challenge of OFDM on HF is **PAPR** (peak-to-average power ratio). A linear combination of N sinusoids can produce instantaneous peaks up to N× the average power — up to `10·log₁₀(N)` dB for N uncorrelated subcarriers. For a 52-subcarrier system this is ~17 dB worst-case. High PAPR demands linear PA headroom: a PA capable of 100 W average must handle 1000–10000 W peak with no distortion. SSB HF radios are typically designed for PAPR ≈ 4–6 dB (speech), so OFDM signals cause significant non-linear distortion unless PAPR is reduced.

The study evaluates whether reduced-PAPR OFDM can be made cost-effective for OpenPulseHF given the existing single-carrier waveform infrastructure.

## 2. Simulation Methodology

### 2.1 Configurations Tested

| Config | Subcarriers | CP Samples | Baud/subcarrier | Modulation | Pilot SCs |
|--------|------------|------------|-----------------|------------|-----------|
| VARA-like | 52 | 16 | ~148 (8000/54) | QPSK | 4 |
| Reduced | 16 | 16 | ~471 (8000/32) | QPSK | 2 |
| Minimal | 8 | 16 | ~1000 (8000/24) | QPSK | 1 |

All configurations use fs = 8000 Hz and QPSK (2 bits/subcarrier).

### 2.2 PAPR Reduction Techniques

| Technique | Description |
|-----------|-------------|
| None | Raw OFDM, no PAPR reduction |
| Clip 4 dB | Single-pass clip at RMS × 10^(4/20), 5-tap smoothing |
| Clip 3 dB | Single-pass clip at RMS × 10^(3/20), 5-tap smoothing |
| Clip 2 dB | Single-pass clip at RMS × 10^(2/20), 5-tap smoothing |
| Iterative clip (target 6 dB) | Repeated clip-to-RMS×2 until PAPR ≤ 5.9 dB (max 50 iters) |
| Tone reservation (4 tones) | Reserve 2 low + 2 high subcarriers; iterative peak cancellation (50 iters) |

### 2.3 Channel Models

All channels use the `openpulse-channel` crate:

| Channel | Description |
|---------|-------------|
| Clean | No impairment (passthrough) |
| AWGN 20 dB | White Gaussian noise at 20 dB SNR (seed 42) |
| AWGN 10 dB | White Gaussian noise at 10 dB SNR (seed 42) |
| Watterson Good F1 | 0.1 Hz Doppler spread, 0.5 ms delay spread (seed 1) |

### 2.4 Metrics

- **PAPR (dB)**: `10·log₁₀(peak_power / mean_power)` on the TX frame after PAPR reduction
- **BER**: Bit error rate on 64-byte payload after demodulation (no FEC)
- **Gross bps**: Net payload bits / OFDM symbol duration (includes CP overhead, excludes pilots)
- **BW (Hz)**: One-sided Nyquist bandwidth of the baseband signal = fs/2 (4000 Hz at 8 kHz sample rate)

*Note: the simulation uses a real-valued IFFT output (no carrier upconversion) so BW = fs/2 (one-sided Nyquist bandwidth of the baseband signal). Carrier upconversion to the SSB passband is not modelled.*

## 3. Results Summary

### 3.1 PAPR by Configuration and Reduction Technique (Clean Channel)

Representative values from the sweep (exact values in `raw_results.json`):

| Config | None | Clip 4 dB | Clip 3 dB | Clip 2 dB | Iterative 6 dB | Tone Res. 4 |
|--------|------|-----------|-----------|-----------|----------------|-------------|
| VARA-like (52 SC) | ~9–11 dB | ~7–8 dB | ~7 dB | ~7 dB | ≤ 6 dB | ~8–9 dB |
| Reduced (16 SC) | ~8–10 dB | ~7 dB | ~6.5 dB | ~6.5 dB | ≤ 6 dB | ~7–8 dB |
| Minimal (8 SC) | ~7–9 dB | ~6.5 dB | ~6.5 dB | ~6.5 dB | ≤ 6 dB | ~7 dB |

**Key finding**: Iterative clipping (target 6 dB, 50 iterations) reliably achieves PAPR ≤ 6 dB across all configurations. Single-shot clipping at 3–4 dB above RMS does not reliably reach the 6 dB gate because the RMS drops after each clip, leaving residual peaks above the target.

### 3.2 BER vs Channel

Clean-channel BER is 0 for all configurations without PAPR reduction. With aggressive iterative clipping (6 dB target), BER increases due to spectral regrowth from the clipping non-linearity, with worse BER at lower SNR.

Tone reservation at 4 tones has minimal BER impact (pilot subcarriers carry no payload) but achieves modest PAPR reduction (~1–2 dB).

### 3.3 Throughput Comparison

| Waveform | Baud | Bits/symbol | Gross bps | Practical net bps |
|----------|------|-------------|-----------|-------------------|
| BPSK250 | 250 | 1 | 250 | ~200 (preamble overhead) |
| QPSK500 | 500 | 2 | 1000 | ~800 |
| QPSK1000-RRC | 1000 | 2 | 2000 | ~1600 |
| 8PSK1000-RRC | 1000 | 3 | 3000 | ~2400 |
| OFDM reduced (16 SC, QPSK) | 471/SC | 2 | ~1600 | ~1100 |
| OFDM VARA-like (52 SC, QPSK) | 148/SC | 2 | ~3900 | ~3000 |

## 4. Comparison Baseline: Single-Carrier Performance and PAPR

| Waveform | Measured PAPR | Notes |
|----------|---------------|-------|
| BPSK250 (Hann) | ~3–4 dB | Hann windowing caps instantaneous amplitude |
| QPSK500 (Hann) | ~3–5 dB | Similar — window is the dominant envelope shape |
| QPSK500-RRC | ~4–6 dB | RRC-shaped impulse train; PAPR slightly higher than Hann |
| OFDM minimal (8 SC, none) | ~7–9 dB | 3–5 dB higher than single-carrier |
| OFDM VARA-like (52 SC, none) | ~9–11 dB | 5–7 dB higher than single-carrier |

Single-carrier PSK waveforms have inherently lower PAPR because the amplitude envelope is shaped by a single window function (Hann or RRC). OFDM PAPR scales with the number of subcarriers.

## 5. Recommendation

### 5.1 Decision: Defer OFDM Implementation

**Recommendation: do not implement OFDM in the OpenPulseHF waveform stack at this time.**

Rationale:

1. **PAPR is the blocking issue.** Achieving PAPR ≤ 6 dB requires iterative clipping (50 iterations per frame) which destroys the ISI-free OFDM orthogonality property and degrades BER. Without aggressive PA linearisation or hardware precoding, OFDM PAPR will cause non-linear distortion on typical HF rigs.

2. **Single-carrier RRC waveforms are competitive.** `8PSK1000-RRC` achieves ~2400 net bps with PAPR ≈ 4–5 dB and straightforward equalisation (single sample per symbol, no cyclic prefix overhead). This is comparable to the reduced OFDM configurations at far lower implementation complexity.

3. **The VARA OFDM advantage (7536 bps) requires wider bandwidth and higher SNR.** Reproducing VARA-level throughput would require QAM64 or QAM256 on 52 subcarriers — these require excellent per-subcarrier channel equalisation, pilot-based channel estimation, and fine Doppler/delay spread tracking. The implementation cost is very high relative to incremental throughput gain over RRC single-carrier.

4. **HF-optimised alternative exists.** The single-carrier HPX waveform with Watterson-aware interleaving and RS FEC is better matched to HF burst-error profiles than OFDM with a short cyclic prefix.

### 5.2 Conditions for Revisiting OFDM

Reconsider if:
- A `clip_iterative` + FFT-domain spectral confinement (clip-and-filter with per-bin frequency-domain equalisation) is implemented — this is the state-of-the-art CF-PAPR approach used in VARA
- Hardware acceleration (GPU or DSP) makes per-frame FFT iterations practical in real time
- A throughput target above 5000 bps is set, requiring OFDM's inherent bandwidth efficiency

### 5.3 Proposed OfdmProfile (If Proceeding — FF-5)

```rust
pub struct OfdmProfile {
    pub n_subcarriers: usize,
    pub cp_samples: usize,
    pub mod_order: usize,         // 1=BPSK, 2=QPSK, 4=QAM16, 6=QAM64
    pub pilot_spacing: usize,     // subcarriers between pilots
    pub papr_target_db: f32,      // iterative clip target
    pub fec: bool,                // RS + interleaver as used in single-carrier stack
}
```

Implementation path (FF-5) would add:
1. FFT-domain channel estimation using pilot subcarriers
2. Per-subcarrier ZF or MMSE equaliser
3. Iterative clip-and-filter PAPR reduction (frequency domain)
4. Integration with existing FEC and SAR layers

---

*Generated by `ofdm_simulation` integration test. Raw results: `docs/ofdm-research/raw_results.json`.*
*Study date: 2026-05-07.*
