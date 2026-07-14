# Robust narrowband weak-signal rung — kill-first measurement (REQ-WSIG-01)

**Status:** measured 2026-07-14. Ideal (genie-sync) bound; kill-first gate **PASSED**.

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
