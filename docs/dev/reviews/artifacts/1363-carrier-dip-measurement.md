---
project: openpulsehf
doc: docs/dev/reviews/artifacts/1363-carrier-dip-measurement.md
status: review
last_updated: 2026-09-17
---

# #1363 — measuring the carrier-dip tie-break on the real chain

**This file is the packet AS SENT, kept verbatim below, plus the verdict.** It is kept unedited on
purpose: the review's value here was catching that I had misread my own numbers, and a packet
silently corrected afterwards hides exactly that.

## Prompt

Sent to Fable 5.1 as an adversarial review of a MEASUREMENT RESULT, before it entered any record.
The prompt supplied the packet (below) and the apparatus, and asked the reviewer to read the issue
thread IN FULL INCLUDING COMMENTS and check "whether it actually does [reproduce the thread's model]
or whether I am pattern-matching my numbers onto theirs".

Attacks requested by name, rather than a general "please review":
- is the windowed-RMS envelope measuring `|H(fc,t)|`, or something else?
- is aligning bit `i` to envelope symbol `i+1` correct under differential decoding — could an
  off-by-one MANUFACTURE or DESTROY the asymmetry?
- is normalising `|H|` to the run's own median defensible, or do the bins then mean different
  physical depths per seed?
- is the clean-channel reference genuinely truth, or could both arms agree and both be wrong?
- the deepest-bin soft asymmetry: noise or signal — "compute something, do not adjective it".

Closing instruction: say plainly whether the result is fit to write into the issue and ledger, and if
so give the exact honest claim, which would be used as the basis for the wording.

## Verdict (Fable, 2026-09-17)

**Core finding holds; packet not fit as written.** The hard-only bit-0 asymmetry is robust — every
apparatus defect found acts identically on both arms and can only DILUTE a one-arm asymmetry, never
create one, and the soft column is a paired control on the same samples. Reproduced independently to
every digit.

Not fit as written, for three reasons, all since fixed or scoped:
1. **Packet point 2 misread a MATCH as a deviation.** The thread predicts err|b1 = 1/3, not ~0.
2. **At 16 dB the deep bins are noise-dominated** (sigma_sym ~ 0.039 vs a fill of ~0.053), so
   "graded, not deterministic" is confounded with "graded by noise". An SNR axis is required first.
3. **Packet point 3 over-claimed**: the probe counts bits and has no byte/frame metric.

Apparatus changes required and made: assert truth against the generator; assert the envelope's
independence from `snr_db` (not same-config determinism); use absolute |H|. The deepest bin is
uninformative (Fisher p = 0.094 soft / 0.050 hard, optimistic because the samples come from ~30 null
crossings sharing channel state) and must be excluded rather than explained.

Scope stated more honestly than the issue body: the same recursion algebra turns ANY zero-mean
residual in a collapsed slot into a negative differential bias, `E[n_k n_k-1] = -beta*sigma^2/(1-beta^2)`,
of which the noiseless tie-break is the limit. A generalisation, not a rival.

## Consumer

`cancel_crossfade_isi` (`plugins/bpsk/src/demodulate.rs`) on the hard differential path, reached from
`stage_demodulate_payload` — i.e. every `Rs*` receive on `hpx_hf` SL2-SL5. The probe itself has no
production consumer by design.

## Prior art

The issue thread's own symbol-domain model (2026-09-13) established the mechanism, the beta-step and
the tap collapse; this measures the same signature on the shipped chain instead of a model.
`TwoRayAwgn` (`crates/openpulse-modem/tests/scfdma_multipath_timing.rs`) is the controlled two-ray
fixture the NEXT measurement should use.

## Twins

`QPSK250-D` (`hpx_hf` SL6) is an exact structural twin — `find_timing_offset` ->
`demodulate_symbols` -> `cancel_crossfade_isi` -> differential decode, no equalizer — and its soft
path REFUSES `-D`, so it has no soft arm and only an in-crate toggle could measure it. Its fade gate
was measured with cancellation ON only. `plugins/qpsk` and `plugins/psk8` ship the same transform but
their `crossfade_isi.rs` tests measure EVM at 40 dB AWGN on the COHERENT path, so they do not answer
this question.

---

# The packet as sent

## Apparatus

In-crate probe, `plugins/bpsk/src/demodulate.rs`, module `carrier_dip_tiebreak`. Composes the
SHIPPED private pieces — `demodulate_iq` -> optional `cancel_crossfade_isi` -> `differential_decode`
— so `cancel` is the only variable between arms.

