---
project: openpulsehf
doc: docs/backlog-waveforms.md
status: living
last_updated: 2026-05-09
---

# Waveform / Multi-Carrier Scheme Backlog

**Frozen research summary (2026-05-09).** Archived; see docs/dev/backlog.md for live open work.

Research conducted 2026-05-09. Current state: OFDM (FF-4) with iterative PAPR clipping, LS channel estimation, ZF equalization.

---

## FF-12 — FBMC, UFMC, GFDM, SC-FDMA, OFDMA: HF applicability analysis

The question is whether any of these 5G-era waveforms would materially improve on OFDM for the specific constraints of HF amateur radio:

- **Narrow occupied bandwidth** (500–3000 Hz audio passband)
- **Slow, sparse channel impulse response** (Watterson: 0.5–2.0 ms delay spread = 4–16 samples at 8 kHz)
- **Power-limited TX** (typical 100 W SSB; PA efficiency and PAPR matter)
- **Peer-to-peer, not scheduled access** (HPX is point-to-point ARQ, not base-station/UE)
- **Low subcarrier count** (OFDM16: 16 data SCs; OFDM52: 52 data SCs)

---

### FBMC (Filter Bank Multi-Carrier)

**What it does:** Replaces the CP with per-subcarrier pulse shaping using a prototype filter (e.g., IOTA or PHYDYAS). Symbols overlap in time; receiver separates them using the filter bank structure.

**Advantages over OFDM:**
- No cyclic prefix → 14% throughput gain at our CP/FFT ratio (32/256)
- Lower out-of-band emission — side lobe suppression ~50 dB vs ~13 dB for rectangular OFDM
- More spectrally efficient in crowded bands

**HF problems:**
- Overlapping symbols break per-symbol channel estimation. LS+ZF works per-symbol; FBMC needs multi-symbol (iterative) equalizers, e.g., MMSE with interference cancellation.
- The PHYDYAS filter has a tail of ~4× symbol durations. At 8 kHz/288-sample symbols, this is ~1152 samples = 144 ms of transient at each burst start and end — devastating for short HF frames.
- Complexity: receiver is significantly more involved than one-tap ZF; no mature pure-Rust implementation.

**Verdict: Not beneficial for HF.** The CP overhead (14%) is outweighed by the long filter transient and equalization complexity. CP is cheap insurance against HF delay spread; FBMC trades it for spectral neatness that does not matter inside a 3 kHz SSB passband.

---

### UFMC (Universal Filtered Multi-Carrier)

**What it does:** Applies a short FIR filter per *sub-band* (a group of SCs) rather than per-SC (FBMC) or no filter (OFDM). Retains CP. Filter length is typically 10–25 taps.

**Advantages over OFDM:**
- Reduced OOBE relative to windowed OFDM
- Compatible with CP-based one-tap equalization
- Shorter filter transient than FBMC

**HF problems:**
- The spectral improvement is most valuable where many sub-bands must coexist without a common timing reference (asynchronous uplink in 5G). In HF point-to-point links both ends are synchronised by the waveform itself.
- Within a 3 kHz SSB passband, adjacent-sub-band leakage is irrelevant — the SSB filter and the rig's IF chain provide far more suppression than UFMC's sub-band filter.
- Implementation adds complexity (sub-band filter bank on TX and RX) for no measurable HF benefit.

**Verdict: Marginal benefit, not worth implementing.** Windowed OFDM (apply a Hann or raised-cosine window to each symbol in time domain) achieves similar OOBE reduction with a one-line change. Add windowing to the OFDM TX before the CP if OOBE ever becomes an issue.

---

### GFDM (Generalized Frequency Division Multiplexing)

**What it does:** Each SC uses circular pulse shaping (root-raised-cosine or similar). The modulation is inherently non-orthogonal — adjacent SCs introduce self-interference, which is cancelled at the receiver via successive interference cancellation (SIC) or matrix inversion.

**Advantages over OFDM:**
- OOBE lower than OFDM, approaching FBMC levels
- Flexible symbol structure — can trade time-frequency granularity
- Lower PAPR than standard OFDM (pulse shaping reduces peak-to-average ratio somewhat)

**HF problems:**
- Non-orthogonality requires SIC at the receiver or matrix-inversion equalizer. For 52 SCs, the latter is a 52×52 complex matrix inversion per symbol — computationally expensive and numerically sensitive with HF fading.
- Channel estimation for non-orthogonal waveforms requires pilot designs that account for inter-carrier interference; LS+ZF no longer applies directly.
- PAPR reduction is modest (1–2 dB) compared to the 3–4 dB achievable with SC-FDMA.

