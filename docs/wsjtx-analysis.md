---
project: openpulsehf
doc: docs/wsjtx-analysis.md
status: living
last_updated: 2026-05-02
---

# WSJTX Analysis

This document captures technical findings from an analysis of the WSJTX project (https://github.com/WSJTX/wsjtx) relevant to OpenPulseHF design. The goal is to understand the signal processing and protocol techniques that enable WSJTX modes to operate at extremely negative SNR, and to identify what is applicable or instructive for HPX.

WSJTX is an open-source implementation maintained by K1JT and the WSJT Development Group. The source is publicly available and the algorithms are described in peer-reviewed papers and on-air protocol specifications. WSJTX is not a TNC or file-transfer protocol — it is a scientific weak-signal communication system. Its relevance to OpenPulseHF is architectural, not compatibility-oriented.

Sources used:
- WSJTX source repository: https://github.com/WSJTX/wsjtx
- Key source files: `lib/ft8_decode.f90`, `lib/ft4_decode.f90`, `lib/bpdecode40.f90`, `lib/bpdecode128_90.f90`, `lib/bpdecode144.f90`, `lib/sync9.f90`, `lib/afc65b.f90`, `lib/afc9.f90`, `lib/analytic.f90`, `lib/ccf65.f90`
- WSJT paper trail (K1JT publications, ARRL articles)

---

## Mode overview

WSJTX implements a family of weak-signal modes sharing a common technical heritage. The primary modes are:

| Mode | T/R cycle | Occupied BW | SNR threshold | Primary use |
|------|-----------|-------------|---------------|-------------|
| FT8 | 15 s | ~50 Hz | −21 dB | General DX; most widely deployed |
| FT4 | 7.5 s | ~90 Hz | −17.5 dB | Contest; faster than FT8 |
| JT65 | 60 s | ~180 Hz | −25 dB | EME (Earth-Moon-Earth) |
| JT9 | 60 s | ~16 Hz | −27 dB | HF QRP; extremely narrow |
| Q65 | 15–300 s | configurable | ~−27 dB | Scatter; meteor; EME |
| MSK144 | 0.4–0.9 s | 2.4 kHz | −1 dB | Meteor scatter |
| WSPR | 2 min | ~6 Hz | −28 dB | Propagation beacons only |

SNR thresholds are specified in a 2500 Hz reference noise bandwidth (standard WSJT convention). All modes except MSK144 are designed for the ionospheric HF channel. Q65 and JT9 approach the Shannon limit for their respective bandwidth and T/R cycle combinations.

---

## FEC: LDPC(174,91) codes and belief propagation decoding

FT8 and FT4 use **LDPC(174,91)** codes: 91 information bits (including a 14-bit CRC) encoded into 174 channel symbols, giving a code rate of approximately 0.523.

The LDPC parity check matrix is designed with low column density (sparse), allowing iterative message-passing decoding. The WSJTX implementation uses a **log-domain sum-product belief propagation** decoder operating over 40–50 iterations per frame:

1. **Initialisation**: channel LLRs (log-likelihood ratios) are computed from the demodulator soft outputs. Each variable node is initialised with its channel LLR.
2. **Check node update**: each check node aggregates incoming messages using the log-domain `tanh` approximation: `2 × atanh(∏ tanh(m/2))`. The approximation avoids numerical underflow in long products.
3. **Variable node update**: each variable node sums incoming check-node messages and its channel LLR.
4. **Syndrome check**: after each iteration, the decoder evaluates the parity syndrome. Iteration terminates early when the syndrome is zero (valid codeword found).

Key implementation files: `lib/bpdecode40.f90` (FT4 decoder), `lib/bpdecode128_90.f90`, `lib/bpdecode144.f90`.

**Comparison with Reed-Solomon (OpenPulseHF current FEC):**

| Property | LDPC(174,91) | RS(255,223) |
|---|---|---|
| Code type | Linear block, sparse parity matrix | Linear block, algebraic |
| Decoding | Iterative belief propagation (probabilistic) | Berlekamp-Massey (deterministic) |
| Performance | Within ~1 dB of Shannon limit at large block sizes | ~3–5 dB from Shannon limit at equivalent code rate |
| SNR advantage at practical rates | ~5–8 dB vs RS at code rate ~0.5 | Baseline |
| Decoder latency | Variable (early termination); 40–50 iterations typical | Fixed and deterministic |
| CPU per decode | Higher (iterative) | Lower (algebraic) |
| Failure mode | Silent miscorrection possible if CRC not checked | Hard decoding; detects up to (n−k)/2 symbol errors |
| Block size constraint | Benefits from longer blocks (≥ 100 bits) | Operates well at 255-byte blocks |
| GPU parallelism | Well-suited (parallel check/variable node updates) | Less parallel; sequential Berlekamp-Massey |

**HPX implication:** LDPC is the stronger candidate for Phase 3.2 FEC evaluation, ahead of Turbo codes. The 5–8 dB coding gain at equal code rate is the primary quantitative argument. The variable decoder latency and higher CPU cost must be benchmarked on Raspberry Pi 4 before committing.

---

## Synchronisation: Costas arrays

FT8 uses a **Costas array** synchronisation scheme. Each FT8 frame embeds three 7×7 Costas arrays at the beginning, middle, and end of the 79-symbol transmission. A Costas array is a permutation matrix where all difference vectors between pairs of marked cells are distinct — this property gives ideal autocorrelation (zero sidelobes) for the sync pattern.

Each Costas sync marker is 7 symbols long, drawn from 8 tones (one tone per symbol, selected by the Costas permutation). Sync symbols are interleaved with data symbols: positions 0–6, 36–42, and 72–78 in the 79-symbol frame are reserved for sync.

The receiver cross-correlates the received signal against the known Costas sync pattern over a 2D search space:
- Frequency axis: ±20 Hz (or more for initial acquisition) in 6.25 Hz steps (one bin per symbol period)
- Time axis: ±1–2 symbol periods

The cross-correlation peak locates both the symbol timing boundary and the carrier frequency offset simultaneously. Once located, soft-decision metrics (LLRs) are extracted for each of the 58 data symbols and passed to the LDPC decoder.

**HPX implication:** Costas arrays are a superior synchronisation approach for HPX preamble design, particularly at low SNR where PSK carrier phase is ambiguous. A short Costas or Chu sequence in the HPX preamble would provide combined timing and frequency acquisition with near-zero false-positive rate. This is distinct from the current BPSK plugin preamble, which is a simple training pattern. Relevant for Phase 1.5 (radio interface) and any future narrowband HPX mode.

---

## Noise floor estimation: 40th-percentile method

WSJTX normalises the receiver noise floor using the **40th percentile of spectral bin energies** across the active passband, rather than the mean. This is computed in `lib/sync9.f90` and equivalent files.

The rationale: in a crowded amateur band, many spectral bins contain signal energy from other stations. The arithmetic mean of all bins is biased upward by signal peaks. The 40th percentile (below-median) is dominated by noise-only bins, providing a robust estimate of the true noise floor even when 40–50% of the band is occupied by signals.

This estimator feeds the SNR computation (for each decoded station's reported SNR in the WSJTX display) and the decoder sensitivity threshold (below which candidate signals are not attempted).

**HPX implication:** The 40th-percentile estimator is directly applicable to OpenPulseHF's AFC and squelch design (Phase 1.5). A robust noise floor estimate is needed to set the squelch threshold and to calculate SNR for the diagnostic output. The mean estimator used in simple implementations will be biased high on active HF bands.

---

## Multi-pass decoding with signal subtraction

WSJTX implements **multi-pass iterative decoding** on each received 15-second block. After a frame is decoded successfully, the decoded signal is reconstructed (re-modulated from the decoded bits) and subtracted from the received audio. A second decoder pass then operates on the residual, which may contain weaker stations previously masked by the stronger decoded signal.

Documented in `lib/ft8_decode.f90`. The JS8Call-improved fork fixed the decode depth at 2 after determining that passes 3+ yielded marginal additional decodes in practice.

**Depth 2 finding:** The second pass (signal subtraction) captures most of the benefit. Passes 3+ rarely produce additional decodes under practical operating conditions. This is a strong empirical finding that should inform HPX decoder architecture: do not plan for more than two decode passes.

**HPX implication:** Multi-pass decoding is most relevant in shared-channel scenarios (e.g. two HPX stations calling on the same frequency). It is out of scope for HPX v1 (which is point-to-point). It remains a consideration for relay and broadcast modes (Phase 2.4–2.6) and should be noted as a future enhancement in the HPX decoder design.

---

## T/R cycle length and the SNR vs latency trade-off

FT8's 15-second T/R cycle is the minimum practical cycle length for keyboard-to-keyboard exchange. Longer cycles (Q65 allows up to 5 minutes) further improve sensitivity by extending coherent integration time. JT9 and WSPR use 60-second and 2-minute cycles respectively.

The relationship is approximately: each doubling of T/R cycle length at fixed bandwidth gives 3 dB improvement in SNR threshold (Shannon consistent, assuming matched filter integration).

| Mode | T/R cycle | Approximate SNR threshold |
|------|-----------|--------------------------|
| FT8 | 15 s | −21 dB |
| JT65 | 60 s | −25 dB (4× cycle = ~6 dB gain) |
| Q65-60 | 60 s | ~−27 dB (shorter interframe guard, tighter modulation) |
| WSPR | 120 s | −28 dB |

**HPX implication:** HPX is not a weak-signal mode — it is a file transfer ARQ protocol. A 15-second T/R cycle is unacceptable for file transfer. HPX's design point is 1–2 second ARQ cycles (consistent with PACTOR/VARA precedent) at the cost of a higher SNR floor. The testbench SNR sweep range of −30 to +30 dB is designed to measure HPX's empirical floor and place it in context against the WSJTX reference points above.

---

## AFC: automatic frequency control

WSJTX implements AFC in `lib/afc65b.f90` (JT65) and `lib/afc9.f90` (JT9). The AFC algorithms perform fine frequency correction after the Costas/sync-based coarse acquisition:

1. Compute the cross-correlation of the received symbol sequence against a set of candidate frequency offsets (±Δf in fine steps).
2. Select the offset maximising correlation with the sync pattern.
3. Apply the correction before soft-metric extraction for the LDPC decoder.

JT65 and JT9 use separate AFC implementations tuned to their different symbol structures.

**HPX implication:** AFC is specified in Phase 1.5 (±50 Hz offset tracking). The WSJTX approach (cross-correlation against sync pattern over a frequency axis) is the correct method for initial acquisition. The ongoing AFC tracking loop (Gardner TED or similar) is the correct method for residual drift during a session.

---

## Analytic signal processing (Hilbert transform)

`lib/analytic.f90` implements Hilbert transform-based analytic signal generation. WSJTX uses analytic signals for frequency-domain processing of audio input: the analytic signal suppresses negative-frequency images, enabling one-sided spectral analysis without aliasing from negative frequencies.

For audio processing at 12 kHz sample rate, the Hilbert filter is a standard 121-tap FIR with odd-symmetric coefficients (all-pass in real part, 90-degree shift in imaginary part). The output is a complex analytic signal whose FFT has non-zero content only in [0, fs/2].

**HPX implication:** The testbench `PowerSpectrum` implementation uses real FFT (positive frequencies only) which achieves the same result without the Hilbert preprocessing. For the future coherent receiver path, analytic signal processing will be needed for baseband mixing and phase coherent demodulation.

---

## Working conclusions for OpenPulseHF

### Direct design recommendations

- **LDPC(174,91) + BP decoding is the target FEC for Phase 3.2**, not Turbo codes. The 5–8 dB coding gain is substantiated by WSJTX's operational results at scale. Must be benchmarked on Raspberry Pi 4 (target: 10 ms per decode at block size 174 bits).

- **Costas array preamble** should be evaluated for any new narrowband HPX mode or the HPX500 preamble redesign. Provides simultaneous timing + frequency acquisition at −15 to −20 dB SNR.

- **40th-percentile noise floor estimator** should be implemented in the HPX receiver for squelch and SNR reporting. Replace any mean-based estimator at the audio input stage.

- **Decode depth cap at 2**: do not design for more than two decode passes in the HPX receiver. WSJTX and JS8Call both confirm that passes 3+ are marginal.

### What WSJTX does that HPX intentionally does not

- **Long T/R cycles**: WSJTX optimises for SNR at the cost of latency. HPX optimises for throughput at the cost of SNR sensitivity. This is correct for the HPX use case (file transfer) but means HPX will never reach −25 dB SNR thresholds.
- **No ARQ**: WSJTX modes transmit and listen; they do not retransmit on error. Reliability is achieved through extreme FEC strength, not through ARQ. HPX uses ARQ and can tolerate weaker FEC per frame because retransmission is available.
- **No session concept**: WSJTX contacts are single-exchange (signal report + confirmation). HPX sessions span minutes to hours.

### Testbench implications

- The testbench SNR sweep range of −30 to +30 dB is calibrated against the WSJTX floor (−21 to −28 dB) at the low end and QPSK500's operating range at the high end.
- The testbench should report measured SNR per cycle so that results can be plotted against the WSJTX reference thresholds.
- LDPC comparison runs (future Phase D/E testbench work) should use FT8 frame sizes (91 data bits, 174 encoded) to produce directly comparable BER curves.
