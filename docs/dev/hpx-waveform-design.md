---
project: openpulsehf
doc: docs/hpx-waveform-design.md
status: living
last_updated: 2026-05-01
---

# HPX Waveform Design

This document defines the waveform architecture for the HPX protocol family: the rate ladder, modulation class selection rationale, ACK frame design, and Memory-ARQ specification. It draws on the comparative analysis in docs/vara-research.md, docs/ardop-research.md, and docs/pactor-research.md.

Parameters marked **[proposed]** are design targets requiring simulation validation before implementation. Parameters marked **[confirmed]** are derived from first principles or from validated incumbent designs.

---

## Design goals

1. Compete with VARA HF on peak throughput in the 2300 Hz class under good channel conditions.
2. Exceed ARDOP on robustness in the 200–500 Hz class under poor channel conditions.
3. Maintain full-power operation on any linear SSB transceiver across the entire rate ladder.
4. Preserve a single ARQ cycle time across all speed levels to enable graceful rate transitions.
5. Provide the most robust possible connection establishment mode for use on any HF path.

---

## The single-carrier / OFDM boundary

### Why the boundary exists

Single-carrier PSK operates without inter-symbol interference (ISI) as long as the symbol period T_sym significantly exceeds the channel's multipath delay spread τ_max.

For the Watterson Moderate M1/M2 channel (typical daytime HF path, τ_max ≈ 1–2 ms):

| Baud rate | T_sym | τ_max / T_sym | ISI risk |
|-----------|-------|--------------|----------|
| 31.25 | 32 ms | 3–6% | None |
| 63 | 16 ms | 6–12% | Negligible |
| 250 | 4 ms | 25–50% | Moderate — usable with differential encoding |
| 500 | 2 ms | 50–100% | Severe — equaliser required |
| 1000 | 1 ms | 100–200% | Unusable without equalization or OFDM |

The practical single-carrier boundary for HF without an adaptive equaliser is **~250–500 baud** [confirmed].

OFDM solves this by making each subcarrier's symbol period much longer than τ_max. A cyclic prefix of duration T_cp ≥ τ_max absorbs multipath with no ISI, at the cost of T_cp / (T_fft + T_cp) guard-interval overhead.

### Why small OFDM rather than large OFDM

VARA uses 52 subcarriers. This gives excellent frequency diversity and high spectral efficiency but produces 9–12 dB PAPR — requiring significant PA back-off and ruling out class-AB operation at rated power.

The PAPR of an N-subcarrier OFDM signal scales approximately as 10 log₁₀(N) dB at the 0.01% CCDF point. For practical HPX design targets:

| N subcarriers | Approx. PAPR | Back-off needed | Notes |
|--------------|-------------|----------------|-------|
| 4 | ~6 dB | Minimal | Compatible with most linear PAs at rated power |
| 8 | ~9 dB | Moderate | Same as VARA ACK; manageable |
| 16 | ~12 dB | Significant | Comparable to VARA data; requires PA headroom |
| 52 (VARA) | ~12–15 dB | Large | Requires conservative power setting |

**Design decision [proposed]:** HPX uses a maximum of 16 subcarriers. HPX500 profiles use 4–8 subcarriers (PAPR ≤ 9 dB, compatible with rated-power linear operation). HPX2300 profiles use 8–16 subcarriers (PAPR 9–12 dB, requires moderate back-off but retains more power than VARA).

This is a deliberate architectural divergence from VARA. The trade-off: slightly lower peak spectral efficiency in exchange for significantly better power efficiency and transceiver compatibility.

### Combining single-carrier and OFDM in one rate ladder

The rate ladder uses single-carrier for low speed levels and small OFDM for high speed levels, with the same 1.25-second ARQ cycle time across all levels. Transitions between modulation classes are transparent to the ARQ protocol: only the waveform changes.