**Verdict: Not beneficial.** High receiver complexity, non-trivial channel estimation, small PAPR benefit. The only plausible motivation (OOBE) can be addressed more simply.

---

### SC-FDMA (Single-Carrier FDMA) — **Most promising**

**What it does:** DFT-spread OFDM. TX applies an M-point DFT to the data symbols before mapping them onto M subcarriers, then runs the standard N-point IFFT (N > M). The result is a single-carrier-like time-domain signal occupying the same bandwidth as OFDM. RX applies one-tap ZF per SC after the FFT, then an M-point IDFT to recover data.

**Advantages over OFDM:**
- PAPR ≈ 3–4 dB lower than OFDM for the same modulation (single-carrier envelope in time domain).
- Allows replacing iterative clipping with an inherently lower-PAPR waveform — simpler TX, lower spectral regrowth, better PA efficiency.
- One-tap channel equalization identical to OFDM → LS+ZF estimation still applies.
- Wire-compatible frame structure (same CP, same FFT size) — RX only needs to add an IDFT stage.
- Used in LTE uplink (3GPP TS 36.211) — well-studied for multipath channels.

**HF benefits:**
- Power-limited HF TX benefits directly from 3–4 dB lower PAPR: either higher average power for the same peak, or PA operating further from saturation.
- Eliminates the iterative clipping loop entirely (no distortion noise floor, no OOB regrowth from clipping).
- Same pilot-based LS channel estimation can be reused.

**HF problems / open questions:**
- DFT spreading spreads each data symbol across all M subcarriers — a deep fade on one SC corrupts all M symbols (frequency diversity is lost, unlike OFDM where a faded SC loses only its own data). For HF channels with selective fading, this is a real trade-off.
- Requires localized subcarrier allocation (SCs must be contiguous) for the DFT spread to form a single-carrier signal; this matches OFDM16/OFDM52 naturally.
- No existing HF amateur radio ecosystem reference; would be novel.

**Implementation scope:** New `plugins/scfdma/` crate; add DFT spread/despread steps around the existing OFDM IFFT/FFT; reuse `channel.rs` pilot, LS, ZF unchanged. Estimated ~200 additional lines beyond ofdm-plugin.

**Verdict: Worth implementing as FF-12 if PAPR measurements on real HF TX show clipping distortion is limiting.** Run a controlled test: compare OFDM52 with clipping vs SC-FDMA52 OOB spectrum on a loopback at peak envelope power. If clipping introduces measurable OOB regrowth or SNR floor, SC-FDMA is the right answer.

---

### OFDMA (Orthogonal Frequency Division Multiple Access)

**What it does:** Multi-user variant of OFDM. A central scheduler assigns disjoint subcarrier groups to different users simultaneously. Users share time-frequency resources without colliding.

**HF applicability: None.** OFDMA requires a base station or net control station that knows which users need to transmit and assigns SC groups dynamically. HPX is fundamentally peer-to-peer ARQ — there is no scheduler. Even in a net or relay scenario, implementing an OFDMA scheduler would require a MAC layer redesign beyond the current session model.

The only conceivable use case: a future `openpulse-mesh` net-control node assigns OFDMA slots to multiple stations simultaneously. This is a multi-year architectural change with very limited HF benefit (HF channels change on timescales shorter than typical OFDMA scheduling cycles).

**Verdict: Not applicable to current or near-term architecture.**

---

## Summary table

| Scheme | HF PAPR benefit | Channel estimation | Complexity | Verdict |
|---|---|---|---|---|
| FBMC | None (no CP gain) | Multi-symbol iterative | Very high | Not beneficial |
| UFMC | Marginal OOBE | One-tap ZF | Medium | Not worth it; use windowing instead |
| GFDM | Small (1–2 dB) | Non-trivial (SIC/matrix) | High | Not beneficial |
| **SC-FDMA** | **3–4 dB** | **One-tap ZF (unchanged)** | **Low** | **Investigate as FF-12** |
| OFDMA | N/A | N/A | Very high (scheduler) | Not applicable |

## Recommended next step (FF-12)

Before implementing SC-FDMA, run a comparative PAPR and OOB distortion measurement between OFDM52 (with iterative clipping) and a prototype SC-FDMA52 (no clipping) using the `openpulse-testbench` spectrum view at high drive levels. If the clipping floor is measurable at -40 dBc or worse relative to the wanted signal, SC-FDMA is the right upgrade path.

**Acceptance criterion for FF-12 implementation:** SC-FDMA52 loopback at Watterson F1 SNR=15 dB achieves equal or better BER than OFDM52 with clipping, with ≥3 dB lower peak power.