**Two assertions run in the DEFAULT test run, not under `--ignored`:**

1. `the_composed_arm_matches_the_shipped_demodulator` — byte identity against `bpsk_demodulate`. So
   the probe measures the product's chain, not its own.
2. `the_same_seed_reproduces_the_same_fading_realisation` — worst-sample delta < 1e-6 between two
   `WattersonChannel`s built from the same seed. **The whole |H(fc,t)| recovery rests on this and
   nobody had tested it.**

`|H(fc,t)|` recovered by pushing a pure carrier through the same seed's realisation with `snr_db =
200`, envelope by windowed RMS over one symbol period x sqrt(2). Stated as an approximation: adequate
for BINNING BY DEPTH, and it avoids a Hilbert whose own correctness would then need establishing.

Truth comes from the CLEAN channel, and the run asserts both arms agree there (0 of N bits differ) —
if they disagreed with no channel, the reference would not be truth.

`moderate_f1`, 16 dB, BPSK250, **no FEC** (raw decisions are the subject), 96 seeds, 200 B payload.
Timing locks observed and logged: `[0, 1, 2, 3, 4, 5, 7, 8, 12]`.

## Result

| \|H\|/median | soft b0 | soft b1 | HARD b0 | HARD b1 | n |
|---|---|---|---|---|---|
| [0.00,0.02) | 0.625 | 0.357 | 0.625 | 0.321 | **52** |
| [0.02,0.05) | 0.379 | 0.336 | **0.795** | 0.353 | 248 |
| [0.05,0.10) | 0.306 | 0.301 | **0.605** | 0.333 | 856 |
| [0.10,0.20) | 0.162 | 0.177 | **0.337** | 0.197 | 3239 |
| [0.20,0.35) | 0.080 | 0.082 | 0.121 | 0.073 | 7563 |
| [0.35,inf) | 0.018 | 0.018 | 0.013 | 0.007 | 145340 |

Whole-run wrong bits: **soft 4142, hard 3605.**

## What I read from it — please falsify

1. **NOT falsified. The bit-value-conditional signature is present in the HARD arm and absent in the
   SOFT arm**, across three bins with n = 248 / 856 / 3239. Hard b0/b1 ratios 2.25, 1.82, 1.71; soft
   is symmetric there (1.13, 1.02, 0.92). The falsifier was "in-dip hard errors NOT concentrated on
   bit 0"; they are.
2. **The magnitudes are GRADED, not the deterministic signature.** Peak hard b0 is 0.795, not ~1.0,
   and hard b1 is ~0.33, not ~0. That matches the thread's own correction — the deterministic 2/3 is
   the deep-null LIMIT of a graded harm, not its onset — rather than the body's cleaner claim.
3. **The hard path makes 537 FEWER wrong bits overall while being worse in every dip bin.** That
   independently reproduces the thread's "895 fewer bit errors and decoded fewer frames", and it is
   the reason a whole-frame BER metric inverts this conclusion. Count bytes/frames, never bits.
4. **An anomaly I do not want to explain away:** in the deepest bin the SOFT arm also shows
   asymmetry (0.625 vs 0.357), which the mechanism does not predict. n = 52, so it may be noise —
   but I have computed no interval, and I would rather flag it than absorb it.

## What I am NOT claiming

- Not that the deep-null deterministic regime was reached: the deepest bin is 52 samples out of
  158 298, and my FIRST attempt (24 seeds, edges at 0.15/0.30/0.50) had that bin **empty** and
  averaged the signature away — the same dilution error the thread made and corrected once.
- Not that the delayed-ray-dominant half was isolated. `moderate_f1` draws complex rays per seed, so
  both dominance cases are sampled but neither is CONTROLLED. Review's recommended static two-ray
  with a controlled complex ratio is not built.
- Not anything about the fix. A dominant-tap gate is the candidate, with a known ceiling (the
  sign-sliced soft arm) and a ~5-10 % firing rate.

## Questions

- Is the graded asymmetry sufficient to call the mechanism confirmed, or does "confirmed" require the
  controlled two-ray grid and the deep-null regime?
- Is windowed-RMS envelope adequate, or does binning by depth need the analytic magnitude after all?
- Does the deepest-bin soft asymmetry need explaining before any of this is written down?
