---
project: openpulsehf
doc: docs/dev/research/reference-mining-plan.md
status: proposal — awaiting review/decision
last_updated: 2026-06-17
---

# Reference-mining plan — ideas harvested from external modem/DSP source

A source-level scan of the five reference projects catalogued in
[references.md](references.md), extracting **every** technique that could benefit
OpenPulseHF — including marginal-benefit and high-effort ones — classified by
benefit, effort, licence, and fit, for the maintainer to **review and decide**.
Nothing here is implemented yet; this is a prioritized catalog plus a
recommendation.

## Method

Each repo was shallow-cloned to `/tmp/refmining` and the **actual source** read
(not READMEs). One mining agent per repo produced a cited catalog; this document
is the deduplicated, cross-referenced synthesis. Three claims were verified
against our own tree (AGC absence, ARQ retransmit path, SC-FDMA PAPR-measurement
basis) and the findings are folded in below.

## Licence — not a blocker for anything here

OpenPulseHF is **GPL-3.0-or-later**. Therefore:

| Source | Licence | Portable into our tree? |
|---|---|---|
| jgaeddert/liquid-dsp | MIT-style | Yes — freely (most permissive) |
| gnuradio/gnuradio | GPL-3.0 | Yes — with attribution |
| daniestevez/qo100-modem | GPL-3.0 | Yes — with attribution |
| dj0abr/SSB_HighSpeed_Modem | GPL-3.0 | Yes — with attribution |
| Rhizomatica/mercury | GPL-3.0 / LGPL-2.1 | Yes — with attribution |

We still reimplement in Rust (from-scratch protocol), but for these sources code
*may* be ported, not only clean-roomed. Licence is therefore **not** a gating axis
for any item; effort and fit are.

**Legend** — Benefit: 🟢 High · 🟡 Medium · ⚪ Marginal. Effort: S(mall) · M(edium) · L(arge).

---

## Where the references converge (the strongest signals)

The most reliable guide to "what we're missing" is where **independent** fielded
modems solve the same problem the same way. Four convergences stand out:

1. **A dedicated frequency-acquisition stage before phase tracking** — implemented
   *three independent ways*: gnuradio **FLL band-edge**, liquid-dsp **qdetector**
   (FFT-domain coarse-CFO sweep + fine de-rotated-FFT), qo100 **FFT acquisition
   seeded from a pilot PRBS**. Our single decision-directed Costas + ISI-biased
   preamble AFC is the outlier — this is the root of every carrier-offset gap we've
   chased (BPSK31, QPSK500, 8PSK, RRC modes). **This is the #1 finding.**

2. **Automatic Gain Control** — liquid-dsp (exp-envelope + squelch FSM) and qo100
   (pilot-based AGC) both have it; **we have none** (verified: 0 hits for `agc` in
   `crates/` + `plugins/`). On HF with 20–40 dB QSB and inter-station level spread,
   this is a real operational gap, not a nicety.

3. **Pilot-aided synchronisation** — qo100 (TDM BPSK pilots → pilot PLL for
   phase/freq/AGC/SNR) and liquid-dsp (`qpilotsync`, FFT on de-rotated pilots) both
   use *data-aided* tracking that is immune to decision errors and cycle slips —
   exactly the failure mode that makes our dense constellations (8PSK/64QAM) fragile.

4. **Polyphase-filterbank symbol timing** — gnuradio (`pfb_clock_sync`,
   `symbol_sync_cc`) and liquid-dsp (`firpfb`/`symsync`) both replace our
   "Gardner TED + linear interpolation" with exact fractional-delay matched
   filtering. The single-carrier references all pair this with **RRC shaping** (our
   recurring lesson from references.md).

Everything else is incremental or strategic. The tiers below are ordered by
benefit ÷ effort, with verify-first items at the top.

---

## Tier 0 — Verify first (cheap checks; a possible bug or free dB)

