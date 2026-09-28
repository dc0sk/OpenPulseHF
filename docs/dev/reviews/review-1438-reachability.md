---
project: openpulsehf
doc: docs/dev/reviews/review-1438-reachability.md
status: resolved
last_updated: 2026-09-27
---

# Adversarial review — #1438 PR2: BPSK timing reachability, and deciding between two locks

Five rounds by Fable, all read-only and prompted for falsification. Round 1 reviewed the measurement
behind the policy choice and the P6 recommendation, before any code was written. Round 2 reviewed a
first write-up. Round 3 reviewed the design of a δ histogram, before it was built. Round 4 reviewed a
measurement that contradicted round 3. Round 5 reviewed the final write-up. PR1's design rounds are
in `review-1438-snr-estimator.md`. The probes are parked outside the repo (`~/parked/openpulse-1438/`), except the pre-registered instrument, which is
committed as an `#[ignore]`d test (`two_lock_policy_measurement`).

## Consumer

- `crates/openpulse-modem/src/engine.rs:7464`, `stage_demodulate_variants`: the hard decode for every
  FEC arm, reached from `receive_from_samples_with_fec_inner` (:4652), `receive_with_fec` (:5290,
  :5371, :5524), the OTA/burst decode (:7500) and :6915. It adjudicates the variants with the FEC and
  counts non-primary wins in `alternate_arm_decodes`. (Line numbers point inside the functions:
  `stage_demodulate_variants` starts at :7451.)
- `BpskPlugin::demodulate` (`plugins/bpsk/src/lib.rs`) = variant 0, for every caller of the single
  hard path.
- Single-lock consumers, each on the widened lock alone: `bpsk_demodulate_soft` (called inside
  `demodulate_soft`, `lib.rs:138`), reached by the uncoded path (`receive_from_samples` sign-slices it)
  and by `ota_demodulate_soft` (HARQ); `estimate_snr_db`; `afc_estimate_hz_with_expected` stage 2.
  The uncoded path's production callers all scan onsets in steps of n: the OTA fallback
  (`decode_burst_phase1`), the non-OTA arm, the monitor, the repeater, ARDOP and KISS (round 3).
- The GPU twin: `bpsk_demodulate_variants_with_gpu`, with `openpulse_gpu::timing_energies_gpu`.

Found by: `git grep -n "demodulate_variants\|stage_demodulate_variants"` over crates/apps/plugins
(the hits above), and the callers of `timing_locks_with_expected` in `demodulate.rs`.

## Prior art

- #1439 (`ad7d7680`): the early lock was the objective's peak, not only a defect.
- #1428 (`e4f78694`): the variants trait and FEC adjudication; PR2 adds a second axis (lock) to it.
- Rejected here: the upper-edge rejection (P2) and lower-edge fallbacks (P4/P5), measured below.
  Round 1 proposed P7e (the union only when P0 is within n/16 of an edge) as the fallback if P6 cost
  too much; it was not needed.

## Twins

- GPU BPSK path: implemented the same way (one energy readback, CPU `pick_lock`), and gated by a
  new GPU equivalence cell where the locks differ (in-crate, since its fixture guard needs the
  crate-private locks). That gate is manual-tier (needs `--features gpu` and an adapter), per
  #1433.
- QPSK/8PSK/64QAM share the `[0, n)` search shape: #1444, unmeasured.
- The daemon's burst accumulator keeps no pre-trigger audio (#1443); PR2 recovers about a quarter
  symbol of negative δ and does not replace that.

---

## Prompt

**Round 1 (design, before implementation).** Sent the six-cell policy probe (P0 … P6, 96 frames per
cell, per-alignment decode masks), the scan model, and five questions: is the apparatus sound; what
does "try both" cost; which consumers cannot try both and what they should do; what do the logs say
beyond the headline; what else bites. Asked for the edge-rejection drop to be tested against its
revert. **Round 2 (write-up).** Sent the draft ledger entry, PR body, acceptance row, this artifact,
the commit messages and the doc comments in the diff, asking for every number to be re-derived from
the logs and for hardened hedges and misplaced emphasis. **Round 3 (histogram design).** Sent the
design of a production δ histogram for the single-lock consumers, with the code and the measured
per-δ curves, asking whether it answers the decision. **Round 4.** Sent the SNR-at-alias probe, its
source and log, asking which of rounds 1 and 3 it supports. **Round 5 (write-up).** Sent the final
ledger entry, PR body, acceptance row, this artifact, the issue texts, the commit messages and the
diff, asking for every number to be re-derived and every round-2 item checked.

## Verdict

