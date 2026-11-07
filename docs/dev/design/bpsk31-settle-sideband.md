---
project: openpulsehf
doc: docs/dev/design/bpsk31-settle-sideband.md
status: draft
last_updated: 2026-11-07
---

# The BPSK carrier estimate lands one sideband off: replace the picket-fence coarse scan

Work plan M3, A2: the slow SL2 start in run f2j (task 30, review "Open, parked"). This note is
measurements plus a design. It is a DSP-path change on every BPSK receive, so it went to Fable as a
design before implementing (the constant question applies; see *Constants*).

**Reviewed 2026-11-07: APPROVE WITH CHANGES** —
[`docs/dev/reviews/review-bpsk31-settle-sideband-2026-11-07.md`](../reviews/review-bpsk31-settle-sideband-2026-11-07.md).
The eight required changes are applied below: the convergence check's behaviour change is stated and
gated (*Behaviour change*), the FFT form is specified by probes with the Nyquist guard on the unfolded
frequency (*The dense stage, specified*), the sideband margin is 11.8 dB, not 6 dB (*Mechanism*,
*Risks*), the mechanism text is corrected, burst 4 is luck rather than a warm-up, gate 5 says which
suites run where, gates 1–2 pin the daemon's construction in code on short cuts, and the tone and
two-signal controls are gates run with the notch on.

## Problem — measured

**On air (f2j, 2026-11-06, B→A, 2 m).** A's AFC was cold (0 Hz) and B was about −9 Hz off. A did
not decode B's first three SL2 frames (`BPSK31|rs`, 64 B, 66.6 s airtime) before the ACK deadline.
So A keyed nothing, B timed out three times, and B abandoned one message. The fourth frame decoded,
and the run went on normally.

**Burst 4 was luck, not a warm-up** (review, measured). At 12 of 13 window starts across f2j burst
4's first 0.8 s, the shipped settle reads −25.37 Hz, a sideband; only the start at +5632 samples reads
−9.74. The trigger settle happened to run at that one start. Nothing about the receiver had changed
between bursts 3 and 4: the AFC is rolled back on every failed attempt, so A met burst 4 as cold as
bursts 1–3.

**Replayed (x86, Ryzen 9 3900X, release).** The replay runs A's engine as `server::run` builds it
from `config-a.toml`: nine plugins in the same order, notch on, CE-SSB on, AGC off, `fast`. It
reads the recording in 400-sample chunks through `accumulate_capture`, then calls
`nack_budget::decode_ota_burst` with the production ACK deadline. The deadline is
`ota_ack_timeout_ms` (9000) − 2000 − 300 − FSK4 ACK airtime, which is 6.18 s on any host. The
trigger settle was read through a temporary instruments accessor that calls `afc_mini_settle` and
`acquire_at_onset` on the burst and lead the coded arm uses.

| f2j burst | trigger settle | accepted | dense coarse (proposed) | x86 decode | Pi (on air) |
|---|---|---|---|---|---|
| 1 | +7.21 Hz | yes | −8.45 | past deadline, 7.72 s | past window, 8.9 s |
| 2 | −24.86 | yes | −9.20 | past deadline, 7.69 s | past window, 9.1 s |
| 3 | −25.31 | yes | −9.65 | past deadline, 7.68 s | past window, 8.9 s |
| 4 | −9.75 (right) | yes | −9.75 | decoded in 1.70 s | decoded, 2.97 s (`decode_latency_ms`) |

- Bursts 1–3 are each off by ±15.6 Hz = Rs/2, one sideband.
- Without the deadline, burst 1 still decodes, but only after 25.5 s (phase 2).
- With the AFC warmed to the dense or wide value before the decode, bursts 1 and 2 decode in
  1.61 s and 1.58 s.
- The Pi/x86 ratio on the path that decoded is 2.97/1.70 = 1.74 for burst 4, and about 1.39 for
  burst 5 (back-solved from the EWMA). The GPU was on, on rpi51.

**Mechanism.** The settle mechanism was read from the code, and the sweep confirms its offset
pattern. `bpsk::demodulate::afc_estimate_hz` has two stages.
1. The coarse stage, `estimate_carrier_hz_wide`, is a Goertzel scan of the squared signal over
   ±400 Hz in **12.5 Hz** steps (25 Hz in the squared domain).
   - For BPSK31 the scan window is the whole 8448-sample settle window. Its main lobe is about
     0.95 Hz wide (squared domain), so the grid is roughly 26× coarser than the scan's own
     resolution.
   - The carrier line is therefore heard only through sidelobe leakage into whichever probe sits
     nearest. The scan picks whichever line (or noise peak) lands closest to a probe.