The ACK frame design (4FSK, independent of data modulation) ensures that rate adaptation signals survive across modulation class transitions without a separate control handshake.

---

## HPX rate ladder [proposed]

All speed levels share:
- ARQ cycle time: **1.25 seconds** (short path) or **3 × 1.25 = 3.75 seconds** (long path, >2500 km)
- Packet structure: Datablock | Status byte | CRC-16 (CCITT)
- Status byte: E / S1 S0 / M2 M1 M0 / C1 C0 (same field layout as PACTOR-4, see below)
- ACK frames: 4FSK, independent of data modulation (see ACK design section)

### HPX500 — 500 Hz bandwidth class

Target occupied bandwidth: ≤ 500 Hz.

| SL | Name | Modulation | Carrier | FEC rate | Net data rate | User bytes/cycle | Notes |
|----|------|-----------|---------|----------|--------------|-----------------|-------|
| 1 | Chirp | 2-tone DBPSK + linear chirp | 2 SC at 650 + 1350 Hz | 1/2 conv. | ~100 bps | ~15 | CONREQ/CONACK only; not used for data |
| 2 | Robust | Spread DBPSK (SF = 8) | 1 SC | 1/2 | ~150 bps | ~23 | Worst-path fallback |
| 3 | SC-B1 | DBPSK | 1 SC, 250 baud | 1/2 | ~125 bps | ~19 | Poor channel |
| 4 | SC-B2 | DBPSK | 1 SC, 250 baud | 5/6 | ~208 bps | ~32 | Moderate channel |
| 5 | SC-Q | DQPSK | 1 SC, 250 baud | 5/6 | ~417 bps | ~65 | Good channel, single-carrier limit |
| 6 | OFDM-4D | DQPSK | 4 subcarriers, 125 Hz spacing | 5/6 | ~667 bps | ~104 | Cross SC→OFDM boundary; differential (no training) |
| 7 | OFDM-8D | DQPSK | 8 subcarriers, 62.5 Hz spacing | 5/6 | ~741 bps | ~116 | Maximum 500 Hz OFDM |

**SL6/7 OFDM parameters:**
- SL6: T_fft = 8 ms, T_cp = 2 ms, T_sym = 10 ms. CP absorbs up to 2 ms multipath delay.
- SL7: T_fft = 16 ms, T_cp = 2 ms, T_sym = 18 ms. CP absorbs up to 2 ms multipath delay; lower overhead ratio.
- Both use DQPSK per subcarrier: differential encoding means no per-subcarrier pilot or channel estimate needed. Simpler receiver at the cost of ~3 dB SNR vs coherent.
- PAPR: SL6 ≈ 6 dB; SL7 ≈ 9 dB.

### HPX2300 — 2300–2400 Hz bandwidth class

Target occupied bandwidth: ≤ 2400 Hz.

| SL | Name | Modulation | Carrier | FEC rate | Net data rate | User bytes/cycle | Notes |
|----|------|-----------|---------|----------|--------------|-----------------|-------|
| 8 | OFDM-8C-Q | Coherent QPSK | 8 SC, 287 Hz spacing | 5/6 | ~1270 bps | ~198 | 2300 Hz class entry; CAZAC training |
| 9 | OFDM-16C-Q | Coherent QPSK | 16 SC, 144 Hz spacing | 5/6 | ~2963 bps | ~463 | Standard 2300 Hz profile |
| 10 | OFDM-16C-16Q | Coherent 16-QAM | 16 SC, 144 Hz spacing | 5/6 | ~5926 bps | ~925 | High throughput; requires good SNR |
| 11 | OFDM-16C-32Q | Coherent 32-QAM | 16 SC, 144 Hz spacing | 5/6 | ~7407 bps | ~1157 | Peak throughput; requires excellent SNR |

