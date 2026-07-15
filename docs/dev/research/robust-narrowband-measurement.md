# Robust narrowband weak-signal rung — kill-first measurement (REQ-WSIG-01)

**Status:** measured 2026-07-14 (ideal + real-sync, kill-first gate **PASSED**). Robust-ACK resolved
2026-07-15 — the deferral's "0.60 binding constraint" was a 40-trial artifact; K=3 per-copy-LLR union
decode clears ≥0.99 at 3 dB below the floor (see the final section). ARQ rung **unblocked**.

## The question

REQ-WSIG-01 proposes a robust narrowband weak-signal waveform as a **sub-floor rung below BPSK31** (the
current SL floor). After the frequency-diversity rung was measured-and-rejected (#864 — its gain didn't
survive its own PAPR), the design review picked a fundamentally different candidate: a **constant-envelope
non-coherent 16-GFSK**. The claim: a non-coherent, constant-envelope waveform collects the large
implementation+fading tax the *coherent* BPSK31 chain pays (carrier tracking through fades, Doppler)
**without** paying it back in PAPR (ΔPAPR ≈ 0, a credit — the opposite sign from #864).

Per the repo's "measure the floor first" rule, we measured the **ρ=0-analogue ideal bound** before building
any production waveform.

## Candidate

16-GFSK, **31.25 baud** (= BPSK31's symbol rate), **31.25 Hz** tone spacing, 256 samples/symbol at 8 kHz,
16 tones → **500 Hz occupied**, 4 bits/symbol → **125 bps raw** (4× BPSK31). Reuses the JS8 tone synth
(`modulate_tones`) and Goertzel energy detector (`goertzel_energy`), and the engine's audio-free union
decode (`combine_and_decode_llrs`) — so both arms run the **same RS(255,223) + Frame/CRC** decode (matched
FEC by construction).

## Method (`crates/openpulse-modem/tests/mfsk_subfloor_bound.rs`)

Both arms transmit the identical FEC-framed 73 B payload, pass it through the same Watterson channel
(matched average power, matched N0 over the band), and decode through the same engine seam. **Ideal
bound:** both arms are symbol-aligned and frequency-exact (genie sync), so this is valid for the *kill*
decision, not the *ship* decision — a real receiver adds acquisition (~2–3 dB erosion per #864). A
non-ignored clean-channel round-trip guard pins the 16-tone LLR convention.

Pre-registered ship bar (roadmap): the **ideal must clear ≥5 dB** at the moderate_f1 0.5-crossing (3 dB
ship bar + ~2 dB ideal→real erosion), with no good_f1 regression and ΔPAPR ≤ 0.5 dB; else honest no-ship.

## Results (40 trials, matched average TX power)

**Watterson coded frame-success (disentangled from the concurrent run):**

`good_f1` (0.1 Hz / 0.5 ms — slow fade):

| snr_db | BPSK31 | 16-GFSK |
|--------|--------|---------|
| −12    | 0.00   | 0.00 |
| −9     | 1.00   | 1.00 |
| −6     | 1.00   | 1.00 |
| −3     | 1.00   | 1.00 |
| 0      | 1.00   | 1.00 |

`moderate_f1` (1 Hz / 1 ms):

| snr_db | BPSK31 | 16-GFSK |
|--------|--------|---------|
| −9     | 0.00   | 0.00 |
| −6     | 0.00   | 0.00 |
| −3     | 0.03   | 0.20 |
| 0      | 0.10   | 0.85 |
| 3      | 0.40   | 0.98 |

`poor_f1` (2 Hz / 2 ms — fast fade):

| snr_db | BPSK31 | 16-GFSK |
|--------|--------|---------|
| −6     | 0.00   | 0.00 |
| −3     | 0.00   | 0.12 |
| 0      | 0.00   | 0.77 |
| 3      | 0.00   | 0.98 |
| 6      | 0.00   | 1.00 |

**AWGN known-answer sanity (label SNR):**

| snr_db | BPSK31 | 16-GFSK |
|--------|--------|---------|
| −9     | 0.05   | 0.08 |
| −6     | 0.20   | 0.22 |
| −3     | 0.65   | 0.50 |
| 0      | 0.98   | 0.68 |

**ΔPAPR = −1.45 dB** (16-GFSK 0.00 dB constant-envelope vs BPSK 1.46 dB).

## Reading — the gate is passed

- **moderate_f1: ~5 dB ideal gain.** BPSK31 is still only 0.40 at +3 dB (crossing ~+3.5–4 dB); 16-GFSK
  crosses ~−1 dB. Clears the ≥5 dB early-kill gate.
- **poor_f1: BPSK31 fails entirely** (0.00 through +6 dB) while 16-GFSK crosses ~0 dB — an unbounded gain.
  This is the mechanism in the clear: 2 Hz Doppler breaks coherent carrier tracking, but a non-coherent
  Goertzel energy detector is immune, and the 500 Hz span gives per-symbol frequency diversity the FEC
  harvests.
- **good_f1: no regression** — both saturate by −9 dB, essentially equal on slow fade.
- **ΔPAPR is a −1.45 dB credit,** not a cost: the RMS-keyed channel understates the constant-envelope
  candidate by ~1.4 dB at matched PEP (the #864 error with the sign flipped, in our favour).
- **AWGN sanity holds:** 16-GFSK crosses ~1 dB *worse* than BPSK31 on the label axis — a fading-only lever
  must not win on AWGN, and it doesn't (no noise-bandwidth/accounting bug).

## Bottom line — physics validated; a production rung is justified, with two conditions

The ideal bound **passes decisively** (opposite of #864): a constant-envelope non-coherent narrowband
waveform beats coherent BPSK31 on fading — by ~5 dB on moderate multipath and *completely* on fast fade —
at a PAPR credit, and behaves correctly on AWGN. The detection class is already proven in this repo
(JS8 decodes at −20 dB label). So this is a genuine sub-floor rung, not a marginal one.

**But two conditions govern whether it ships as an ARQ rung** (a positive waveform number alone doesn't
settle it):

1. **The ACK channel.** ARQ continuity at −3…−8 dB needs the *return* link to live there too. The current
   FSK4-ACK (100 baud, hard-decision, Hann-windowed) dies far above the candidate's floor. Shipping this
   as a data rung requires an MFSK-class ACK (trivially the same waveform at a short frame) — otherwise it
   buys **broadcast / one-way** robustness only.
2. **Real-sync erosion + session timers.** This is the *ideal* bound; a production plugin must add
   acquisition (budget ~2–3 dB — moderate_f1 stays positive, poor_f1 is unbounded so safe), and the 16 s
   frame duration stresses HPX timeouts.

**Recommended next stage (a genuine multi-PR build, not a quick add):** (a) a real-sync measurement (add a
Costas/base-frequency search to the MFSK arm) to get the net gain; (b) a production `mfsk16`
`ModulationPlugin` + engine registration; (c) ladder placement at the vacant SL1 sub-floor with measured
floor/ceiling; (d) an MFSK-class ACK; (e) session-timer handling for the longer frame. The reproducible
measurement (`mfsk_subfloor_bound.rs`) and the reused JS8 primitives remain.

## Real-sync measurement — the erosion, measured directly (2026-07-14)

Stage (a) is done: the harness now adds acquisition to the 16-GFSK arm — three 7-symbol **Costas sync
blocks** (FT8 array ×2 = `[8,4,10,12,2,6,0]`, positions [0,262,524]), a **normalized per-symbol
tone-fraction correlation** (the JS8 `sync_score` pattern, immune to the high-energy-noise-window trap), a
coarse→fine **timing × frequency search**, and an **injected ±25 Hz tuning offset** the searcher must find
(BPSK31 keeps genie frequency — its ±7.8 Hz AFC can't absorb ±25 Hz without an engine AFC chain the bare
demod lacks, so the bias runs *against* the candidate). A **genie column is printed alongside the real
column**, so the erosion is measured directly (`mfsk_real_sync_sweep`, 24 trials). Guarded by a non-ignored
`real_sync_acquires_a_tuning_offset_and_lead` (finds +18 Hz + a 300-sample lead and decodes).

**Watterson (BPSK31 | 16-GFSK genie | 16-GFSK real):**

| channel | BPSK31 crossing | genie crossing | **real crossing** | **real net gain** |
|---|---|---|---|---|
| good_f1 | ~−4 dB | ~−4 dB | ~−4 dB | ~0 (no regression) |
| moderate_f1 | ~+3.7 dB | ~−1.5 dB | ~−0.5 dB | **~4.2 dB** |
| poor_f1 | never (0.00 → +6 dB) | ~−0.3 dB | ~−0.3 dB | **unbounded** |

Representative rows — moderate_f1: BPSK31 0.03/0.10/0.40/0.80 vs real 16-GFSK 0.04/0.79/0.96/1.00 at
−3/0/+3/+6 dB. poor_f1: BPSK31 0.00 everywhere vs real 16-GFSK 0.00/0.67/1.00/1.00 at −3/0/+3/+6 dB.

- **Acquisition erosion (genie→real): ~1 dB on moderate_f1, ~0.3 dB on poor_f1** — well inside the
  0.5–1.5 dB budget and far below #864's 2–3 dB. Non-coherent acquisition is genuinely easy: there is no
  carrier phase to track, and "AFC" degenerates to a static ~2 Hz tone-grid alignment on a 31.25 Hz grid.
- **Net moderate_f1 gain ~4.2 dB clears the ≥3 dB ship bar;** poor_f1 stays unbounded.
- **AWGN real-vs-genie sanity holds** at −6 dB and up (real ≈ genie); a sync floor appears only at −9 dB
  (real 0.00 vs genie 1.00), which is *below* the operational fading crossing (~0 dB) and is a tunable
  score threshold, not the flat-across-SNR bug signature.

**Verdict: the waveform AND its acquisition are validated end-to-end.** The technical (DSP) risk of the
sub-floor rung is retired. The remaining ship questions are **operational, not waveform**: (1) an
**MFSK-class ACK channel** — the crux, since FSK4-ACK dies far above the candidate's floor, so as an ARQ
rung it needs its return link there too (else broadcast-only); (2) HPX session-timer handling for the 16 s
frame. Production stages b–e (the `mfsk16` plugin + engine registration + SL1 ladder placement + the ACK +
timers) remain a scheduled multi-PR build.

## ACK-channel feasibility — measured, the crux confirmed (2026-07-14, PR-C measure-first)

Before wiring the ARQ ACK seam (the most regression-sensitive machinery in the repo), we measured whether
a short `MFSK16-ACK` return frame survives at the data rung's floor. Path: a 5-byte `AckFrame` →
`ShortFecCodec::new()` (t=4 → 13 bytes) → `MFSK16-ACK` (40 symbols ≈ 1.28 s) → Watterson (with an injected
±25 Hz tuning offset + a lead, so acquisition is exercised) → demodulate → ShortFec decode
(`plugins/mfsk16/tests/ack_feasibility.rs`).

**Result — the ACK is the binding constraint.** `MFSK16-ACK` on moderate_f1: 0.00 / 0.22 / **0.60** / 0.88
at −6 / −3 / **0** / +3 dB. The data rung crosses ~−0.5 dB, so at the operating point (~0 dB) the ACK
decodes only ~0.6 while the data decodes ~0.85 — the short **1.28 s ACK is ~3–4 dB more fade-sensitive**
than the 17 s data frame, because it cannot fade-average over enough coherence times. It functions fine a
few dB up (≥0.9 at +6 dB).

**Naive tone repetition does not fix it.** A 3× spaced-and-energy-summed ACK (99 symbols, 3.2 s) still
measured ~0.62 — energy-summing a copy that is in a deep fade *adds that copy's noise* across all tones and
dilutes the good copies (the #694 soft-combine lesson: summing a ruined look loses). A robust ACK needs
proper per-copy **LLR** diversity (decode each copy's soft metric, MAP-combine) plus per-copy acquisition —
real additional DSP.

**Conclusion:** this confirms the production review's verdict decisively — **ship broadcast-first, defer
the ARQ rung.** The measure-first gate saved us from wiring a marginal ACK into the shared ACK seam. The
`MFSK16-ACK` mode exists in the plugin (usable, functional above the floor) but is **not** wired as the ARQ
return channel; the ARQ rung is deferred pending a robust-ACK design (per-copy LLR diversity). The shipped
state — `MFSK16` as a robust broadcast/beacon + explicit-mode waveform — stands.

## Robust-ACK, resolved — two findings overturn the deferral (2026-07-15, PR-C)

Picking the ACK back up to build the "robust ACK" the section above deferred, the measurement produced two
findings that reverse its premise. Harness: `plugins/mfsk16/src/robust_ack.rs` (child module, real
`acquire`/`frame_noise`/`bit_llrs`), 400 trials, one continuous Watterson realization per trial (honest fade
correlation), genie-sync + wrong-lock instrumentation.

**Finding 1 — the "0.60 binding constraint" was a 40-trial sampling artifact.** At 400 trials the single
40-sym ACK decodes **0.90 (moderate_f1) / 0.92 (poor_f1) at 0 dB** — right at the bar, not 0.6 — with the
hard-argmax path and the per-bit soft-LLR path *identical* (so the decoder is not the variable). The
mechanism reframes too: at poor_f1 (2 Hz Doppler) the 1.28 s ACK spans ~5 coherence times, so it does **not**
sit inside one fade — the limiter is a fade **burst** exceeding ShortFec's t=4 byte budget, not a lack of
fade-averaging. The ACK is near-adequate at the nominal floor, with a steep cliff below it (−3 dB → 0.56–0.66).

**Finding 2 — the intuitive fix (longer contiguous, stronger code) LOSES; per-copy-LLR diversity WINS.**
Two candidates at matched airtime (400 trials, ≥0.9 bar at 0 dB on both channels):

| | single (1×) | Arm B longer+t=24 (3.5×) | **Arm C K=3 union, no hop (3.8×)** |
|---|---|---|---|
| moderate_f1 −3 dB | 0.66 | 0.49 | **0.99** |
| moderate_f1 0 dB | 0.90 | 0.93 | **1.00** |
| poor_f1 −3 dB | 0.56 | 0.26 | **1.00** |
| poor_f1 0 dB | 0.92 | 0.91 | **1.00** |

* **Arm B** (one longer frame, stronger single ShortFec block, one acquisition — the predicted winner) is
  *worse than the short baseline* at −3 dB. A longer frame at fixed baud accumulates more fade-burst
  exposure than the extra ECC covers, and interleaving is inert within a single RS block (position-agnostic).
  The "17 s data frame decodes 0.85, so longer-contiguous must win" intuition is falsified by measurement.
* **Arm C** — K=3 time-spaced copies of the existing 40-sym `MFSK16-ACK`, each demodulated to calibrated
  soft LLRs, **union-decoded** (each copy standalone first, MAP-sum only as fallback — #694, *not*
  sum-then-decode) — clears the bar with ~1.0 and holds **0.99–1.00 at 3 dB below the floor**, a large fade
  margin. `genie ≈ real` and **wrong-locks = 0** everywhere: acquisition is not the bottleneck, the union
  combining is doing the work honestly. **No frequency hop is needed** (`hop=0 ≡ hop=500 Hz`) — the 0.5 s
  time gaps already decorrelate the fades, so the ACK stays 500 Hz (no bandwidth cost, and Fable's two-tap
  hop-overfit concern is sidestepped by not hopping). K=2 is marginal (0.88 at −3 dB); K=3 is the knee.

Why energy-summing failed but LLR-union works: energy-summing combines *before* the per-copy noise
normalization, so a faded copy's noise pollutes every tone; the union decodes each copy standalone (a clean
copy is never diluted) and only MAP-sums as a fallback, so success is a strict superset of any single copy.

**Shipped:** the reusable, validated primitive `openpulse_core::ack::decode_ack_from_llr_copies` (the #694
union over per-copy ACK LLRs → `AckFrame`, CRC-gated against wrong-lock mis-corrections) + the measurement
(fast gates `single_ack_is_near_the_floor_bar`, `k3_union_holds_below_the_floor`; ignored research
`robust_ack_sweep`, `baseline_reconciliation`). The robust-ACK design is now **validated and cheap** — 3
copies + union decode, no new waveform, no hop — so the ARQ rung is **unblocked**. The remaining ARQ
integration (transmit K copies with airtime-bounded gaps on the ACK TX path, receive-side buffering + the
union primitive, SL1 ladder placement, airtime-scaled timers) is engine/daemon wiring into the
regression-sensitive ARQ seam — scoped, not speculative — and is the greenlight-gated next step.