| # | Idea | Source | Benefit | Effort | Why |
|---|---|---|---|---|---|
| T0.1 | **ARQ retransmit identity** | mercury `datalink_arq/arq_fsm.h:226`, `arq_fsm.c:573` | 🟢 | S | Mercury saves each sent frame to a dedicated `tx_retransmit_buf` and *replays it* on retry; re-reading the TX ring instead corrupts the stream (documented historical bug). Our ARQ was refactored since CLAUDE.md (`arq_session.rs` is gone; logic now in [harq.rs](../../crates/openpulse-modem/src/harq.rs) + [rate_policy.rs](../../crates/openpulse-modem/src/rate_policy.rs)). [fec.rs:408](../../crates/openpulse-core/src/fec.rs#L408) says we "retransmit the same frame; only the receiver accumulates" — our HARQ LLR-combining *requires* byte-identical retransmissions, so any divergence would also silently break soft-combining. **Verify the current retry path replays the exact prior frame.** |
| T0.2 | **LLR σ² normalisation audit** | qo100 `ldpc/aff3ct-sims/*.txt:55` (`MAXSS`, `sigma_square=on`) | 🟡 | S | LDPC soft-decode assumes channel LLRs scaled by 1/σ². Audit that our 64QAM/SCFDMA demod LLRs into `ldpc.rs` are noise-variance-scaled (SCFDMA already estimates σ² via `mmse_llr_noise_var`; check the single-carrier QAM path). Mis-scaling costs 0.3–1 dB — potentially free to recover. |
| T0.3 | **LDPC syndrome early-termination** | qo100 `ldpc/aff3ct-sims/*.txt:49` | 🟡 | S | Add a zero-syndrome check after each BP iteration in [ldpc.rs](../../crates/openpulse-core/src/ldpc.rs); stop early. 10–100× average latency reduction at good SNR; zero risk, pure optimisation. |

---

## Tier 1 — Quick wins (small effort, clear additive benefit)

| # | Idea | Source | Benefit | Effort | Notes |
|---|---|---|---|---|---|
| T1.1 | **AGC + squelch FSM** | liquid-dsp `agc/src/agc.proto.c:148,422`; qo100 pilot-AGC `pilot_pll_impl.cc:115` | 🟢 | S | We have **none**. Exp-envelope gain update + `lock()` after burst-detect (so gain changes don't corrupt LLR scaling). Insert before the matched filter. Biggest correctness gap for real-HF operation. |
| T1.2 | **NLMS equalizer step** | gnuradio `adaptive_algorithm_nlms.h:50` | 🟡 | S | ~3-line change inside our `LmsEqualizer::update`: normalise step by ‖input‖². Stops amplitude-fading-induced divergence/instability. Floor the denominator. |
| T1.3 | **SNR / Es-N0 estimator** | qo100 `differential_8psk/Analysis.ipynb` cells 16–18 | 🟡 | S | Measure noise floor in a quiet window (our DCD already finds silence); `C/N0 = 10log10((P_sig+n − P_n)/P_n · fs)`. Enables SNR-driven rate adaptation, LLR scaling, and a real TUI link-quality number. Pilots (T2.4) give a continuous in-band version. |
| T1.4 | **Loop-filter gains from ζ/ω_n** | gnuradio `clock_tracking_loop.{h,cc}` (bilinear-transform formulas) | 🟡 | S | Replace empirically-tuned Gardner/Costas gains with gains derived from damping ζ + normalised natural freq ω_n. Lets us change loop bandwidth meaningfully and documents why current constants work. |
| T1.5 | **Tracking-loop bandwidth ratios** | SSB `hsmodem/symboltracker.cpp:105` | 🟡 | S | Fielded 2.7 kHz-SSB calibration: AGC=0.02·bw, symsync=0.001·bw, EQ=0.02·bw, PLL=0.001·bw; switch LMS to decision-directed after a 200-symbol warmup. Config guidance, ~zero code. |
| T1.6 | **SNR-weighted Costas error** | gnuradio `costas_loop_cc_impl.cc:97` | 🟡 | S | Divide the Costas phase error by a noise-power estimate so the loop is not driven by noise at the bottom of a fade — fewer cycle slips on 8PSK/64QAM. Needs a signal-vs-noise estimate (pairs with T1.3). |
| T1.7 | **Scrambler / spectral whitening** | SSB `hsmodem/scrambler.cpp` (gnuradio LFSR `header_format_ofdm.cc:69`) | ⚪ | S | XOR payload (post-FEC) with an m-sequence to break repetitive-fill spectral lines that fool timing/AGC. Our interleaver only partially does this. Must wrap the FEC block exactly. |

---

## Tier 2 — High-value, medium effort (contained new capability)

### T2.1 — Burst-mode frame synchronizer (coarse+fine CFO) 🟢 M — *the flagship DSP fix*
- **Source:** liquid-dsp `framing/src/qdetector.proto.c:491–752` (MIT, freely portable);
  alternatives: gnuradio `fll_band_edge_cc_impl.cc` (RRC-only), qo100 `acquisition_impl.cc:82`.
- **What:** FFT-domain cross-correlation sweep gives coarse CFO + timing from the
  preamble in one O(N log N) pass; a second pass de-rotates by the known sequence
  and finds the residual CFO as a quadratically-interpolated FFT peak — returning
  τ̂, γ̂ (gain), Δφ̂ (freq), φ̂ (phase) atomically. This is precisely the
  "coarse-from-preamble, fine-from-payload" two-stage we lack.
- **Why us:** Directly closes the carrier-offset acquisition gaps that have cost
  dozens of commits. Replaces the brittle "settle AFC on a coarse/mostly-silent
  gate window" path (the QPSK500 #413 failure class).
- **Fit/risk:** Needs `rustfft`; the FFT-sweep range maps to our ±baud/4 target.
  **Best paired with T3.7** (a longer m-sequence preamble) for reliable coarse CFO —
  our 16-symbol preamble only resolves ~baud/16. **Recommend qdetector** over FLL:
  it matches our burst (not continuous) model and is licence-unencumbered.

### T2.2 — Generic soft-LLR via nearest-neighbour table 🟡 S
- **Source:** liquid-dsp `modem/src/modem_common.proto.c:473,649` (MIT).
- **What:** Pre-compute the *p* nearest neighbours per constellation point; at
  demod, LLR per bit = (min-dist-to-0 − min-dist-to-1)·γ. One generic path for any
  constellation (PSK/QAM/APSK), O(p) not O(M).
- **Why us:** We hand-derive max-log-MAP per constellation (8PSK, 64QAM). A generic
  table removes that work for every future mode (APSK, π/2-BPSK) and is the natural
  home for the σ² scaling from T0.2.

### T2.3 — Rotation-invariant frame sync 🟡 S
- **Source:** SSB `hsmodem/frame_packer.cpp:75–242` (GPL).
- **What:** Pre-rotate the sync header into all M constellation rotations; match any
  rotation (≤2 bit errors); the matched rotation index tells the decoder how much to
  de-rotate. Frame detection no longer depends on the carrier loop having already
  resolved the ±90°/±45° phase ambiguity; CRC-16 is the false-trigger backstop.
- **Why us:** Removes a real burst-onset failure mode for QPSK/8PSK independent of
  the carrier tracker — complements T2.1 cheaply.

### T2.4 — Pilot-aided sync block (phase + freq + AGC + SNR) 🟢 M
- **Source:** qo100 `pilot_pll_impl.cc`, `acquisition_impl.cc`, `constants.py` (GPL);
  liquid-dsp `qpilotsync.c` (MIT).
- **What:** Insert known pilot symbols (qo100: BPSK every 50 symbols, 2% overhead);
  a type-2 pilot PLL tracks phase + frequency, plus pilot-based AGC and a continuous
  in-band SNR estimate. Data-aided ⇒ immune to decision errors and cycle slips.
- **Why us:** The robust way to run dense constellations (32APSK, 64QAM) on HF.
  Requires a **new framed waveform** (pilot positions known to the decoder) — bundle
  with T3.1 (32APSK). Gives T1.3/T1.6 their cleanest inputs for free.

### T2.5 — Polyphase-filterbank symbol timing 🟡 M
- **Source:** liquid-dsp `firpfb.proto.c` + `symsync.proto.c:127` (MIT); gnuradio
  `pfb_clock_sync`, `symbol_sync_cc_impl.cc`, `clock_recovery_mm_cc_impl.cc` (GPL).
- **What:** Split the RRC filter into N≈256 sub-filters (1/N-sample timing
  resolution); TED from the *exact* matched-filter derivative (`dh=h[i+1]−h[i−1]`),
  not Gardner's biased rectangular approximation. Works at 1 sps post-decimation.
- **Why us:** Timing jitter is the dominant residual error for our RRC modes
  (QPSK2000-RRC, 8PSK2000-RRC, SCFDMA). ~117 KB static at N=256. Larger refactor of
  `openpulse-dsp` timing recovery — schedule after T2.1.

### T2.6 — CMA blind equalizer 🟡 S–M
- **Source:** gnuradio `adaptive_algorithm_cma.h`; liquid-dsp `eqlms.proto.c:454` (`step_blind`).
- **What:** `e = y(|y|²−R)`; adapts with no training sequence for constant-modulus
  (PSK/FSK). Pre-converge before carrier lock; fall back to DD-LMS for QAM.
- **Why us:** Equalise fast-fading HF where a one-shot preamble-trained EQ goes stale
  mid-frame. ~10-line algorithm; the wiring (algorithm selection per mode) is the work.

### T2.7 — RLS equalizer 🟡 M
- **Source:** liquid-dsp `eqrls.proto.c:259` (MIT).
- **What:** Recursive least-squares; converges in ~(filter-order) symbols vs LMS's
  many. O(p²)/symbol (≈1024 mults at p=32 — acceptable).
- **Why us:** Our preambles are short (~16 symbols); RLS converges within them where
  LMS may not. Offer alongside LMS/NLMS, not as a replacement.

### T2.8 — OFDM/SCFDMA channel-estimate denoising 🟡 S–M
- **Source:** gnuradio `ofdm_chanest_vcvc_impl.cc:208` (idea described as a TODO).
- **What:** IFFT the per-subcarrier channel estimate → zero taps beyond the CP →
  re-FFT. Removes out-of-CP noise from the estimate.
- **Why us:** Our SCFDMA DFT-CE already does the LS→IDFT→zero-beyond-CP→DFT chain
  ([channel.rs:137](../../plugins/scfdma/src/channel.rs#L137)); this confirms the
  approach and suggests adding inter-frame EMA smoothing (gnuradio
  `ofdm_equalizer_simpledfe.cc`) for slow-fading paths. ~1–2 dB on low-SNR 64QAM
  subcarriers.

---

## Tier 3 — Strategic / larger (new modes or families)

### T3.1 — 32APSK mode (DVB-S2 geometry) 🟢 M–L
- **Source:** qo100 `gr-qo100_modem/python/constants.py:27`, `32apsk/APSK sync.ipynb`
  (GPL); liquid-dsp `modem_apsk.proto.c` (MIT, APSK4–256 generic).
- **What:** 3-ring 4+12+16 constellation, DVB-S2 radius ratios (γ₁=2.53, γ₂=4.30) and
  bit labeling, 5 bits/symbol. Demonstrated OTA in 2.7 kHz at ~5% uncoded BER.
- **Why us:** Fills the 8PSK(3 bps)→64QAM(6 bps) gap with a *circular* geometry far
  more tolerant of PA AM-PM and HF fading than rectangular 64QAM. Bundle with T2.4
  (pilots) + T2.2 (generic LLR) + T3.2 (high-rate LDPC). Our stated APSK interest.

### T3.2 — Practical high-rate LDPC (MacKay-Neal girth-6) 🟢 M
- **Source:** qo100 `ldpc/alists/3_27_girth6_mackay-neal.alist` (N=7595, K=6751,
  rate ≈8/9; also N=16200) (GPL; matrices includable).
- **What:** (3,27)-regular girth-6 LDPC, ~949 bytes info/block at rate 8/9, within
  0.1–0.2 dB of DVB-S2 at practical block sizes, BP-SPA decodable.
- **Why us:** Our LDPC is rate-1/2 — half the bandwidth wasted at 5–6 bps/symbol.
  Rate-8/9 with 32APSK ≈ 4.44 net bps vs 64QAM-rate-1/2's 3.0. alist is standard and
  loadable by our LDPC infra (T0.3 + a min-sum→normalised-min-sum or SPA upgrade
  buys ~0.2–0.5 dB). Use girth-6 (girth-4 has an error floor).

### T3.3 — K=7 convolutional Viterbi 🟡 M
- **Source:** gnuradio `cc_decoder_impl.cc:142` (GPL; VOLK SIMD kernel + scalar logic).
- **What:** Rate-1/2 K=7 (64-state) Viterbi, ~3 dB over our K=3, 8-bit soft input.
- **Why us:** A cheap coding-gain bump for the convolutional path; needs a pure-Rust
  scalar fallback (VOLK is C/SIMD). Optional vs investing in LDPC (T3.2).

### T3.4 — Differential-encoding option (8PSK) 🟡 S
- **Source:** qo100 `differential_8psk/Analysis.ipynb` (GPL).
- **What:** Encode phase *differences*; demod multiplies by conj(prev). Blind to
  constant phase offset and turns a cycle slip into a 1-symbol error. Measured cost:
  ~2.1 dB MER vs coherent.
- **Why us:** An optional flag for low-Doppler HF where occasional cycle slips
  (burst errors) dominate. The 2.1 dB penalty means *option*, not default.

### T3.5 — FSK_LDPC very-robust narrowband mode 🟡 L
- **Source:** mercury/FreeDV `freedv_api.h:58`, used in `datalink_broadcast` (GPL).
- **What:** Configurable 2/4-FSK + LDPC; FreeDV's most-robust data mode, works below
  −10 dB SNR in a 200–500 Hz footprint.
- **Why us:** Extends our SL-floor below what PSK reaches. New plugin; our FSK4-ACK is
  a partial foundation. Pairs with mercury's ARQ as the bottom rung of the ladder.

### T3.6 — Longer m-sequence preamble 🟢 S (but protocol-breaking)
- **Source:** liquid-dsp `msequence.c`, `framesync64.c:92` (MIT).
- **What:** 64-symbol m-sequence (m=7) preamble: ~18 dB lower autocorrelation
  sidelobes, ±baud/2 coarse-CFO range.
- **Why us:** **Prerequisite** for T2.1 to acquire reliably (16 symbols is too short
  for confident coarse CFO). Changes the wire preamble → must ship as a **new
  waveform/profile**, not a drop-in. Cheap to build; the cost is interop versioning.

### T3.7 — Polar SCL / TPC decoders ⚪ L
- **Source:** gnuradio `polar_decoder_sc_list.cc`, `tpc_decoder.cc` (GPL).
- **What:** Near-ML short-block FEC (polar) / PACTOR-III-style product codes (TPC).
- **Why us:** Marginal — we already have RS/conv/LDPC. Polar's power-of-2 blocks and
  TPC's PACTOR-III niche don't fit our ecosystem without new framing. Catalogued.

---

## Flagship track — SC-FDMA done right for low PAPR (the distinguishing feature)

**Directed item.** The user wants SC-FDMA-low-PAPR reconsidered as a distinguishing
feature (revisiting the dropped FF-14). A source audit of `plugins/scfdma`
**corrects the premise** that led to dropping it:

- ✅ The DFT-spread/transform-precoding is correctly implemented
  ([modulate.rs:85](../../plugins/scfdma/src/modulate.rs#L85)).
- ✅ Subcarrier mapping is **localized/contiguous**
  ([modulate.rs:89](../../plugins/scfdma/src/modulate.rs#L89)) — *not* the
  interleaved/distributed scheme prior notes assumed. The single-carrier structure
  is largely intact.
- So the low-PAPR prerequisites are already present. Three things erode the gain:

| Lever | Current state | Fix | Payoff |
|---|---|---|---|
| **F.0 Measurement basis** | `measure_papr` uses peak/mean of **real passband samples** ([modulate.rs:138](../../plugins/scfdma/src/modulate.rs#L138)) — includes the ~3 dB bandpass/carrier term | Measure the **complex-envelope CCDF** (analytic ‖Hilbert(s)‖) — the quantity that sets PA back-off | **~3 dB on paper, zero waveform change**; apples-to-apples vs OFDM and textbook numbers |
| **F.1 Comb pilots** | Frequency-domain comb pilots inserted *per symbol*, bypassing DFT precoding ([modulate.rs:93](../../plugins/scfdma/src/modulate.rs#L93)) — ~20% of SCs are unprecoded OFDM tones that genuinely raise envelope PAPR | Move to **time-domain pilot blocks** (1–2 known SC-FDMA symbols per frame); all data SCs precoded | Removes the real ~1–2 dB comb inflation; trades per-symbol CE for per-frame CE (fine for HF coherence time) |
| **F.2 No low-order constellation** | Min data constellation is QPSK; no π/2-BPSK ([params.rs](../../plugins/scfdma/src/params.rs)) | Add **π/2-BPSK** (and maybe π/4-QPSK) data mode | Brings envelope PAPR near the SC-FDMA floor; the QRP/portable target |

**Recorded FF-14 figure** (12.7 → 8.9 dB, −3.8 dB) was measured on the real-sample
basis, so ~3 dB of "headroom" is a measurement artifact, not a physics limit. The
PA-relevant envelope PAPR is already ~3 dB below the reported number.

**Proposed sequence:**
1. **F.0 first** — implement envelope-CCDF measurement and a shared SC-FDMA-vs-OFDM
   CCDF benchmark (same constellation + bandwidth). Cheap, de-risks the whole track,
   and likely shows we are *already* meaningfully better than 8.9 dB. **Decision
   gate:** only continue if the envelope CCDF gap vs OFDM is worth the work.
2. **F.2** — add π/2-BPSK (additive, no existing mode changes).
3. **F.1** — time-domain pilots (larger; touches CE/equalization).
4. **F.3** — publish the measured CCDF dB advantage; gate any marketing claim on it.

**Open question (out of scope unless we go SDR-direct):** a *complex-baseband/IQ*
chain. Over a real **SSB radio** the audio is real by construction and the SSB
filter discards the negative sideband, so the PA already sees the single-sideband
envelope — the Hermitian-symmetry "penalty" is the F.0 *measurement* artifact, not a
transmit-chain defect. A complex-baseband rewrite (cross-crate, touches
`openpulse-audio`/`openpulse-modem`) is only warranted if we drive an IQ/SDR front
end directly. **Recommend deferring it** and capturing the PAPR win via F.0–F.2.

Why it's a credible differentiator: no other open ham HF modem advertises a true
low-PAPR wideband mode; less PA back-off = more average power from any linear amp —
a concrete QRP/portable advantage, *if* F.0 confirms the envelope CCDF gap.

---

## Adaptive-ARQ cluster (Mercury/HERMES) — batch as one "ARQ robustness" workstream

Mercury is the closest analog to our *system* (HPX ARQ + B2F). Its ARQ refinements
are individually small and map onto our `RateAdapter`/`HpxSession`/`AckFrame`. Most
valuable first:

| # | Idea | Source | Benefit | Effort |
|---|---|---|---|---|
| A1 | **Per-direction asymmetric mode** (my-TX vs peer-TX tracked independently) — *directed interest* | `arq_fsm.h:181`, `arq_fsm.c:1515` | 🟢 | M |
| A2 | **Backlog-aware upgrade gating** (don't negotiate up unless queued bytes justify the airtime) | `arq_fsm.c:336`, `arq_protocol.h:261` | 🟢 | S |
| A3 | **Retry-forced downgrade + re-upgrade hold timer** (anti-oscillation) | `arq_fsm.c:299,436` | 🟢 | S |
| A4 | **SNR upgrade hysteresis** (stay vs upgrade thresholds differ by ~1 dB) | `arq_fsm.c:364` | 🟡 | S |
| A5 | **Persist-after-retry / no-progress wall-clock budget** (don't give up too early on deep fades) | `arq_fsm.c:1385` | 🟡 | S |
| A6 | **Deferred DISCONNECT with drain timeout** (don't key forever; don't drop last bytes) | `arq_fsm.c:1100` | 🟡 | S |
| A7 | **HAS_DATA piggyback in ACK** (skip a TURN_REQ round-trip — ~7–15 s saved) | `arq_fsm.c:538,1692` | 🟡 | S |
| A8 | **Implicit ACK from peer DATA while in WAIT_ACK** | `arq_fsm.c:1454` | 🟡 | S |
| A9 | **Startup guard window** (lock robust mode for first ~10 s) | `arq_fsm.c:888` | 🟡 | S |
| A10 | **IRS-originated keepalive + miss-limit disconnect** | `arq_fsm.c:1804` | 🟡 | S |
| A11 | **SNR byte in every frame type** (not just ACK) | `arq_protocol.h:34` | ⚪ | S |
| A12 | **ACK-delay field → clean OTA-RTT** | `arq_protocol.h:36`, `arq_timing.c:67` | ⚪ | S |

A2 (backlog-aware) + A1 (per-direction) are the directed interests and the biggest
structural gaps vs our single-`current_adaptive_mode` model.

---

## Channel-simulator cluster (gnuradio gr-channels) — test infrastructure

| # | Idea | Source | Benefit | Effort |
|---|---|---|---|---|
| C1 | **Sample-Rate-Offset (SRO) model** | `sro_model_impl.cc:63` | 🟢 | S | 
| C2 | **CFO drift (random-walk) model** | `cfo_model_impl.cc:47` | 🟡 | S |
| C3 | **Clarke-Jakes flat fader (+ Rician)** | `flat_fader_impl.cc` | 🟡 | S–M |
| C4 | **Selective fading w/ sinc fractional-delay taps** | `selective_fading_model_impl.cc:73` | 🟡 | M |

**C1 is high-value:** our persistent dual-clock hardware failure (SCFDMA52/64QAM
fail on two-soundcard rigs but pass single-clock virtual loop — see memory
`loopback-mode-matrix`) is *exactly* an SRO. Adding an SRO channel to
`openpulse-channel` lets us reproduce and gate that failure class **in CI without
hardware** — the single most useful test-infra item in the whole scan.

---

## Marginal / long-shot (catalogued for completeness)

- liquid-dsp: NCO 32-bit phase accumulator + LUT; rational P/Q resampler (48↔8 kHz
  hardware support); layered packetizer (concatenated FEC); pre-decimator for
  high-oversampling modes; Schifra RS(255,k) param validation (confirms our t=16).
- gnuradio: Mueller-Müller TED (1-sps alternative to Gardner); Schmidl-Cox integer
  carrier-offset search (we have SC timing; this is the >subcarrier-spacing piece);
  LDPC bit-flip decoder (too weak for our short frames); LFSR header scrambler.
- qo100: 5%-rolloff RRC + non-integer sps (~3.11) for tighter baud packing (needs
  polyphase timing, protocol-breaking); separate pilot diagnostic stream;
  resync-on-demand API; PRBS pilot constants.
- mercury: compact arithmetic-coded CALL/ACCEPT (conflicts with our signed ConReq);
  compact CQ beacon; two-level FSM refactor; shared-memory audio transport (only if
  we split processes); TX peak-dBFS telemetry.

---

## FreeDV DATAC parameter reference (from mercury `modem/freedv/ofdm_mode.c`)

A proven HF-OFDM comparison point for our OFDM52 ladder. All modes: ts=16 ms
(62.5 baud), fs=8 kHz, CP=6 ms (QAM16C2: 4 ms), ns=5, LDPC FEC.

| Mode | Data carriers | LDPC | Payload (B) | ~BW (Hz) | Mercury SNR floor |
|---|---|---|---|---|---|
| DATAC16 | 3 | rate ~1/5 short | 14 | ~250 | best (control) |
| DATAC4 | 4 | rate 1/2 | 54 | ~350 | −6 dB |
| DATAC3 | 9 | rate 1/2 | 126 | ~600 | −1 dB |
| DATAC1 | 27 | rate 1/2 | 510 | ~1700 | +3 dB |
| DATAC17 | 33 | rate 0.6 QPSK | 1180 | ~2100 | +7 dB |
| QAM16C2 | 33 | rate 0.6 QAM16 | 1213 | ~2100 | +13 dB |

Note our OFDM52 uses 52 carriers with an 8PSK/16QAM/32QAM/64QAM ladder — denser than
FreeDV's QPSK/QAM16 DATAC modes. The SNR-floor column is a useful sanity check for
our own per-mode admission thresholds.

---

## Recommendation (for the maintainer to decide)

If I were sequencing this, in order:

1. **Tier 0 (T0.1–T0.3)** — do now; one is a possible silent-corruption bug, two are
   near-free dB/latency. Hours, not days.
2. **AGC (T1.1) + SNR estimator (T1.3)** — the highest-leverage small additions for
   real-HF robustness; everything downstream (rate adaptation, LLR scaling, T1.6)
   benefits.
3. **SRO channel model (C1)** — turn the dual-clock hardware failure into a CI gate.
4. **Burst synchronizer (T2.1) + m-sequence preamble (T3.6)** as one new-waveform
   effort — the convergent #1 finding and the durable fix for the carrier-offset
   gaps. Ship as a new profile to preserve interop.
5. **SC-FDMA flagship F.0** — cheap measurement fix; **decision gate** on whether the
   envelope-CCDF advantage justifies F.1/F.2. This answers the FF-14 question with
   data instead of an estimate.
6. **ARQ robustness batch (A1–A3)** — per-direction + backlog-aware + anti-oscillation
   are the directed system-level gaps.
7. **Strategic** (32APSK + pilots + high-rate LDPC, T3.1/T2.4/T3.2) as a bundled
   high-throughput-in-2.7-kHz track, if throughput (not just robustness) is the goal.

Open questions for you: (a) is the priority **robustness** (AGC/sync/ARQ) or
**throughput** (APSK/LDPC/SC-FDMA)? (b) do we commit to a **new pilot-framed
waveform** (unlocks T2.4/T3.1 and clean tracking) or stay preamble-only? (c) is true
low-PAPR SC-FDMA a **marketing differentiator** worth F.1/F.2 *after* F.0 quantifies
it? Each branch point changes what's worth building.