2. The squared `--++` preamble has **structural** lines: the main line at 2f, and lines at
   2f ± Rs/2 and 2f ± Rs (baseband ±Rs/4 and ±Rs/2).
   - Their levels follow from the `--++` square wave's Fourier coefficients 3/4, 2/(3π) and 1/8:
     the baseband ±Rs/4 lines are **−11.8 dB** and the ±Rs/2 lines **−15.6 dB** below the main
     line. On the real windows they measure −10.7…−11.2 dB and −14.5…−16.0 dB (review). An earlier
     draft of this note said 6 dB; that was wrong.
   - Which line the picket hears is **alignment arithmetic**, not signal quality. The grid is 25 Hz
     in the squared domain. BPSK250 (Rs = 10 steps) and BPSK100 (Rs = 4 steps) put every line on
     the grid with the carrier, so they are immune. BPSK63 (Rs = 2.5 steps) is anti-aligned. BPSK31's
     lines sit 0.625 and 1.25 steps off the carrier, so at some carrier positions a sideband sits
     nearer a probe than the carrier does and out-scores it through the sidelobes.
   - When the coarse stage lands on a line, say c = f + Rs/4, the fine IQ-squaring stage sees a
     true residual of −Rs/4, exactly at its own ±Rs/4 wrap. If it reads the residual wrapped (≈ +Rs/4)
     and accepts it, the answer is c + Rs/4 = f + Rs/2 (the "sideband" class). If the residual fails
     its `|residual| < baud/4` range test, the coarse pick f + Rs/4 is returned (the "Rs/4" class,
     f2h). **The Rs/4 and Rs/2 classes are the same coarse pick**; the fine stage's wrap decides which
     one is reported.
   - The fine stage's residual reads about 0 at the line, which is why nothing flags the error.
3. Where the carrier line lands relative to the grid sets the outcome. At some offsets the carrier
   always loses. In noise, the leaked line can lose to a noise peak, which gives the "other" class
   below.

## Part 1 — the cold settle sweep (option 3)

**Apparatus.**
- **Frames.** `transmit_with_fec_mode` on a TX engine started at SL2 of `fast`, with CE-SSB on,
  `free_rs_strengthening` and a 64 B body: the shipped modulator by reference, 66.6 s. The carrier
  is offset with `set_center_frequency(1500 + Δ)` on the TX engine.
- **Noise.**
  - (0) f2j's own pre-run hiss, rx-a-f2j 1–48 s (flat 300–2750 Hz, no tone).
  - (1) `ic9700-idle-wide-500hz-control.wav` from the corpus, scaled to the hiss's mean square
    (3.0e-5).
- **SNR.** 3 dB and 12 dB, total-band signal power against noise power. On air the bursts sit about
  5 dB above the floor in level, that is about 3 dB S/N, but A ran AGC FAST, which compresses that
  figure. The receiver's `estimate_snr_db` returned `None` on the settle windows, so the synthetic
  SNR is **not** calibrated against the on-air frames.
- **Leads.** 16 000 and 48 000 samples of noise before the frame. On each, sub-read phases of +0,
  +133 and +266 samples move the frame against the 400-sample read boundary.
- **Offsets.** −20 to +20 Hz in 1 Hz steps (settle); even steps for decode.
- **Classes.** Right: within 2 Hz, or both under the 2 Hz deadband. Sideband: within 2 Hz of a
  nonzero multiple of Rs/2 = 15.625 Hz. Other: anything else.

**Result: synthetic frames pick the sideband too, so this is not the FT-991A's filter or phase
noise.**
- Overall: 318 of 984 cells picked a sideband, and the settle accepted 241 of them; 445 were right;
  221 were "other", almost all rejected by the convergence check.
- The sideband rate is set by the offset, not by the SNR (SNR 3: 156 of 492; SNR 12: 162 of 492).
- It is about 100 % at −17, −9, −8, −4, −3, +3, +4, +8, +9 and +17 Hz, and 0 % at 0, ±5 and ±20 Hz.
- −9 Hz (f2j) is one of the worst offsets. +3 Hz is the other on-air instance, see *Prior art*.

**Decode within the production deadline, cold (x86, 168 cells, even offsets):** 96 of 168.

| settle class | SNR 3 decoded | SNR 12 decoded |
|---|---|---|
| right, accepted | 16/16 (≤ 2.62 s) | 43/43 (≤ 0.64 s) |
| right, rejected | 7/10 | 7/9 |
| sideband, accepted | 5/13 | 10/22 |
| sideband, rejected | 1/4 | 0/4 |
| other, rejected | 6/41 | 1/6 |

- The sideband cells that decode are the small offsets (±2, ±4 Hz). They decode through the
  uncorrected pass, inside BPSK31's native reach.
