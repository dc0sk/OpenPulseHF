---
project: openpulsehf
doc: docs/dev/dsp-acquisition-playbook.md
status: resolved
last_updated: 2026-09-30
---

# DSP acquisition and carrier-recovery playbook

> Moved verbatim from `CLAUDE.md` on 2026-09-30 (work plan decision 7). Code comments that cite "the DSP playbook in CLAUDE.md" mean this file. Read it before touching any plugin's demodulator or the engine's acquisition path.

Blind acquisition — recovering timing, frequency, **and** phase simultaneously from a 16-symbol preamble — is the single most-churned and most-misdiagnosed area of the modem (60+ AFC/carrier commits). These are hard-won, load-bearing practices; read them before touching any plugin's demod or the engine acquisition path.

1. **Diagnose an "AFC" failure with the swept-applied-correction experiment FIRST.** When a mode won't acquire through an offset, modulate at `fc+Δ`, then demodulate with a *manually swept* `afc_correction_hz` (and matching `center_frequency`). If it fails even at the exactly-correct Δ, the estimator/AFC is **innocent** — the bug is in timing, onset, or the carrier tracker. This one check relocated the 8PSK gap (PR #417) from "AFC precision" (where earlier sessions spent days on FLL / preamble-redesign / liquid-dsp ports) to a broken drift-fit branch in `carrier_phase_correct`.

2. **AFC is the usual suspect, rarely the culprit.** The acquisition chain is `energy gate → refine_onset → afc_mini_settle → decode → carrier tracker` (`crates/openpulse-modem/src/engine.rs`); a weakness in *any* link reads as "doesn't decode → must be AFC." Historically these were: onset landing (BPSK31 #406, QPSK500 #413), timing metric at 90° carrier phase (`5dded08`/`866b085`), sample-rate offset on the dual-clock rig (#391/#392/#397), and carrier tracking (8PSK #417) — **not** the AFC estimate.

3. **Settle AFC on the refined-onset window, never the coarse energy-gate window** (it may be mostly silence → a confident-but-bogus estimate, e.g. QPSK500's spurious ~257 Hz). And **don't apply sub-noise-floor (<2 Hz) settled corrections** (`AFC_SETTLE_DEADBAND_HZ` in `engine.rs`).

4. **Carrier recovery is acquire-then-track, not one loop.** A gentle (low-BW) loop holds lock but **cannot acquire** even a ~1 Hz residual over a short (~60–200 symbol) frame. Use two passes: pass 1 wide BW to acquire the frequency, pass 2 narrow BW *seeded* with it to track cleanly. 64QAM (`dd_carrier_track_2pass`) and 8PSK (`dd_track_seeded`, #417) both do this. A single high-BW loop fixes the offset but regresses clean/dense modes (8PSK9600) — the split keeps both.

5. **Don't try to extract sub-Hz CFO from the 16-symbol preamble by a magnitude-peak frequency search.** Its frequency resolution is only ~baud/16 (31–62 Hz) and the magnitude metric is sidelobe-ridden; a coarse scan locks to spurious peaks (−100…−256 Hz observed). Use a scan only for *coarse* acquisition; leave the fine residual to the 2-pass tracker. The data-aided mean-phase-increment estimator is the precise stage (ISI-biased ~0.9 Hz, which the tracker now absorbs).

6. **Dense constellations are the regression canaries.** 8PSK (±22.5° margin) and 64QAM surface every timing/phase/AFC weakness that BPSK/QPSK hide. Validate acquisition changes against them, not just BPSK.

7. **Rebuild BOTH ends for any loopback test** — the preamble sequence and frame geometry are shared protocol; a one-sided rebuild fails silently with "invalid magic."

8. **Test FEC-protected modes WITH their FEC.** Dense modes (SCFDMA-HOM, 64QAM) only ever run FEC-protected, so a no-FEC loopback is an unrealistic bar — use the loopback `FEC=` env / CLI `--fec`. Soft FEC (~+6 dB) was the bigger lever that the loopback had never exercised.

External modem/DSP references (gnuradio FLL band-edge, liquid-dsp framesync, daniestevez/qo100-modem) are catalogued in `docs/dev/research/references.md`. Recurring lesson: those references all use **RRC pulse shaping + a dedicated frequency-acquisition stage**; our rectangular single-Costas PSK is the outlier, which is why band-edge techniques don't drop in cleanly.
