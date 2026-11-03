---
project: openpulsehf
doc: docs/dev/reviews/review-ofdm-carrier-offset-2026-11-03.md
status: resolved
last_updated: 2026-11-03
---

# Review: the OFDM receiver acquires its own carrier offset (2026-11-03)

Adversarial review (Fable) of `a7f42e9` and `docs/dev/design/ofdm-carrier-offset.md`. Recommended,
not mandatory: it adds constants to a DSP path (the coarse grid). It keys nothing and changes no wire
format or trait.

## Prompt

The prompt asked for falsification, not agreement. It asked about:
- the shift's correctness and the signs of its composition;
- wrong coarse picks: aliasing, interferers, leading noise, low SNR;
- what opting out of the engine's AFC loses;
- the cost per OTA decode against the Pi 4's ACK deadline;
- the constants' inventory and falsifiers;
- test quality;
- the side finding on the OFDM SNR estimate.

It also asked "Does this block Release 1?".

## Verdict

**Should-fix, no blocker; does not block Release 1.** Release 1 gates on 2 m with trimmed rigs
(≈4 Hz, high SNR). The shift and the sign chain were verified against independent anchors.

## Findings and what was done

| # | Finding | Done |
|---|---|---|
| S1 | The timing gate is on M = P²/R² ≥ 0.5, so it scales as cos² (0.65 at the 6.25 Hz worst residual), not 81 %. By formula, a halfway offset fails below ~9 dB SNR, and no test used a halfway offset | Measured at the falsifier: halfway offsets (+6.25, −18.75, +31.25 Hz), 8 seeds: SL10 near its floor and SL14 decode 8/8 with 12.5 Hz, as with 6.25 Hz. The step is kept; the doc carries the cos² and the measurement |
| S2 | Attempt 0 mixed the whole burst before any preamble was found: a 26 s burst is a 208k-point FFT pair per OFDM candidate | Only the window is mixed during the search, around the told offset; the burst is mixed once, and only when a preamble with pilots is found. 6 ms per call on a burst with no frame, whatever its length |
| S3 | Observability lost: no AFC telemetry after an OFDM decode, and no log | A debug line with the told and acquired offsets; the telemetry gap is parked (it needs a trait change) |
| N1 | `FftPlanner` re-planned per shift | Accepted (measured cost above) |
| N2 | `GATE_RESULT` placeholder in the ledger | Filled with the gate's result |
| U1 | A CW tone on a pilot bin with a real frame biases the fine estimate | UNCHECKED, noted |
| U2 | The peer's BPSK preamble sits on pilot bin 50 | Cost only, noted in the design |
| U3 | No fading, low-SNR or sample-clock-offset test | Low SNR now measured (S1); fading and SRO parked |
| — | The OFDM SNR side finding: a likely mechanism (noise-only post-roll symbols in the estimate); the fast ladder still climbs on evidence | In the design note; to the maintainer |