- **AFC warmed to the dense estimate (the proposed fix's answer): 168 of 168, ≤ 1.69 s.**
- That run is an oracle for the fix, not the fix itself, because it skips the failed attempt 0 at
  0 Hz. The "right, accepted" cold cells are the fixed path's real cost: ≤ 2.62 s on x86, so about
  4.6 s on a Pi at ×1.74, inside the 6.18 s budget.

**The sweep table** (settle: 24 cells per offset = 2 noises × 2 SNRs × 6 leads; dense and wide: 12
per offset; decode: 8 per even offset):

| offset Hz | sideband pick | of which accepted | right | other | dense right | wide-anchor right | cold decode in deadline | warm-at-dense decode |
|---|---|---|---|---|---|---|---|---|
| -20 | 0/24 | 0 | 23/24 | 1/24 | 12/12 | 11/12 | 8/8 | 8/8 |
| -19 | 3/24 | 1 | 15/24 | 6/24 | 12/12 | 11/12 | – | – |
| -18 | 1/24 | 0 | 13/24 | 10/24 | 12/12 | 12/12 | 5/8 | 8/8 |
| -17 | 23/24 | 23 | 1/24 | 0/24 | 12/12 | 12/12 | – | – |
| -16 | 11/24 | 3 | 5/24 | 8/24 | 12/12 | 11/12 | 0/8 | 8/8 |
| -15 | 8/24 | 0 | 13/24 | 3/24 | 12/12 | 10/12 | – | – |
| -14 | 5/24 | 4 | 13/24 | 6/24 | 12/12 | 11/12 | 0/8 | 8/8 |
| -13 | 2/24 | 0 | 16/24 | 6/24 | 12/12 | 12/12 | – | – |
| -12 | 1/24 | 0 | 12/24 | 11/24 | 12/12 | 12/12 | 6/8 | 8/8 |
| -11 | 2/24 | 1 | 13/24 | 9/24 | 12/12 | 12/12 | – | – |
| -10 | 7/24 | 6 | 13/24 | 4/24 | 12/12 | 11/12 | 5/8 | 8/8 |
| -9 | 20/24 | 18 | 1/24 | 3/24 | 12/12 | 12/12 | – | – |
| -8 | 23/24 | 23 | 1/24 | 0/24 | 12/12 | 11/12 | 0/8 | 8/8 |
| -7 | 2/24 | 0 | 18/24 | 4/24 | 12/12 | 11/12 | – | – |
| -6 | 1/24 | 0 | 13/24 | 10/24 | 12/12 | 10/12 | 3/8 | 8/8 |
| -5 | 0/24 | 0 | 23/24 | 1/24 | 12/12 | 11/12 | – | – |
| -4 | 19/24 | 16 | 0/24 | 5/24 | 12/12 | 7/12 | 8/8 | 8/8 |
| -3 | 24/24 | 24 | 0/24 | 0/24 | 12/12 | 11/12 | – | – |
| -2 | 1/24 | 0 | 15/24 | 8/24 | 12/12 | 11/12 | 7/8 | 8/8 |
| -1 | 2/24 | 0 | 13/24 | 9/24 | 12/12 | 11/12 | – | – |
| +0 | 0/24 | 0 | 24/24 | 0/24 | 12/12 | 11/12 | 8/8 | 8/8 |
| +1 | 1/24 | 0 | 17/24 | 6/24 | 12/12 | 10/12 | – | – |
| +2 | 5/24 | 3 | 12/24 | 7/24 | 12/12 | 8/12 | 8/8 | 8/8 |
| +3 | 24/24 | 24 | 0/24 | 0/24 | 12/12 | 12/12 | – | – |
| +4 | 19/24 | 16 | 1/24 | 4/24 | 12/12 | 7/12 | 7/8 | 8/8 |
| +5 | 0/24 | 0 | 22/24 | 2/24 | 12/12 | 12/12 | – | – |
| +6 | 2/24 | 0 | 15/24 | 7/24 | 12/12 | 10/12 | 7/8 | 8/8 |
| +7 | 1/24 | 0 | 18/24 | 5/24 | 12/12 | 10/12 | – | – |
| +8 | 23/24 | 23 | 1/24 | 0/24 | 12/12 | 10/12 | 1/8 | 8/8 |
| +9 | 15/24 | 14 | 3/24 | 6/24 | 12/12 | 11/12 | – | – |
| +10 | 10/24 | 6 | 11/24 | 3/24 | 12/12 | 11/12 | 3/8 | 8/8 |
| +11 | 5/24 | 5 | 5/24 | 14/24 | 12/12 | 11/12 | – | – |
| +12 | 3/24 | 0 | 13/24 | 8/24 | 12/12 | 11/12 | 4/8 | 8/8 |
| +13 | 3/24 | 1 | 13/24 | 8/24 | 12/12 | 11/12 | – | – |
| +14 | 5/24 | 1 | 3/24 | 16/24 | 12/12 | 11/12 | 4/8 | 8/8 |
| +15 | 2/24 | 1 | 12/24 | 10/24 | 12/12 | 11/12 | – | – |
| +16 | 17/24 | 4 | 1/24 | 6/24 | 12/12 | 11/12 | 1/8 | 8/8 |
| +17 | 23/24 | 22 | 0/24 | 1/24 | 12/12 | 12/12 | – | – |
| +18 | 1/24 | 1 | 16/24 | 7/24 | 12/12 | 10/12 | 4/8 | 8/8 |
| +19 | 4/24 | 1 | 13/24 | 7/24 | 12/12 | 12/12 | – | – |
| +20 | 0/24 | 0 | 24/24 | 0/24 | 12/12 | 12/12 | 7/8 | 8/8 |
| **all** | 318/984 | 241 | 445/984 | 221/984 | 492/492 | 444/492 | 96/168 | 168/168 |

**The real first frames.** The bursts were found by replaying each recording from its start through
A's path. The first SL2 decode matches B's first `OTA ACK received` in every run (f2f 21:32:43,
f2g 07:08:59, f2h 11:00:27, f2i 17:07:15, f2j 20:31:11 after three silent windows).

| burst | settle | dense / wide | settle class | x86 decode |
|---|---|---|---|---|
| f2f #1 | −7.20 | −7.20 / −7.21 | right | 0.65 s |
| f2g #1 | −9.87 | −9.87 / −9.84 | right | 0.65 s |
| f2h #1 | −11.69 | −3.85 / −3.80 | **Rs/4 off** | 0.69 s (native reach) |
| f2i #1 | −19.81 | −4.14 / −4.15 | **sideband** | 1.87 s (native reach) |
| f2j #1–3 | +7.21, −24.86, −25.31 | −8.45, −9.20, −9.65 | **sideband** | past deadline |
| f2j #4 | −9.75 | −9.75 | right | 1.77 s |
| corpus `ft991a-ota-frame-bpsk31-rs` (IC-9700 → FT-991A) | −12.80 | +2.82 / +2.65 | **sideband** | 0.64 s (native reach) |

- f2f–f2i "started cleanly" only because their offsets (−3.8, −4.2 Hz) are inside BPSK31's native
  reach. **f2i's first settle picked the sideband too, and f2h's was Rs/4 off.**
- The defect also hit warm frames (f2f #2 +7.45 against −8.2; f2j #5 +5.67 against −9.95). It hits
  the other rig pair as well (the corpus capture, A→B).
- The post-decode tracking update uses the same estimator. f2h's first decode moved the AFC to
  +1.09 Hz with the carrier at −3.8. At `afc_step` 0.1 that is a raw estimate of about +10.9, which
  is a sideband.
- Dense was right on all 14 real bursts.

**The other rungs (settle only, f2j hiss, 2 leads, 2 SNRs).**

| rung | Rs/2 | measured ±60 Hz in 4 Hz steps |
|---|---|---|
| SL3 BPSK63 | 31.25 Hz | **Same ambiguity.** ±31.25 picks accepted in 10/62 (SNR 12) and 13/62 (SNR 3) cells. Dense 124/124. |
| SL4 BPSK100 | 50 Hz | No accepted picks. At SNR 3, 25/62 rejected "other". Dense 124/124. |
| SL5 BPSK250 | 125 Hz | No picks. 32/62 (SNR 12) right but **rejected**: the coarse anchor was wrong, so \|fine − anchor\| > 20. Dense 124/124. |
| SL6 QPSK250-D | 62.5 Hz (4th power) | A different coarse stage: data-aided preamble correlation at baud/8. At SNR 3 in hiss, 42/122 settles accepted a wrong value. Decode impact **not measured**. |

## Part 2 — design (option 1)

### Candidates, measured on the same cells

| candidate | right, synthetic (492) | right, real (14) | why not |
|---|---|---|---|
| (a) rescore {c + k·Rs/2} | 347/492 (vote prototype: squared-line power at k ∈ −2..2) | 12/14 (misses f2h's Rs/4 error) | 36 % of the production settles are not on the Rs/2 lattice at all ("other", Rs/4), so no rescoring of {c, c ± Rs/2} can exceed (445 + 318)/984 = 78 %. Ranking by preamble ρ has the same cap. BPSK31 publishes no template, and a ranked decode per candidate costs up to two more full-frame BPSK31 attempts per off-frequency frame. |
| (b) BPSK250-type wide anchor, then refine | 444/492 (100 % at SNR 12, 80 % at SNR 3) | 14/14 | Its coarse stage is the same picket-fenced Goertzel. Its misses are multiples of 125 Hz, BPSK250's own Rs/2, so it moves the lattice rather than removing it. |
| **(d) dense coarse stage** | **492/492** | **14/14** | Chosen. 372/372 on SL3–SL5 too. |

**Decision: replace the BPSK coarse stage's 12.5 Hz Goertzel grid with a dense peak search of the
squared signal's spectrum.**
- The search runs over the same ±400 Hz, with the same DC and fc interferer guards. It is a
  zero-padded FFT read at a dense set of probes, then the best probe, then parabolic interpolation.
  *The dense stage, specified* below says exactly which probes, because a bin-range FFT is **not**
  equivalent to the Goertzel.
- The fine IQ-squaring stage then runs at the dense centre exactly as now. Its residual is small, so
  the ±Rs/4 wrap is never approached.
- This keeps the playbook's §5: the scan is still only the coarse stage, and the precise estimate
  stays with the IQ-squaring and data-aided stages.
- The prototype measured above is a 0.25 Hz Goertzel grid followed by the unchanged
  `afc_estimate_hz` re-centred at its peak. The production form is the FFT, which gives the same
  answer at about 1/400 of the cost. That equivalence must itself be a test; see gate 6.

### The dense stage, specified (review item 2)

`goertzel_carrier_scan` folds each probe frequency into [0, fs/2] and only then applies its
`fs/2 − 150` Nyquist guard. That matters once the settle has re-centred: at fc + c with c ≥ 300 Hz the
carrier's Nyquist **mirror** sits inside the ±400 Hz span, at the same folded bin as the carrier. An
emulated dense path diverged from the Goertzel at +300/+350 Hz for that reason, and a plain
bin-range FFT lost the carrier at +380 Hz (review). So the production form is specified by probes,
not by bins:
- **Probes.** Iterate δf over the FFT's bin grid in the δf domain, step `binw / m` (binw = fs / L),
  from −range to +range, **lowest δf first**.
- **Per probe**, exactly as the Goertzel does: the probe frequency is m·(fc + δf), folded into
  [0, fs/2]; skip it if it lies within 150 Hz of an interferer (DC, and fold(k·fc) for k < m); read
  the power of the nearest bin.
- **The Nyquist guard on the UNFOLDED frequency for m = 2:** skip any probe whose unfolded m·(fc + δf)
  exceeds fs/2 − 150. For m = 2 no probe can then reach the carrier's mirror. (For m ≠ 2 the folded
  guard is kept, because the M-th-power line of QPSK legitimately sits above Nyquist.)
- **Ties** go to the smallest |δf|, so an exact mirror tie that survives (m ≠ 2) resolves to the
  nearer candidate.
- **Refinement.** Parabolic interpolation over the winning bin and its two neighbours in the folded
  spectrum, with the sign flipped when the probe was folded from above Nyquist, then ÷ m.
- **Gate:** the sweep (gate 3) carries ±300, ±350 and ±399 Hz cells, run **through the mini-settle**,
  so that the re-centred passes (where the mirror lives) are what is tested, not only the first pass.

### Where it runs

- **Function.** It goes in a new `openpulse_dsp::acquisition` function,
  `mth_power_peak_scan(samples, fs, fc, m, range_hz)`. `rustfft` is already a dependency there. It
  is called from `plugins/bpsk/src/demodulate.rs::estimate_carrier_hz_wide`, which is stage 1 of
  `afc_estimate_hz_with_expected`.
- `goertzel_carrier_scan` stays as it is, for the pilot twin, and as the reference the gates
  reconstruct the shipped estimator from.
- **Seam.** The seam is the plugin's estimator, not the engine's settle.
  - Every engine AFC path reaches it through the one call at `update_afc_estimate`
    (`engine.rs` ~9474): `afc_mini_settle`, which covers the OTA trigger settle in
    `acquire_at_onset`, phase 2, the uncoded arm and the CLI receive paths, and also the
    post-decode tracking update (25 `update_afc_estimate` call sites).
  - The ambiguity is the estimator's property, so a fix inside `afc_mini_settle` would leave the
    tracking update wrong (f2h) and would need plugin knowledge (Rs) in the engine.
- **Proof of wiring.** The proof is the replay gate below, through `accumulate_capture` →
  `ota_decode_burst`. It must not be a callers-grep.

### Cost on a Pi (estimated from this host)

- **The current settle** costs 282 ms per settle on x86, 6 estimates of 47 ms. Most of that is the
  fine stage's timing lock and demodulation, not the Goertzel (65 probes × 8448 samples).
- **The dense stage** is one 32 768-point FFT of the squared window: 0.066 ms on x86 (16 384-point:
  0.031 ms; measured with `rustfft`, 500 runs). The probe loop adds about 6 500 table reads.
- **Pi factor.** Taking ×1.4–1.74 (the measured Pi/x86 ratio on this decode path) and allowing up
  to ×10 for FFT code without AVX, it is under 1 ms per estimate, so the settle's cost does not
  change measurably.
- **Decode time.** The fix moves first frames from "past deadline" (Pi 8.9 s) to the "right,
  accepted" path: 1.52–1.67 s x86 on the real bursts (review replay) and ≤ 2.62 s on the synthetic
  SNR 3 cells. That is about 3–4.6 s on the Pi, against a 6.18 s budget. The on-air proof is f2j
  burst 4, the same path, answered in time at 2.97 s.

### Constants (every new one)

- **FFT length = next power of two ≥ 2·N (zero-pad ×2).**
  - Fitted to: nothing. The ×2 is chosen so that bin spacing is at most half the main lobe;
    scalloping loss is then ≤ about 0.9 dB, against the current grid's ~28 dB worst case.
  - Inventory it must hold on: the 492 synthetic cells, the 372 SL3–SL5 cells and the 14 real
    bursts above.
  - Falsified by: a sweep at quarter-bin offsets losing any cell the 0.25 Hz Goertzel prototype
    wins, or an FFT-vs-Goertzel disagreement beyond the gate-6 tolerance on the same windows.
- **No new threshold.** The ±400 Hz range, the 150 Hz interferer guard, the `--++`/PN preambles,
  `AFC_SETTLE_DEADBAND_HZ`, the convergence check's constants (`last_delta < 5`,
  \|fine − anchor\| ≤ 20) and the 5 iterations at 0.7 are all unchanged. What the check *does*
  changes; see the next section.

### Behaviour change: the convergence check now accepts noise settles (review item 1)

An earlier draft said the convergence check is unchanged and that the "other" settles are almost all
rejected by it. **That rejection was an accident of the picket grid.** On noise the picket's argmax
jumps between probes as the settle re-centres, so `last_delta` and |fine − anchor| are large. A
dense peak is stable under re-centring, so a noise settle now converges, and the check passes it.

Measured by the review on windows with no frame, shipped against dense:

| windows | shipped accepted | dense accepted |
|---|---|---|
| f2j pre-run hiss, BPSK31 | 2/120 | 116/120 |
| corpus `ic9700-idle-wide-500hz-control`, BPSK31 | 0/118 | 113/118 |
| f2j pre-run hiss, BPSK250 | 5/120 | 99/120 |

- **Inert on the OTA arm** (review): four foreign-burst and noise-step fixtures gave identical
  results with and without the patch. That arm only settles inside a burst that carrier detect
  opened, and its acceptance is followed by the deadband and, for BPSK250, the preamble veto.
- **NOT shown inert** on the CLI `receive_with_timeout` path, where a converged settle latches the
  scan planner (#1021), or on BPSK250's veto counters (`rho_rejected_settles`,
  `rho_accepted_settles`, `settle_condemnations`). The veto and the condemnation recovery now see
  settles the convergence check used to drop.
- It also stops rejecting right answers whose anchor was wrong (SL5: 32/62 cells), so the
  corrected pass runs more often on BPSK250.
- **Gated, not noted:** gates 7, 8 and 9 below.

### Gates

The modem-level gates construct the engine as `server::run` does, **in code**, in one helper
(`daemon_like_engine` in `crates/openpulse-modem/tests/bpsk_settle_sideband.rs`): the nine plugins in
`server::run`'s order (BPSK, FSK4, MFSK16, OFDM, 8PSK, 64QAM, QPSK, SC-FDMA, PILOT); `mode = "BPSK250"`
(`cfg.modem.mode`) passed to both `accumulate_capture` and `ota_decode_burst`; the notch configured
as the daemon does (`notch_max`, `notch_q`, 2000 Hz, `notch_persistence` from `ModemConfig`'s
defaults); CE-SSB on; AGC off; `start_ota_session(fast)` with no lock; 400-sample reads. A harness
with an SL2 lock, `accumulate_capture(Some("BPSK31"))` and five plugins decoded **nothing**, with
the fix or without it (review), so the construction is part of the gate.

1. **f2j burst 2 decodes inside the deadline, cold** (deadline/attempt gate). One full cut: a whole
   66.6 s frame with 40 s of the recording's own hiss ahead of it, so the floor tracker is warm.
   Through `accumulate_capture` → `ota_decode_burst`, with `set_ota_ack_deadline` at the production
   budget (`ota_ack_timeout_ms` − 2000 − 300 − the FSK4 ACK airtime). `decode_ota_burst` is
   `pub(crate)` in the daemon, so the modem-level call is the gate.
   - Asserts the decode, `decode_attempts_past_deadline() == 0`, and the kept AFC within 2 Hz of
     the decoded carrier. Baseline (review replay, daemon construction): unpatched false with 152
     attempts past the deadline and 7.64 s; patched true in 1.52–1.67 s with AFC −9.25.
2. **The real trigger settles** (short cuts). Lead + 8448-sample cuts (about 50 KB each) of f2j
   bursts 1–3, f2h #1 and f2i #1, plus the committed corpus `ft991a-ota-frame-bpsk31-rs.wav`. The
   settle runs through the engine's own `afc_mini_settle` and convergence check (an `instruments`
   accessor), at window starts spread across the frame's first 0.8 s, because burst 4 showed the
   shipped settle's answer depends on the start. Each must land within 2 Hz of the decoded carrier.
   Baseline: f2j +7.21/−24.86/−25.31, f2h −11.69, f2i −19.81, corpus −12.80.