**SL8–11 OFDM parameters:**
- SL8: T_fft = 3.5 ms, T_cp = 2 ms, T_sym = 5.5 ms. High guard interval ratio (36%) — evaluate whether a longer FFT is preferable.
- SL9–11: T_fft = 6.9 ms, T_cp = 2 ms, T_sym = 8.9 ms. Guard interval ratio 22%. CP absorbs up to 2 ms multipath. 16 subcarriers × 144 Hz ≈ 2304 Hz occupied bandwidth.
- Coherent detection: CAZAC-16 training sequences inserted between data blocks for channel estimation. Enables QAM at higher modulation orders without the 3 dB differential penalty.
- PAPR: SL8 ≈ 9 dB; SL9–11 ≈ 9–12 dB. Moderate PA back-off required at SL9+.

**Note on SL11 (32QAM):** 32QAM requires a constellation SNR of approximately 24 dB for BER ≤ 10⁻³ with rate 5/6 FEC. This is only achievable on excellent short-path HF links. It should be treated as a bonus level, not a performance baseline.

### Rate ladder summary diagram

```
SNR (approx.) | HPX500                        | HPX2300
-----------+------------------------------+----------------------------
> 25 dB   |                               | SL11: OFDM-16C-32QAM ~7400 bps
> 20 dB   |                               | SL10: OFDM-16C-16QAM ~5900 bps
> 15 dB   | SL7: OFDM-8D ~740 bps        | SL9:  OFDM-16C-QPSK  ~2960 bps
> 12 dB   | SL6: OFDM-4D ~670 bps        | SL8:  OFDM-8C-QPSK   ~1270 bps
> 10 dB   | SL5: SC-Q    ~420 bps        |
>  8 dB   | SL4: SC-B2   ~210 bps        |
>  5 dB   | SL3: SC-B1   ~125 bps        |
>  2 dB   | SL2: Robust  ~150 bps        |
Any path  | SL1: Chirp   ~100 bps        |
          |  (CONREQ only)                |
```

SNR thresholds are indicative. Actual switching thresholds must be determined by simulation using Watterson M1/M2 channel models (see docs/benchmark-harness.md).

---

## Status byte design

The status byte is identical in layout to PACTOR-4, which itself mirrors the PACTOR-1 through 3 design:

```
Bit 7   : E  — extended status follows at packet start (reserved for future use)
Bits 6-5: S1 S0 — protocol status
Bits 4-2: M2 M1 M0 — compression mode
Bits 1-0: C1 C0 — packet counter (mod 4)
```

**S1 S0 protocol status:**

| S1 S0 | Meaning |
|-------|---------|
| 0 0 | Normal (no special signal) |
| 0 1 | BK — break/changeover request (IRS requests ISS role) |
| 1 0 | QRT — graceful session end |
| 1 1 | SPUG — rate change suggestion (speed up or slow down, direction inferred from ACK type) |

The SPUG bit + ACK type together encode the rate change signal. SPUG with ACK-OK means "I'm receiving well, consider stepping up." SPUG with NACK means "retransmit and step down." This is equivalent to VARA's ACK2/ACK3 but encoded with one status bit rather than a distinct ACK type, reducing the number of required ACK types.

**M2 M1 M0 compression mode:**

| Code | Mode |
|------|------|
| 000 | None (8-bit transparent) |
| 001 | Huffman (English-tuned) |
| 010 | Pseudo-Markov English |
| 011 | Pseudo-Markov German |
| 100–111 | Reserved |

Compression mode is selected per-packet by the transmitter and signalled in the status byte. The receiver switches decompressor mode on each packet. This means compression can be adapted dynamically (e.g. fall back to transparent when compressing binary data) without session renegotiation. See the compression section below.

---

## ACK frame design: 4FSK decoupled from data

### Rationale

ARDOP uses 4FSK for all control frames independent of data modulation. VARA uses parallel FSK (48 tones) for ACK frames independent of OFDM data frames. Both systems arrive at the same design principle: **ACK frames must be decodable at lower SNR than data frames**, because the ACK must survive when the data-to-ACK direction has worse propagation than the primary direction.