**Round 1.**

- Implement P6, widened-first, with the restricted lock as the rescue. Do NOT revert the
  edge-rejection drop: upper-edge rejection (P2) was measured harmful (−91 single-slice, −27 scan
  in the worst cells), and it cannot catch the alias it was meant for. The −2-symbol alias lands at
  the LOWER edge, and a lower-edge rule removes the negative-δ recoveries PR2 exists for, because the
  true lock at δ ∈ [−0.25n, −0.15n) sits at the lower edge too.
- The apparatus (padding emulating a signed search) is sound for decode outcomes, but not
  bit-identical: implement the signed range inside the demodulator, with a zero-read, so the
  `[0, n)` energies stay bit-identical to the shipped search. Done, and pinned by a verbatim
  pre-PR2 reference.
- Two cells were underpowered (BPSK100 AWGN −9 dB, BPSK31 AWGN −14 dB, ~10/96): re-run ~1 dB
  higher. The cost driver (lock-differ rate on preamble-free windows) was unmeasured. Both were
  requested for the implementation's measurement and are reported in the ledger.
- Dedupe is mandatory (the trait bans duplicate variants); the trait's "from ONE acquisition"
  becomes false and was reworded; `alternate_arm_decodes` now counts lock rescues too and is
  documented as "non-primary variant".
- Soft/HARQ, SNR and AFC take the single widened lock. In the alias band the soft path now retains a
  2-symbol-shifted LLR vector where P0 retained an aligned one; a misaligned newest vector poisons
  that burst's combine, bounded by `OTA_HARQ_MAX_ATTEMPTS = 3`, with no false delivery. Recorded,
  not built.
- The GPU twin must gain an equivalence cell where the locks differ, or the second-lock GPU path is
  #1433's untested copy. Done.

**Round 2.** Not ready. Corrections, all applied:
- the "no regression" headline holds by construction, so it is reported as a wiring check;
- the 2× cost kill had never been pre-registered, so it is reported without a threshold;
- only four cells were comparable with the probe;
- the skipped pre-registered items are now a declared deviations list;
- the direction of the next scan onset was wrong;
- three quoted numbers had no saved run, and now have one;
- a "WIP" commit message;
- eleven comments still described a one-symbol search, including the module doc of the file changed;
- the maintainer's decisions had no durable record.

The largest finding: consumers that take one lock get the widened lock alone, and the pre-registered
kill for them had not been evaluated. Measured: it trips.

**Round 3.** Don't build the histogram as specified. The uncoded consumers are scan consumers, which
the existing scan-level data answers; HARQ's soft arm is nearly moot on air (#1139). It also stated
that the SNR on a decoded span, after a rescue, reads #1142's "+5 … −8 dB swing" — a defect to fix.
Side finding, confirmed through the real `accumulate_capture`: behind a narrow receive filter the
squelch collapses to its clamp.

**Round 4.** The measurement stands and supports round 1, not round 3. The "+5 … −8 dB swing" came
from #1142 (`engine.rs:3179–3186`), a noise-argmax lock with no preamble in view, and does not
transfer to the alias lock; round 3's claim is **retracted**. Both measured columns follow the
estimator's own phase response at the phase each lock sits at, so reading at the lock that decoded
would make AWGN readings worse. Recommendation, adopted by the maintainer: no SNR/AFC change in PR2;
file the fade minority.

**Round 5.** Every number in the ledger, PR body, acceptance row and issue texts that has a saved
log re-derives from the raw logs with an independent script; the sabotage results have no saved run
and could not be checked. Round 2's items are resolved. Not ready as written, for twelve corrections
none of which changes a conclusion, all applied:
- the tripped-kill list omitted an in-band −4 (BPSK100 fade, 1.375n) and did not say the uncoded kill
  also trips, once, by one frame;
- the SNR figures were rounded past the data;
- the soft/HARQ limitation named an issue where the disposition is a comment on #1139;
- the stale-comment sweep missed six sites, so its count became a checkable grep;
- three re-pointed tests were missing from the test list;
- #1429 lost its characterisation test with no comment drafted, and the texts disagreed about what
  stays open;
- a doc comment quoted "on `main`" BER figures with no saved run, now replaced by the saved PR2 run;
- smaller provenance and wording fixes (the probe SNRs, the deviation's construction path, the AFC
  loop gains, the narrow-filter probe's run, `Refs #1438`).

The single-lock section fairly presents the tripped kill; the retraction is correctly scoped to the
alias lock and sourced; nothing claims to be gated that is not.

**Measured results.** See the 2026-09-27 ledger entry for #1438 PR2.