3. **The sweep**, by reference (`bpsk_modulate` at 1500 + Δ, embedded in the corpus idle).
   - Default run: the ten all-sideband offsets (±3, ±4, ±8, ±9, ±17) and the mirror cells ±300,
     ±350, ±399, through the mini-settle, SNR 12. All settle within 2 Hz. Baseline: 213 of 240
     sideband-offset cells in the full sweep picked a sideband.
   - The full 41 offsets × 6 leads × 2 SNRs × 2 noises stays a measurement (this note's Part 1), not
     a slow-tests suite: the default-run cells are the ones the picket loses, and dense was 492/492.
4. **`an_off_frequency_first_frame_does_not_pay_the_onset_scan`**, with its counts **pinned**:
   BPSK100 6 decode attempts at 0 Hz and 6 at +50 Hz (baseline 2026-11-07), asserted, not "still
   passes". The other three tests in `daemon_frequency_acquisition.rs` pass.
5. **The existing AFC tests, by where they run.**
   - Default run: bpsk `afc_estimate_detects_known_offset` and `afc_wide_detects_large_offset`; the
     dsp `goertzel_tests`; `afc_correction`, `carrier_offset_acquisition`,
     `legacy_receive_carrier_offset`; `coded_noise_settle_recovery`;
     `preamble_veto_interference::a_frame_behind_refusable_interference_is_still_acquired`.
   - **Not in the default run and not in `scripts/slow-tests.sh` either:** `carrier_offset_matrix`
     and `freq_acquire_accuracy` (`#[ignore]`d characterizations that print and assert nothing),
     `chain_veto_slow_rung` and the other 10 of 11 `preamble_veto_interference` tests (`#[ignore]`d
     verification runs). They are run by name, by hand, for this change, and the result is recorded
     in *Implementation*.
   - Then `scripts/gate.sh` and `scripts/slow-tests.sh`. Slow-tests is mandatory: this changes the
     acquisition chain.
6. **The FFT form agrees with the Goertzel prototype** on the gate-3 windows: the final estimate
   within 0.05 Hz of the 0.25 Hz-Goertzel prototype's (review: within 0.021 Hz with the
   interpolation, 0.061 Hz with the raw bin). In the default run, so the production form cannot
   quietly drift from what was measured.