The simplest way to guarantee this is to use a modulation that is inherently more robust than the data modulation. 4FSK is the natural choice: it requires no phase reference, no channel estimate, no training sequence, and is decodable at SNR levels where coherent PSK would fail.

PAPR of 4FSK: ≈ 0 dB (single tone at any instant). ACK frames can be transmitted at full rated power regardless of which data modulation class is active.

### ACK frame structure [proposed]

Each ACK frame consists of a short preamble followed by a 4FSK payload:

```
Preamble (known sequence for sync) | ACK payload (4FSK) | CRC-8
```

Duration target: **200–400 ms** (short enough to fit within the guard time of a 1.25 s ARQ cycle after data frame transmission).

4FSK parameters:
- 4 tones, 100 Hz spacing (tones at e.g. 900, 1000, 1100, 1200 Hz — within SSB passband)
- Symbol rate: 100 baud (T_sym = 10 ms)
- Each symbol: 2 bits (4FSK encodes 2 bits per tone)
- A 20-symbol 4FSK burst encodes 40 bits = 5 bytes in 200 ms
- 5 bytes is sufficient for: 3-bit ACK type + session-ID hash (anti-collision) + CRC-8

### ACK frame types

| Type | Code | Meaning |
|------|------|---------|
| ACK-OK | 000 | Data frame received correctly; maintain current rate |
| ACK-UP | 001 | Received correctly; request one rate step up |
| ACK-DOWN | 010 | Received correctly; request one rate step down |
| NACK | 011 | Received with uncorrectable errors; retransmit |
| BREAK | 100 | Changeover: IRS requests ISS role |
| REQ | 101 | ACK was lost; please retransmit last data frame |
| QRT | 110 | Graceful session end |
| ABORT | 111 | Abnormal teardown |

