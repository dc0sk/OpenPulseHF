---
project: openpulsehf
doc: docs/dev/research/scfdma-improvements.md
status: living
last_updated: 2026-07-08
---

# SC-FDMA waveform improvements — research

A prioritized, code-grounded list of ways to further improve the SC-FDMA waveform. From a research
pass over `plugins/scfdma/`, the shared DSP crate, the FEC crates, and the SC-FDMA profile rungs.
No code was changed. Effort is S/M/L.

## Ground truth (corrections to earlier assumptions)

- **Pilots are already comb-type in frequency** (every 5th subcarrier in *every* symbol,
  `plugins/scfdma/src/channel.rs:13`). "Block pilots" only described the dropped `SCFDMA52-LP`
  demonstrator. The real pilot questions are the *time-direction* processing of the comb (currently a
  causal EMA only) — not comb-vs-block.
- **Demodulation is batch** (`demodulate.rs:134`) — the whole frame is in memory, so non-causal
  (two-pass / centered) processing is essentially free; the current one-sided EMA does not exploit it.
- **A dedicated frequency-acquisition stage already exists and is unused**:
  `openpulse-dsp/src/freq_acquire.rs::acquire()` (qdetector-style joint timing+CFO+phase+gain), with no
  caller anywhere.
- **AGC is already engine-level** (`openpulse-modem/src/engine.rs`), so "no AGC" is addressed upstream.
- **Adaptive pilot density is dead plumbing** (`lib.rs:82`) — no engine caller, and pilot spacing is
  wire format, so it cannot activate without a negotiation protocol; it is a latent footgun.
- **Per-subcarrier bit-loading is inapplicable** — the DFT spread makes every data symbol see the
  *average* channel; there is no per-SC symbol to load. The codebase's answer is the narrowband
  SCFDMA26 family (already shipped).

## Prioritized improvements

| # | Improvement | Benefit | Effort | Risk | Building block |
|---|---|---|---|---|---|
| **P1** | **Wire in `freq_acquire::acquire()`** for one-shot joint timing+CFO, replacing/augmenting `find_sync_offset` | Single-shot acquisition over the full ±60 Hz window; removes the ±4 Hz own-sync fragility; frees phase+gain to seed CE | M | Low–Med | exists, unused (`freq_acquire.rs`) |
| **P2** | **Common-phase-error (CPE) removal per symbol + non-causal CE smoothing** — de-rotate `raw_h` by the pilot-mean phase; replace the causal EMA with a centered / forward-backward pass | ~1–2 dB on slow-fading dense modes; adds the intra-frame CFO/Doppler tracking the code currently lacks | S/M | Low | `pilot_tracker.rs`, `doppler_tracker.rs` |
| **P3** | **Smooth the noise-variance estimate across symbols** (EMA/median) — per-symbol σ² has ~50% relative std with only 4 residual dof | Stabler MMSE + calibrated LLRs → a few tenths dB soft-FEC gain; nearly free | S | Very low | trivial |
| **P4** | **Fix the hard-demod MMSE amplitude bias + GPU-path divergence** (correctness): hard path omits the `/alpha_avg` the soft path applies; both GPU paths skip `deramp_timing`+CE smoothing | ~0.5–1 dB on hard-path QAM near threshold; stops silent CPU/GPU divergence under SRO | S | None | mirrors soft path |
| **P5** | **Second-pass decision-directed CE** — re-spread sliced symbols as virtual pilots → 100% density for a 2nd CE/equalize pass | ~1–2 dB under frequency-selective fading; removes the CE-noise floor | M (L for full turbo-CE) | error propagation (gate on 1st-pass quality) | `IterativeDecoder` trait exists |
| **P6** | **LDPC on the dense rungs (SL8–SL11)** — swap `SoftConcatenated` for `LdpcHighRate`/`Ldpc`, keep the interleaver | ~1–3 dB vs RS soft-concat at the same rate; lowers the ladder floors; exploits the max-log LLRs already emitted | M (calibration, not DSP) | Low–Med | `LdpcCodec` + engine plumbing exist |
| **P7** | **Frequency-domain iterative block DFE (IBDFE)** after MMSE — cancel residual ISI at spectral notches over 1–2 iterations | 1–2 dB on frequency-selective (Watterson) channels for 16/64QAM | M/L | Med (convergence < ~10 dB) | ~100 lines; `dft`/`idft` plans in scope |
| **P8** | **TX windowing + CP tuning** — raised-cosine edge windowing (WOLA); optionally halve CP 32→16 samples | Windowing = adjacent-channel/regulatory hygiene; CP-16 = +5.9% throughput | S (S but wire-incompatible for CP) | Low (windowing); Med (CP) | — |

**Deprioritized:** π/2-BPSK pilot shaping (value is PAPR of TDM pilots; PAPR work was dropped) and
TDM DMRS pilot symbols (the per-symbol comb tracks Doppler strictly better; not worth a wire break).

## Weak / fragile spots in the current demod path

1. **Hard-path MMSE amplitude bias** — no `/alpha_avg` before demap; biases QAM hard decisions toward
   the origin at exactly the ladder-probe SNRs (P4).
2. **GPU paths skip `deramp_timing` + CE smoothing** — silent CPU/GPU divergence under sample-rate
   offset; GPU hard path also carries the alpha bias.
3. **Causal EMA CE lags phase under residual CFO/Doppler** — a ~1 Hz residual never trips the jump-reset
   yet steadily rotates the estimate; **no intra-frame CFO/CPE tracking at all** (P2).
4. **Watterson fading is a documented failure regime** — 16/64QAM fail the 90% HF gate on all tested
   scenarios; QPSK+RS decodes only ~6% of good-F1 seeds without Memory-ARQ (SC-FDE notch-smearing, P7).
5. **Sync fragility** — ±4 Hz own tolerance; `estimate_cfo_hz` aliases beyond ±13.9 Hz; the `rho=0.15`
   detection floor is untested against fading/impulse noise.
6. **Adaptive pilot density can't deploy** (no negotiation) — wire it or delete it (footgun).
7. **`SCFDMA52-LP` mis-decode risk** is documented but the mode stays user-selectable.

## Do next (top 3)

1. **P2 — CPE tracking + two-pass CE smoothing** (S/M): cheapest real dB against the documented
   Watterson failure mode, pure RX-side, wire-compatible, and a prerequisite for everything iterative.
   Bundle P3 + P4 into the same PR — they touch the same few lines.
2. **P1 — wire in `freq_acquire`** (M): a finished, tested module sits unused while the waveform lives
   with ±4 Hz sync tolerance and an iterative AFC crutch; largest robustness win per new line.
3. **P6 — LDPC on the dense rungs** (M): no new DSP — the codec + engine plumbing exist; 1–3 dB of
   ladder-floor improvement for calibration effort, and it makes the eventual P5 turbo-CE loop matter.

> Any mode/FEC change alters `SessionProfile::fingerprint()` (an intentional interop version break);
> RX-only fixes (P2/P3/P4) and floor retunes do not. Re-run `snr_floor_calibration` after P6/P7.