7. **Noise windows** (behaviour change, part a). The settle on windows holding no frame — the idle
   corpus and the full cut's 40 s of f2j hiss — for BPSK31 and BPSK250, with **both estimators**:
   the product, and the shipped one reconstructed by reference (`goertzel_carrier_scan(…, 2, 400,
   12.5)` as stage 1 of the plugin's own stage 2). Asserts both acceptance rates inside bands around
   the measured values, so the change is visible in every run and cannot grow or vanish unnoticed.
8. **The CLI path** (behaviour change, part b). `coded_noise_settle_recovery`'s coded and uncoded
   BPSK250 round trips assert the condemnation and veto counters (`settle_condemnations`,
   `rho_rejected_settles`, `rho_accepted_settles`) as well as the decode, against bounds measured on
   both estimators.
9. **BPSK250's veto counters are a gate** (part c): the counters of gate 8 are asserted, not logged.
10. **Tone and two-signal controls, with the daemon's notch on.** A BPSK31 frame at Δ = −9 Hz,
    SNR 12, with a CW tone at fc + 137 Hz at +6, 0 and −6 dB relative to the frame, and separately a
    second BPSK31 frame at +200 Hz. Measured by the review with the notch **off**:
    - tone +6 dB: both estimators lock the tone, so not worse;
    - **tone 0 dB: dense converges on the cross-term line (+54.6 Hz, accepted), where shipped
      rejected it;**
    - tone −6 dB: dense right, shipped rejected;
    - two BPSK31 frames at +200 Hz: both estimators can lock the cross term at +95.5 Hz, dense
      reliably.

    Gated through `accumulate_capture` with the notch on. Where the notch does not remove a case,
    that case is a known limit (*Risks*), and the gate pins it so a change is seen.

### Sabotages (each must turn a gate red)

1. Put the 12.5 Hz Goertzel back as stage 1. Gates 1, 2, 3 and 7 must fail.
2. Remove the zero-padding (FFT length = N, rounded down to a power of two) or take every 50th bin.
   Gate 3 must fail on the attractor offsets.
3. Return the peak in the squared domain without dividing by m. Every nonzero offset in gate 3 must
   fail.
4. Skip the parabolic interpolation and keep only the bin argmax. Gate 3 must still pass, because
   the fine stage absorbs half a bin; gate 6 must fail at its 0.05 Hz tolerance only if the raw bin
   moves the final estimate by more than that. If gate 3 fails, the fine stage is not doing what this
   note claims.
5. Apply the Nyquist guard to the folded frequency, as the Goertzel does. The ±300/±350/±399 cells
   of gate 3 must fail.
6. Gates 8–9: sabotages named with the gate in *Implementation*, each watched red.

### Risks

- **A CW tone in the ±400 Hz span looks like a BPSK carrier in the squared spectrum.** A dense scan
  finds it reliably, where the picket grid found it by luck. The daemon runs the notch ahead of the
  burst, `notch_enabled` ships on, and A ran with it. Gate 10 measures what the notch leaves.
- **The `--++` sidebands are 11.8 dB (±Rs/4) and 15.6 dB (±Rs/2) below the main line in the squared
  domain.** A filter tilt or fade that suppresses the main line by more than that would pick a
  sideband. No real frame did (14/14).
  - Falsifier for the next on-air run: one `debug!` line per decoded burst with the trigger
    settle's answer and the AFC after the decode. Any disagreement over 2 Hz reopens this.
- **A second BPSK signal** produces a squared-domain cross term at the mean of the two carriers. A
  dense scan finds it as reliably as it finds the carrier; gate 10 pins what happens.

### Parked (one line each)

- **QPSK250-D's coarse stage** (SL6): data-aided, 42/122 wrong accepts in hiss at SNR 3; measure the
  decode impact at SL6 cold before trusting SL6 cold. Post-release unless it measures as a decode loss.
- **The deferred exhaustive fallback past the deadline** (*Secondary item* below): a separate change,
  which needs the mandatory keying review because it changes when a NACK is keyed; its bound is a
  tripwire-counted skip.

## Option 2 — seed the AFC from the previous session: not recommended for Release 1

- It does not reach a first contact with a new peer, or the first frame after a QSY, a band change
  or a rig change.
- The warm value it would seed comes from the same ambiguous estimator: f2h's tracking update read
  a sideband.
- The offset moves within a session (f2h −3.8 → −6.0 Hz in 70 s; f2j −8.4 → −9.95 Hz over 5 min).
- It adds persisted per-peer state.
- With the estimator fixed it is at most a hint. Park it as one line in the work plan, post-release.

## Secondary item — the deferred exhaustive non-ladder fallback ignores the ACK deadline

- `decode_fallback_near_trigger` runs with `non_ladder_decode = true`, so `past_decode_deadline()`
  is false throughout it.
- On f2j's failing bursts the x86 replay overran the 6.18 s deadline by about 1.5 s (7.7 s total).
  The Pi overran by about 2.7 s (8.9 s hold), which is consistent with the 2.7 s figure, not
  re-measured here.
- With the estimator fixed, the f2j-type first frame decodes in the corrected pass before reaching
  it. The cost then lands only on bursts nothing decodes, and there it can push an owed NACK past
  B's window, turning a NACK into silence.
- **Recommendation:** a separate, small change after this one, not inside it.
  - When a ladder deadline is set and the NACK verdict is `Keyed`, skip the deferred exhaustive scan
    once past the deadline, and count the skip in a tripwire.
  - Measure first, on the f2i/f2j replays, how many owed NACKs it delays. If none on the Release 1
    path, park it.

## Consumer

`ModemEngine::update_afc_estimate` (`crates/openpulse-modem/src/engine.rs` ~9474, the only
production caller of `ModulationPlugin::estimate_afc_hz`, with 25 call sites of its own). It is
reached for BPSK through `plugins/bpsk/src/lib.rs:218` → `demodulate::afc_estimate_hz` →
`estimate_carrier_hz_wide` → `goertzel_carrier_scan`. The daemon reaches it from `server::run`'s rx
tick → `nack_budget::decode_ota_burst` → `ota_decode_burst` → `acquire_at_onset` →
`afc_mini_settle`. Found with:
`grep -rn "estimate_afc_hz(\|afc_estimate_hz(\|estimate_carrier_hz_wide(" crates plugins --include=*.rs | grep -v /tests/`
(it matched the BPSK line above and the other plugins' own definitions) and
`grep -n "update_afc_estimate(" crates/openpulse-modem/src/engine.rs`.

## Prior art

- **The same defect, recorded but not diagnosed.**
  - `ota-scan-onset-major.md`, *Parked*: "the trigger settle's wrong estimate: −12.8 against
    +3.2 Hz on BPSK31, possibly a preamble sideband". It is −16.0 Hz, one Rs/2. That capture is now
    the corpus file `ft991a-ota-frame-bpsk31-rs.wav`, and its replay here reads −12.80, with dense
    +2.82.
  - `docs/dev/research/acquisition-comparison-codec2-ft8.md` §1.3 and §6 names the one trigger-point
    estimate, and FT8's "exhaustive but cheap grid", as the gap.
- **The playbook.** §3 says to settle on the refined-onset window. Here the window held the preamble
  in every synthetic cell: the gate's lead was 2448 in all of them, and dense is right regardless. §5 says a
  magnitude-peak scan is for coarse acquisition only, which this design respects.
- **The QPSK plugin's own comment** (`plugins/qpsk/src/demodulate.rs`, `coarse_scan_preamble`) records
  why a 4th-power Goertzel coarse stage failed there: crossfade k·baud lines alias over the wanted
  line. That is the same family of spectral lines; squaring (m = 2) keeps the main line 11.8 dB
  clear of the nearest structural line (review item 3).
- **`features.md`** gives the fine estimator's ±Rs/4 range (BPSK31 ±7.8 Hz), whose wrap is the
  Rs/2 lattice seen here.

Found with:
`grep -rn "trigger settle\|settle.*sideband\|15.6 Hz" docs/dev --include=*.md`, then by reading
`plugins/bpsk/src/demodulate.rs` (`afc_estimate_hz_with_expected`) and
`crates/openpulse-dsp/src/acquisition.rs` (`goertzel_carrier_scan`).

## Twins

- **`plugins/pilot/src/demodulate.rs:208`** has the same `goertzel_carrier_scan(…, 2, 400.0, 12.5)`
  picket grid. It is not on `hpx_hf`, so it is parked post-release; it should switch to the new
  scan in the same way.
- **QPSK (SL6 `QPSK250-D`)** uses a different coarse stage: a data-aided preamble correlation at
  baud/8 steps over ±400 Hz, then a 4th-power fine stage with ±baud/8 range and Rs/4 = 62.5 Hz
  ambiguity. Its settle accepted a wrong value in 42/122 hiss cells at SNR 3. Whether that costs
  decodes is UNCHECKED, and it is the next measurement before Release 1's SL6 is trusted cold.
- **psk8 / 64qam / scfdma** do not call `goertzel_carrier_scan`. Their estimators were not measured
  here (UNCHECKED); none of them is on the `hpx_hf` cold path.
- **The engine's post-decode tracking update** is fixed by the same change. It is a benefit, and
  gate 5 covers it.

Found with:
`grep -rn "goertzel_carrier_scan(" plugins crates --include=*.rs` (two production hits: bpsk:445,
pilot:208) and reading each plugin's `estimate_afc_hz`.

## Not established

- **Pi timings were not measured directly.** The ×1.4–1.74 ratio comes from two on-air decodes on
  rpi51, with the GPU on.
- **Deadline verdicts on x86 ran 4–6 cells in parallel** on a 24-core host, so contention could have
  pushed a borderline cell late.
- **The synthetic SNR is not calibrated** to the on-air frames: the plugin's SNR estimator returned
  `None` on the settle windows, and AGC FAST distorts the level ratio.
- **The FFT form of the dense stage has not been run in the decode path.** The 0.25 Hz Goertzel
  prototype was.
- **Not measured before review:** the tone negative control (the review measured it with the notch
  off; gate 10 runs it with the notch on); QPSK250-D's decode impact (parked); BPSK63's decode
  impact (its settle goes from 24 wrong accepts to 124/124 right, review); whether the convergence
  check's wider acceptance changes BPSK250's veto counters (gates 8–9).
- **The f2f–f2i replays ran today's `main`**, not the builds that were on air.

## Measurement record

All probes were temporary and are deleted:
- `crates/openpulse-daemon/src/zz_settle_probe.rs`;
- an instruments-gated accessor `crates/openpulse-modem/src/zz_settle_accessor.rs` (`zz_trigger_settle`,
  `zz_settle_from`, `zz_set_afc`);
- `crates/openpulse-dsp/tests/zz_fft_cost.rs`.

Run from `fix/bpsk31-settle-sideband` at `main` 23151ec6, on x86 release, with:
`cargo test --release -p openpulse-daemon --no-default-features --lib zz_settle_probe -- --ignored`.
The recordings are `rx-a-f2{f,g,h,i,j}-8k.wav.gz` on `m3/onair`, under
`docs/dev/test-reports/on-air/rx-audio-2026-11-01/`. The logs are the
`onair-twin-ota-2026110{5-223132,6-080749,6-115916,6-180604,6-212610}-evidence` directories.