The session-ID hash (anti-collision field, analogous to VARA's session-unique ACK codes) prevents a nearby station's ACK from being decoded as a command to a different session. It is derived from the session handshake and is unique per connection.

### Rate adaptation protocol

The ISS maintains the current speed level (SL). On each ACK received:
- ACK-OK → maintain SL.
- ACK-UP → increment SL by 1 (if not at maximum).
- ACK-DOWN → decrement SL by 1 (if not at SL2).
- NACK → retransmit at current SL; if NACK count exceeds threshold, decrement SL.
- Three consecutive NACK at SL2 → attempt SL1 (chirp) and signal link quality event to application.

The SPUG bit in the data status byte is an *advisory* suggestion from the receiver side; it does not override the ACK type. The ISS is the decision authority on rate changes.

On crossing the SL5→SL6 boundary (SC to OFDM), both parties must agree via the CONREQ/CONACK bandwidth negotiation. If the remote station cannot receive OFDM (capability mismatch), the rate ladder is capped at SL5. This is negotiated at connection establishment, not dynamically.

---

## Memory-ARQ: receiver soft combining

### What it is

Memory-ARQ is a receiver-only enhancement: no wire protocol changes, no transmitter changes. When a packet is received with CRC errors (triggering a NACK), the receiver retains a soft representation of the received signal. When the retransmission arrives, the receiver combines the two soft representations before FEC decoding.

This works because the two transmissions carry the same information bits (identical codeword). The channel errors are statistically independent between attempts (assuming the channel has changed even slightly between transmissions). Combining them gives an effective SNR gain of 3–6 dB on typical HF fading channels.

### Implementation design

**Soft representation to store:** After matched filtering and before symbol decision, the receiver has a complex baseband sample (or soft LLR — log-likelihood ratio) per coded bit. This is the natural representation for Maximum Ratio Combining (MRC).

**Combining rule:**

Equal Gain Combining (EGC, simpler, implement first):
```
combined_llr[i] = attempt1_llr[i] + attempt2_llr[i]
```
Sum the LLRs per bit position across attempts. Feed the combined LLR vector into the FEC decoder. This works well when channel SNR is similar across attempts.

Maximum Ratio Combining (MRC, better, implement as enhancement):
```
combined_llr[i] = w1 * attempt1_llr[i] + w2 * attempt2_llr[i]
```
Weight each attempt by the estimated channel SNR. Optimal when SNR differs significantly between attempts.

**Buffer requirement:**
- One stored attempt per active session.
- Buffer size: one maximum-length data frame worth of soft LLRs.
- At SL11 (largest frame, 1157 user bytes), the data field before FEC ≈ 1157 × 8 × (6/5) ≈ 11,107 coded bits.
- At 32 bits per float: ~43 KB per session. Trivial on any modern platform including Raspberry Pi 4.

**Combine policy:**
- Store on NACK (frame failed CRC).
- Combine on next received attempt for the same sequence number.
- Discard stored attempt on successful ACK or BREAK/QRT.
- Maximum combine depth: 2 attempts (store at most one previous attempt). Deeper combining offers diminishing returns.

**Integration with FEC:**
- For Reed-Solomon (current): LLR combining is at the bit layer before symbol detection. RS operates on bytes; feed combined bits into RS symbol demapper.
- For convolutional + Viterbi (future): LLR combining feeds directly into the Viterbi branch metric computation. This is the natural integration point and where Memory-ARQ is most effective.

---

## Small OFDM receiver design

### FFT size and sample rate

At 48 kHz audio sample rate:

| SL | Subcarriers | Subcarrier spacing | T_fft | FFT samples | T_cp | CP samples | T_sym | Total samples |
|----|------------|-------------------|-------|-------------|------|-----------|-------|--------------|
| 6 | 4 | 125 Hz | 8 ms | 384 | 2 ms | 96 | 10 ms | 480 |
| 7 | 8 | 62.5 Hz | 16 ms | 768 | 2 ms | 96 | 18 ms | 864 |
| 8 | 8 | 287 Hz | 3.5 ms | 168 | 2 ms | 96 | 5.5 ms | 264 |
| 9–11 | 16 | 144 Hz | 6.9 ms | 333 | 2 ms | 96 | 8.9 ms | 429 |

Note: 48000 samples/s × 2ms = 96 samples CP. FFT sizes above are not powers of 2 — implementation may round to the nearest power of 2 (e.g. 512 for SL6, 1024 for SL7, 256 for SL8, 512 for SL9-11) with the extra bins discarded. A 512-point FFT at 48 kHz takes 512/48000 = 10.7 ms — satisfactory for SL6.

Small FFTs (512–1024 point) have negligible computational cost on Raspberry Pi 4 (ARM Neon SIMD). No GPU acceleration is required for the OFDM receiver at these sizes.

### Differential subcarrier detection (SL6–7)

DQPSK per subcarrier: multiply current subcarrier output by complex conjugate of previous symbol's output on the same subcarrier. No pilot, no channel estimate. Decision on the resulting phase difference.

This is the exact approach used by ARDOP for its OFDM modes. It is robust to slow channel phase variation (up to ~1/4 of the subcarrier symbol rate) and requires only 2 FFT operations per symbol period.

### Coherent subcarrier detection with CAZAC training (SL8–11)

Each data block is preceded by a 32-sample CAZAC-16 training sequence (equivalent to PACTOR-4's normal mode design). The training sequence has known complex values; the receiver estimates the channel impulse response by correlating the received training against the known sequence.

CAZAC-16 property: ideal periodic autocorrelation (zero at all non-zero lags). This means one training symbol period yields a perfect single-tap channel estimate per subcarrier with no noise amplification. The estimate is used to correct the phase and amplitude of the subsequent data block symbols before constellation decision.

Training overhead at SL9: 32-sample training / (32 training + 207 data × 16 samples) = 32 / 3344 ≈ 1% overhead. Negligible.

---

## Compression design

### Multi-mode compression with per-packet selection

Inspired by PACTOR-4's status byte M2 M1 M0 compression field. The compressor selects the best algorithm for the current payload and signals the choice in the status byte. The decompressor switches mode per packet with no renegotiation.

**Algorithm selection logic:**

```
for each candidate algorithm in [none, huffman, pseudo-markov]:
    compressed = compress(payload, algorithm)
    if len(compressed) < len(payload):
        use this algorithm
        break
use none if no algorithm improves size
```

**Pseudo-Markov compression:**
A first-order Markov model over the 128 ASCII characters. The transition probability table P[prev_char][next_char] is precomputed for English and German text. Arithmetic coding is applied to the sequence using P conditioned on the previous character.

For English text, empirical compression ratios: Huffman ~1.8:1, Pseudo-Markov English ~2.8:1. For binary data: both algorithms may expand; the "don't compress if larger" rule sends the raw payload.

The English and German Pseudo-Markov tables should be computed from a large text corpus and embedded as compile-time constants (≈ 128×128 × 4 bytes ≈ 64 KB per table, acceptable).

---

## Implementation sequencing

The waveform features described here depend on each other and on platform capabilities. Recommended implementation order:

**Prerequisite (Phase 1, already in roadmap):**
- Interleaver integrated with RS FEC ← required before any HF waveform claims are valid
- SAR sub-layer ← required before large OFDM frames can be carried

**Phase 2 additions (this document):**
1. **4FSK ACK frame** — implement first; enables rate adaptation for all subsequent levels. Requires: audio backend, basic modulator/demodulator infrastructure (done).
2. **Memory-ARQ (EGC)** — implement alongside 4FSK ACK. Requires: FEC decoder to accept soft LLR input; buffer per session.
3. **HPX500 SC levels (SL2–5)** — extend current BPSK/QPSK plugins with FEC+interleaver and the ACK-driven rate ladder.
4. **HPX500 small OFDM (SL6–7, differential)** — implement DQPSK OFDM after SC levels are stable. Requires: small FFT, cyclic prefix insertion/removal.
5. **Pseudo-Markov compression** — can be implemented independently; integrate into status byte encoding.

**Phase 3 additions:**
6. **HPX2300 coherent OFDM (SL8–9, CAZAC training)** — requires CAZAC training sequence insertion and per-subcarrier channel estimation. Implement after SL6–7 are validated.
7. **HPX2300 high-order QAM (SL10–11)** — requires validated channel estimation; SNR measurement for adaptive threshold.
8. **Memory-ARQ MRC** — enhance EGC with SNR-weighted combining after EGC is benchmarked.

**Simulation validation gates before implementation:**
- SL switching SNR thresholds: simulate Watterson M1 BER curves for each SL; set thresholds at the crossover point.
- Cyclic prefix length: verify 2 ms is sufficient for the Poor P2 channel (τ_max ≈ 4 ms); if not, increase to 4 ms at the cost of ~28% overhead in SL9–11.
- Memory-ARQ gain: simulate two-attempt combining under Gilbert-Elliott burst model; verify ≥ 3 dB gain in heavy-burst scenario.

---

## Benchmark integration

Each speed level must have a corresponding benchmark scenario defined in docs/benchmark-harness.md:

- SL2–5 (single-carrier): add to HF500 scenario family with Watterson M1/M2 and Gilbert-Elliott moderate/heavy burst profiles.
- SL6–7 (small OFDM): add HF500-OFDM-01 through HF500-OFDM-04.
- SL8–11 (2300 Hz OFDM): add HF2300-OFDM-01 through HF2300-OFDM-04 with appropriate Watterson P1/P2 stress variants.

Acceptance criterion for each SL: must achieve ≥ 90% of theoretical net data rate at the nominal SNR threshold under Watterson M1 (5 MHz path).
