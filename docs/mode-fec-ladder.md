# Mode and FEC ladder — how the modem chooses a waveform and a code

OpenPulseHF picks an operating point along **two independent axes**:

1. **Modulation** — the waveform and constellation. Trades spectral efficiency
   (bits/s/Hz) against the SNR needed to keep symbol errors low.
2. **Forward error correction (FEC)** — trades net throughput (coding overhead)
   against the channel error rate it can clean up.

A link's usable operating point is the highest-throughput combination whose
**post-FEC** error rate is acceptable at the link's SNR and fading profile. The
adaptive rate controller walks the modulation axis automatically; the FEC is
chosen per session/use-case (and, for the dense modes, is not optional).

---

## 1. The modulation ladder (least → most demanding)

Two knobs set how demanding a mode is:

- **Constellation order** — bits per symbol. Each step roughly doubles the data
  but needs more SNR and a tighter phase/timing lock:
  BPSK (1) → QPSK (2) → 8PSK (3) → 16QAM (4) → 32QAM (5) → 64QAM (6).
- **Occupied bandwidth** — baud rate (single-carrier) or subcarrier count
  (SC-FDMA/OFDM). For a *fixed transmit power*, a **narrower** signal puts more
  power per Hz, so it needs less SNR — at the cost of throughput. This is why
  `SCFDMA26-*` (≈1 kHz, 26 SCs) is ~3 dB more robust than `SCFDMA52-*`
  (≈2 kHz, 52 SCs) at the same constellation.

Approximate clean-AWGN SNR each constellation needs to decode (uncoded, before
FEC), measured in sim on the SC-FDMA path:

| Constellation | Bits/sym | ~Uncoded SNR floor | Notes |
|---|---|---|---|
| BPSK / QPSK | 1–2 | ≤ ~10 dB | Wide decision regions; very forgiving |
| 8PSK | 3 | ~13–15 dB | 45° margins |
| 16QAM | 4 | ~17–18 dB | |
| 32QAM (cross) | 5 | ~22–23 dB | ~5 dB easier than 64QAM |
| 64QAM | 6 | ~25–26 dB | Needs tight clock + carrier |

**Single-carrier vs multi-carrier.** Single-carrier PSK tolerates clock/timing
offset well (wide eye, carrier loop). SC-FDMA/OFDM pack more constellation into a
slice with low PAPR and pilot-aided equalization, but the DFT de-spread makes them
sensitive to residual per-subcarrier phase — handled by the per-symbol pilot
`deramp_timing` (see §5). RRC variants (`-RRC`, α = 0.35) add ~35 % bandwidth for
a cleaner spectrum and better timing recovery; the plain (Hann/rectangular) 2000-baud
modes close the eye at 4 samples/symbol and are superseded by their `-RRC` siblings.

---

## 2. The FEC ladder (least → most powerful)

| FEC mode | Code | Rate | Corrects | Input | Best for |
|---|---|---|---|---|---|
| `None` | — | 1.00 | nothing | — | Clean loopback / very high SNR only |
| `Rs` | RS(255,223), t=16 | 0.875 | ≤ 6.3 % byte errors/block | hard | Light random errors |
| `RsInterleaved` | RS + block interleaver | 0.875 | 6.3 %, **burst-tolerant** | hard | HF burst/fading (Gilbert-Elliott) |
| `RsStrong` | RS(255,191), t=32 | 0.749 | ≤ 12.5 % byte errors/block | hard | Heavier random errors, hard-decision |
| `Concatenated` | Conv(½,K=3) + RS | ~0.44 | high (random) | hard | AWGN-dominant, no soft LLRs |
| `SoftConcatenated` | Soft-Viterbi(K=7) + RS | ~0.44 | **highest practical** | **soft** | Dense modes on real links |
| `Ldpc` | rate-1/2 LDPC (min-sum) | 0.50 | very high | **soft** | Short blocks (≤128 B), soft |
| `Turbo` | rate-1/3 PCCC | 0.33 | very high | **soft** | Maximum robustness, low rate |

Two rules of thumb:

- **Soft beats hard by ~3–4 dB** when the modulation emits real LLRs. Every
  OpenPulseHF data plugin provides `demodulate_soft`, so `SoftConcatenated` /
  `Ldpc` / `Turbo` get genuine soft-decision gain — they are not equivalent to
  their hard counterparts.
- **Interleaving matters on HF.** Watterson/Gilbert-Elliott channels produce
  *bursts*; `RsInterleaved` spreads a burst across many RS codewords so it stays
  within per-block capacity, where bare `Rs` would fail on the same raw rate.

---

## 3. The decision process

```
        ┌─────────────────────────────────────────────────────────┐
  SNR → │ rate controller picks the SpeedLevel (→ modulation mode) │
        │   • below a level's SNR floor   → step DOWN one rung      │
        │   • above its SNR ceiling (+ACK) → step UP one rung       │
        │   • N consecutive NACKs          → step DOWN              │
        └─────────────────────────────────────────────────────────┘
  channel type ─────────────────────────────────────────────► FEC family
   • AWGN / high SNR        → None / Rs
   • HF burst & fading      → RsInterleaved (burst) or SoftConcatenated
   • dense constellation    → soft code REQUIRED (SoftConcatenated/Ldpc/Turbo)
```

