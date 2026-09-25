---
project: openpulsehf
doc: docs/dev/reviews/review-1438-snr-estimator.md
status: resolved
last_updated: 2026-09-25
---

# Adversarial review — #1438 PR1: the BPSK SNR estimator, and the design rounds that led to it

Eight rounds by Fable, all read-only, each prompted for falsification. Rounds 1–6 reviewed the
design (v2 → v6) and the measurements behind each version; round 7 reviewed two side findings before
either reached an issue or this doc. The design history and every probe log are parked outside the
repo (`~/parked/openpulse-1438/`). The probes were temporary `#[ignore]`d tests and are not
committed.

## Consumer

- `plugins/bpsk/src/demodulate.rs` — `estimate_snr_db`, called through
  `BpskPlugin::estimate_snr_db` (`plugins/bpsk/src/lib.rs:166`).
- `crates/openpulse-modem/src/engine.rs:3195` — the OTA controller's input,
  `rx_snr_estimate.or_else(|| decoded_span → rx_snr_db)`, so a reading exists only for a DECODED
  span. `set_rx_snr_estimate` has no production caller (only its definition outside `tests/`).
- `engine.rs:4514/4555/4674/4787` — `record_rx_snr` → `last_rx_snr_db` → the QSY scan
  (`daemon/src/lib.rs:1398, 1421, 1431`; one stale number since #1312, so no decision moves), the
  ADIF RST (`:2637`), the panel.
- `engine.rs:4812` `receive_with_ack_hint` → `select_rx_ack_type` (:4870), from the ARDOP adaptive IRS
  (`crates/openpulse-ardop/src/bridge.rs:452`, default off): a decision consumer on the WHOLE buffer,
  misframed past ~1056 samples of lead-in (#1142). Its framing is out of scope here.
- `apps/openpulse-linksim/src/lib.rs:861` passes `Some(snr)` on failures too, so the linksim's
  fast-downshift moves under this change where the daemon's cannot.

Found by: `git grep -n "estimate_snr_db\|last_rx_snr_db\|rx_snr_db\|record_rx_snr\|select_rx_ack_type"`
over crates, apps and plugins; the hits above are the positive control for the absences (TUI, KISS,
mode_advisor, session_metrics).

## Prior art

- #934: per-window least-squares gain removal (`additive_snr_db_windowed`). This change extends it
  to two neighbour taps plus a frame-global residual frequency.
- #1142: estimate on the decoded span; kept.
- The data-aided mean-phase-increment frequency estimator is CLAUDE.md's DSP playbook item 5, applied
  here to the estimator rather than the demod.

## Twins

- QPSK/8PSK estimators have the same ISI-floor shape (to be filed).
- The GPU decode path locks with its own search while `estimate_snr_db` is CPU-only. The estimate
  spans ±0.3 dB over −0.45…−0.10n and reads 1 dB low at 0, so a one-sample disagreement is harmless.
- BPSK `-RRC` modes reach this path with no crossfade: the fit is harmless (neighbour taps → 0), but
  the constant is a Hann-pulse number (as the old 7.1 was).

---

## Prompt

**Rounds 1–3 (sweep, v3, v4).** Sent the forced-phase sweep, the alignment sweep and the design
text, asking the reviewer to break "the shipped objective already peaks at the optimum", "the
union's gain came from low-δ bursts", the estimator claims, and the production-δ arithmetic.
**Rounds 4–6 (v5, v6).** Sent each revised design, the new logs (the production estimator measured
on the cancelled stream; windows 8/12; carrier-offset controls; frame-global derotation), and asked
for every quoted number to be re-derived from the logs, the derotation's bias and alias limits, the
conditioning rule, and whether the controller harness was buildable. **Round 7.** Sent the BPSK31/63
carrier-offset decode result and the fade-plateau attribution, before either was written anywhere.
**Round 8 (the write-up).** Sent the actual ledger entry, this artifact, the review-1435 banner, the
CLAUDE.md row, the PR body and the code's doc comments, asking for hardened hedges, misplaced
emphasis, numbers that do not match their logs, provenance, and implausible sabotage rows.

## Verdict

**What the rounds overturned (each was a claim of mine).**

- The design v2 lock at the symbol boundary: the two crossfade arms want different sampling phases,
  and the uncancelled arm at its best beats the cancelled arm at its best (BPSK250, 200 B:
  −4.15 vs −2.85 dB AWGN; 6.17 vs 8.00 dB `moderate_f1`).
- "The shipped objective already tracks the channel": true on AWGN and flat fades, false on BPSK250
  `moderate_f1`, where it is 1.5–2.1 dB behind a fixed oracle phase.
- "The union's gain came from low-δ bursts": `ad7d7680` records 17 of 18 at a span one symbol
  before the frame.
- "PR1 switches on the fast-downshift": a failed decode passes `None`.
- A 4 % / 32 % exposure figure (the reviewer's round-3 arithmetic, which I adopted in v4) that was
  really 2 % / 16 % under the stated model.
- The v5 "1.5× std kill", which could never trip (the predicted ratio is 1.14).
- v6's written ω̂ formula, which was missing a conjugate.
- The claim that a 2 Hz residual is harmless on every rung.
- Pulse-matched timing (option A) was measured worse on the fade and withdrawn by the maintainer; edge
  rejection was dropped by the maintainer.

**The objective comparison the code cites** (reachability idealised, BPSK250, per-realisation
threshold in dB with never-decode counts; `moderate_f1` 200 B): the shipped half-Hann lock scores 8.04
(11); a pulse-matched lock with a −0.28n bias scores 8.35 (18); a fixed oracle phase at −0.16n scores
5.96 (1). On AWGN and a flat fade all three are within 0.45 dB.

**What the change is (v6 as implemented).**

1. `estimate_snr_db` reads the **uncancelled** stream.
2. Decisions come from its differential decode.
3. One frame-global residual frequency is removed, `ω̂ = arg Σ m_k·m*_{k−1}` with `m_k = z_k·d*_k`.
4. Per 8-symbol window, `z_k ≈ a·d_{k−1} + b·d_k + c·d_{k+1}` is fit by least squares, summed over
   windows before the ratio (`openpulse_dsp::constellation::{remove_residual_frequency,
   isi_aware_snr_db_windowed}`).
5. `MATCHED_FILTER_LOSS_DB` goes from 7.1 to 4.4.

**Round 7.**

- **Supported:** a BPSK31/63 decode failure near a residual offset of m·baud/32. The coherent
  32-symbol timing metric has Dirichlet nulls there, and the AFC settle discards sub-2 Hz corrections.
  It is pre-existing and separate from PR1; issue text needs the near-null framing, the SNR regime and
  the production chain, and must not quote a prevalence before a fine sweep.
- **Supported:** the remaining fade plateau is channel variation inside the 8-symbol window, not
  residual frequency. This is the reviewer's reasoning from the flat-fade, window-length and
  derotation controls, not a separate measurement; do not quote it onward as measured.
- **Not shown as I wrote it:** my 1 ms static-echo clause. Its low readings are decision errors on
  frames that do not decode, and they are never consumed. The derotation also costs 4 dB on that
  asymmetric echo at the shipped lock (18.3 vs 22.3 at true 30 dB) — a recorded limitation.

**Round 8.** Fourteen corrections, all applied:
- a doc stating the carrier-offset cap on the wrong scale and in the wrong direction;
- a harness doc claiming readings the old estimator was never observed to give;
- a banner correcting a remark in a meaning it did not have;
- "9 of 120" where the gate runs 180 cells;
- "not the estimator" where only the frequency estimate was excluded;
- `symbol_stream` left dead in the non-test build (a clippy failure waiting for the gate);
- the code citing design labels that exist only outside the repo;
- an unscoped fixture census;
- the old estimator's level generalised past its phase;
- a cap computed with the retired constant;
- criterion 2's reporting half missing;
- a probe figure labelled as the gate's.

Most usefully, it found that the "wrong decision index" sabotage was caught only by the payload's
sign balance, and that the DSP frequency test passed under it. A deterministic flip-heavy fixture now
closes that.

**Measured results of the change.** See the 2026-09-25 ledger entry.
