# Design proposal: dedicated frequency-acquisition stage

**Status:** proposal for review — not implemented. Addresses the #1 finding of
[reference-mining-plan.md](../research/reference-mining-plan.md) (task T2.1).

## Problem

Every modem in our reference set (gnuradio, liquid-dsp, qo100) runs a **dedicated
coarse-frequency-acquisition stage before phase tracking**. We don't: we estimate
the carrier offset by *settling* a per-mode AFC loop (`afc_mini_settle` in
`engine.rs`) over a coarse, often mostly-silent energy-gate window, then hand a
decision-directed Costas/pilot loop the residual. That path is the documented
root of a long string of carrier-offset acquisition gaps — QPSK500 (#413, AFC
settled on a near-silent window → bogus ~257 Hz), 8PSK (#417), the RRC modes
(#420), and the recent BPSK31 onset work (#455/#458). Each was fixed with a
point patch; the references show the durable fix is a real acquisition stage.

The two acquisition issues fixed this session (BPSK31 onset micro-sweep, SCFDMA52
retry) are *timing/buffering* fixes, not frequency fixes — they're orthogonal to
and complementary with this proposal.

## Proposed approach — qdetector-style FFT acquisition

Port the **liquid-dsp `qdetector`** design (`framing/src/qdetector.proto.c:491–752`,
MIT-licensed, freely portable), chosen over gnuradio's FLL band-edge (RRC-only,
continuous-stream model — wrong fit for our burst model) and qo100's PRBS-FFT
(tightly coupled to its waveform):

1. **Coarse stage (from the preamble), one O(N log N) pass.** FFT-domain
   cross-correlation of the received window against the known preamble sequence
   gives coarse timing τ̂ and coarse CFO Δφ̂_coarse simultaneously (the
   cross-correlation peak's location is τ̂; sweeping/searching its frequency bins
   gives Δφ̂_coarse).
2. **Fine stage (de-rotated), second pass.** De-rotate the window by the known
   sequence and locate the residual CFO as a **quadratically-interpolated FFT
   peak** — sub-bin resolution well below our current ~baud/16 preamble limit.
3. Returns **(τ̂, γ̂, Δφ̂, φ̂)** atomically — timing, gain, frequency, phase — i.e.
   exactly the "coarse-from-preamble, fine-from-payload-onset" two-stage we lack.

## Where it slots in

`crates/openpulse-dsp/src/` gains a `freq_acquire.rs` module (pure function over a
complex preamble window, no engine state). The engine's `receive_with_timeout_fec`
acquisition chain changes from:

```
energy gate → refine_onset → afc_mini_settle (per-mode AFC loop) → decode → carrier tracker
```

to:

```
energy gate → refine_onset → freq_acquire (τ̂,γ̂,Δφ̂,φ̂) → seed carrier tracker → decode
```

`freq_acquire` **replaces `afc_mini_settle`** as the primary estimator. The
existing per-plugin `estimate_afc_hz` stays as a fallback / cross-check. The
2-pass acquire-then-track carrier loops already in the dense plugins
(`dd_carrier_track_2pass`, the pilot tracker) become the *track* half, **seeded**
by Δφ̂ instead of starting blind — which is the seeding several plugins already
do ad hoc.

## The preamble question (the real decision)

Our 16-symbol preamble resolves coarse CFO only to ~baud/16 (31–62 Hz) — too
coarse for reliable single-pass acquisition (documented in the DSP playbook,
point 5). qdetector wants a longer m-sequence preamble for a sharp, sidelobe-free
correlation peak (plan task T3.7). This forces the interop decision:

- **Option A — new profile (recommended by the plan).** Add a longer-preamble
  waveform family; ship `freq_acquire` only on it. Preserves bit-exact interop
  with deployed modes; the new stage is opt-in per profile.
- **Option B — RX-only, current preamble.** Apply `freq_acquire` to the existing
  16-symbol preamble (accept its coarser resolution; the fine de-rotated pass
  still helps). No wire change, no new profile, interop-safe — but coarse-CFO
  reliability is capped by the short preamble.

**Recommendation:** start with **Option B** (RX-only, zero interop risk) to prove
the fine-stage value on the existing modes, measured against the carrier-offset
gaps; escalate to **Option A** only if the short preamble proves to be the
limiter. This de-risks the work and defers the interop-affecting change until the
data justifies it.

## Dependencies

- `rustfft` (already a workspace dependency via the OFDM/SCFDMA plugins — confirm
  the version and reuse).

## Validation strategy

- **Reuse the `carrier_offset_matrix` harness** (mode × offset characterization,
  from PR #420) as the primary metric: acquisition success vs applied CFO across
  BPSK31/63/100, QPSK500/1000, 8PSK, and the RRC modes — the exact modes that had
  gaps.
- **In-process** channel-sim sweeps (AWGN × CFO, SRO × CFO) for the regression
  floor; the dense modes (8PSK, 64QAM) are the canaries.
- **Dual-card hardware** full tier (the dual-clock regression guard — see
  [dualcard-loopback.md](../dualcard-loopback.md)); these real-time acquisition
  behaviours don't reproduce in-process.
- Gate: no regression vs the current per-mode AFC settle on any passing
  mode/offset, and a measurable widening of the acquired-CFO range on the modes
  that currently have gaps.

## Effort & rollout

Plan rates this **🟢 High benefit / M(edium) effort** — "the flagship DSP fix."
Realistic phasing:

1. `freq_acquire.rs` + unit tests (synthetic preamble + known CFO/τ) — self-contained.
2. Wire as RX-only (Option B) behind a flag; characterize on `carrier_offset_matrix`.
3. Decide Option A (new profile) only if the short preamble caps coarse-CFO reliability.
4. Remove/demote the `afc_mini_settle` heuristic once the new stage subsumes it.

Phase 1 is a clean, low-risk standalone module; the risk concentrates in phase 2
(touching the receive acquisition loop — the same loop stabilized in #455/#458, so
the dual-card full tier must gate every change there).

## Status

- **Phase 1 — DONE** (PR #460): `openpulse_dsp::freq_acquire::acquire()` — the
  standalone joint timing/CFO/phase/gain estimator. PR #461 switched its API to
  `(f32, f32)` tuples (no num-complex on callers) and added the
  `freq_acquire_accuracy` characterization.

- **Phase 2 (wire into `afc_mini_settle`) — TRIED, REVERTED. Net-negative; do not
  pursue this integration.** Two measurements settled it:
  1. *Estimator accuracy* (`freq_acquire_accuracy.rs`): freq_acquire is far more
     accurate than the per-plugin `estimate_afc_hz` — BPSK/QPSK are already fine
     (<2 Hz), but 8PSK500 has 35–48 Hz error (448 Hz at +200) and 64QAM up to
     7 Hz, which freq_acquire cuts to <2.5 Hz. This *looked* like a strong case.
  2. *End-to-end decode* (the thing that matters): wiring the accurate estimate
     into the settle CFO **regressed** 8PSK500 (9/9 across ±50 Hz → fails 4 cells)
     and improved essentially nothing (the OFF column is already near-perfect).

  **Root cause** (a clean confirmation of the DSP playbook's "AFC is the usual
  suspect, rarely the culprit"): the dense-mode demods do their **own** carrier
  acquisition from the preamble (8PSK's 2-pass DD tracker, #417; 64QAM's
  `dd_carrier_track_2pass`) and expect `afc_correction ≈ 0`. The old estimator's
  deadbanded ~0 value is *exactly* what lets the tracker pull in the full offset;
  feeding it an *accurate* −39 Hz pre-correction double-corrects and breaks it.
  freq_acquire's estimate was verified accurate at the failing cells (−38.8 to
  −40.6 Hz for −40), so this is not an estimator bug — the **AFC-settle is simply
  the wrong place**. The carrier-offset gaps this design targeted were already
  closed by the per-demod tracker work (#413/#417/#420).

  **If freq_acquire is to earn its place**, it needs a different role than AFC
  pre-correction — e.g. directly *seeding* a per-demod tracker (replacing its
  blind pull-in), serving modes that lack a robust tracker, or providing the
  detection metric/timing — not feeding `afc_correction_hz`. That is a separate
  investigation, not a continuation of this one. The module and its
  characterization remain as the validated estimator for whoever takes that on.