1. **Estimate SNR** (from soft-LLR magnitude on RX) and feed it to the rate
   adapter, which maps it to a `SpeedLevel` in the active profile (§4). Each rung
   has an SNR floor (drop below → step down immediately) and ceiling (rise above,
   with a positive ACK → climb).
2. **Pick the FEC** for the channel character, not just the SNR:
   - flat/AWGN, plenty of margin → `None` or `Rs`;
   - HF multipath/fading (bursts) → `RsInterleaved`;
   - any dense mode (16QAM and up, all SC-FDMA HOM, 64QAM) → a **soft** code.
3. **Acceptable** = the post-FEC frame CRC passes reliably. The headline number is
   *net* throughput = `gross_bps × code_rate × (1 − retransmit_fraction)`.

---

## 4. Adaptive profiles (the SpeedLevel ladders)

Each profile is a `SpeedLevel → {mode, SNR floor, SNR ceiling}` map in
`crates/openpulse-core/src/profile.rs`. The controller starts at `initial_level`
and walks up/down.

| Profile | Rungs (low → high) | Use |
|---|---|---|
| `hpx500` | BPSK31 → BPSK63 → BPSK250 → QPSK250 → QPSK500 | Robust narrowband HF |
| `hpx_hf` | …BPSK → QPSK → 8PSK500 → SCFDMA52-8PSK | HF-compliant, ≤2 kHz |
| `hpx_wideband_hd` | **SCFDMA26-8PSK/16QAM/32QAM (SL9–11)** → SCFDMA52-16QAM/-32QAM/-64QAM → 64QAM2000-RRC (SL12–15) | Wideband HD; the SL9–11 narrowband rungs are the graceful-degradation path |
| `hpx_narrowband_hd` | QPSK/8PSK 9600-RRC | Post-1.0 (wider than 3 kHz; deferred) |

The key design point in `hpx_wideband_hd`: when the link cannot sustain the
full-width SL12+ modes, the controller drops onto the **half-width `SCFDMA26-*`
rungs** (SL9–11) — same constellations, ~+3 dB per-subcarrier SNR — instead of
falling all the way back to QPSK.

---

## 5. Which mode/FEC combinations make sense

Robustness (left) vs throughput (right). "Net bps" is gross × code-rate (the
retransmit cost is on top of that). Recommended HF pairings:

| Operating regime | Mode | FEC | ~Net bps | Why |
|---|---|---|---|---|
| Weak signal, NVIS, QRM | BPSK31–250 | `RsInterleaved` | 25–220 | Burst-tolerant; QPSK/BPSK shrug off timing offset |
| Solid HF, ~2 kHz | QPSK500 / SCFDMA52 | `RsInterleaved` | ~900 / ~2 500 | Workhorse; soft optional |
| Good HF, want more | 8PSK500 / SCFDMA52-8PSK | `SoftConcatenated` | ~660 / ~1 900 | 8PSK needs the soft-coding gain |
| Marginal-SNR dense (the SL9–11 fallback) | **SCFDMA26-16QAM / -32QAM** | **`SoftConcatenated`** | ~1 270 / ~1 590 | **+3 dB narrowing + soft FEC — hardware-validated reliable** |
| High SNR, ~2 kHz, max data | SCFDMA52-16QAM/-32QAM | `SoftConcatenated` | ~2 540 / ~3 180 | Soft FEC closes them where hard RS can't |
| Very high SNR (≥25 dB) | SCFDMA52-64QAM / 64QAM2000-RRC | `SoftConcatenated` / `Ldpc` | ~3 800 / ~5 300 | Only on excellent links / on-air with margin |

Combinations that **don't** make sense:

- **Any dense mode (16QAM+) with `None` or bare `Rs`.** Validated on hardware:
  the full-width SCFDMA52-HOM modes fail no-FEC and fail with hard RS; soft FEC
  (`SoftConcatenated`) is what closes 8PSK, and narrowing **plus** soft FEC closes
  16QAM/32QAM reliably. RS(255,223)'s 6.3 % capacity is simply below what these
  modes leave on a realistic channel.
- **64QAM single-carrier on a marginal link.** It needs ~25–26 dB and a tight
  clock; below that no FEC rescues it economically. Use it only when the link
  genuinely supports it (then `SoftConcatenated` for margin).
- **`Turbo`/`Ldpc` on a clean, high-SNR link.** Their low code rate (0.33/0.5)
  throws away throughput you don't need to spend; prefer `Rs`/`RsInterleaved`
  there.

---

## 6. Empirical anchors (hardware loopback, rpi51↔rpi52)

- Single-carrier BPSK/QPSK/8PSK and `SCFDMA16` decode reliably no-FEC.
- `SCFDMA52` (QPSK) required a per-symbol pilot **`deramp_timing`** to survive the
  two-soundcards sample-rate offset (it removes the SFO phase ramp the DFT
  de-spread otherwise amplifies); after that it passes.
- The dense `SCFDMA52-8PSK/16QAM/32QAM` are SNR-bound no-FEC; with
  `SoftConcatenated` 8PSK passes and 32QAM is intermittent, and with the
  half-width `SCFDMA26-*` **+** `SoftConcatenated` all three pass reliably.
- **64QAM single-carrier under sample-rate offset** is marginal (≈2 % byte errors
  at 100 ppm with two-pass carrier tracking) and is SNR-bound on the test cable, so
  it can only be validated in simulation, not on the current hardware. It remains a
  documented, lower-priority item rather than a v1.0 blocker.
