---
project: openpulsehf
doc: docs/dev/reviews/review-ota-scan-onset-major-2026-11-01.md
status: resolved
last_updated: 2026-11-01
---

# Review: run the OTA burst scan onset-major (2026-11-01)

Adversarial review (Fable) of `cd3bf5c`:
- the rewritten `'scan` block in `ota_decode_and_ack_inner`;
- the design note `docs/dev/design/ota-scan-onset-major.md`;
- the test `crates/openpulse-modem/tests/ota_scan_onset_major.rs` and its real-capture fixture.

## Prompt

The prompt asked for falsification, not agreement, on six points:
- that the attempt set is identical;
- that the reorder cannot change which frame or candidate wins in a way that matters;
- state leakage: the AFC correction, `last_err`, `decoded_span`;
- the cost on healthy and failing bursts (the #1118 98-minute history);
- the causal story and the test's meaning;
- the CLAUDE.md rules.

It also asked "Does this block Release 1?". The playbook, the design note, the diff, the replay
harness and the recordings were sent with it.

## Verdict

**It does not block Release 1.** It removes a measured 8 s cost on the exact failing A2 path, and no
attempt that decoded before can stop decoding, except the corner in finding 1. The reviewer ran the new
test: it passes with 26 attempts, in 5.4 s in a debug build, in the default run.

Confirmed:
- settle order and timing are unchanged in effect;
- the rollback discipline holds;
- `last_err` has no production consumer that depends on which error it holds; the `nack_budget`
  evidence guards use spans and lengths;
- the memory used for the attempt list is trivial (worst plausible case ≈ 300 KB);
- no `unwrap` was added to library code.

## Findings and disposition

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | should-fix | "Same set" was false in one corner. When `0 < scan_end < step`, the old uncorrected pass made one attempt at onset `step` (unclamped), and the new walk dropped it. | **Taken.** The schedule adds that attempt explicitly; the code comment and the design note say so. |
| 2 | nit (unconfirmed) | The realistic divergence: the uncorrected pass decoding one step before the corrected one. The payload and level are the same; the AFC base and `decoded_span` shift by a step. | Noted. No rate-controller effect, and the winning slice re-runs `update_afc_estimate`. |
| 3 | nit | `last_err` now comes from the largest (onset, candidate, pass). | Noted. It is only logged; phase 2 and HARQ overwrite it on failing bursts. |
| 4 | — | Cost and memory. | OK. |
| 5a | should-fix | "Read size multiplies it: `scan_end` grows" does not hold for the measured rows. For BPSK31 the `4·acq` floor dominates until `onset_bound` is about 24 576 samples. | **Taken.** The note now says the read-size effect is measured, its mechanism UNCHECKED, and parks it. |
| 5b | should-fix | At 4096-sample reads it is still 4.2–4.9 s on x86, so on a Pi the fix is necessary, not shown sufficient. | **Taken.** In the note and in the M3 status line. |
| 5c | nit | The test pins a count, not the order. | **Taken as a stated limit.** `OtaRxResult` does not expose the winning onset; the note says what the test does and does not pin. |
| 5d | nit | The trigger settle uses the squelch-anchored window that the playbook's §3 warns against. | **Taken.** Cited in the parked item. |

Not changed: the trigger settle itself, the cost of bursts that cannot decode, OTA phase 2 and
`decode_burst_inner`. These are parked and twinned in the design note.

## Second round: the short-of-block guard (`2711829`)

The addendum to the design note was reviewed in a second round, with the same falsification brief:
- whether the suffix argument holds for every FEC and plugin;
- whether the flag lines up with the right candidate in the scan and in phase 2;
- whether skipping phase 2 loses anything;
- how robust the error parsing is;
- what the test means.

**Verdict:** no blocker, and it does not block Release 1. The guard is sound for every `hpx_hf` rung:
- the BPSK and QPSK byte count derives only from the sample count, so a suffix gains at most about one
  symbol, well inside the 2-byte margin;
- MFSK16 failures and soft FECs produce other errors and are never flagged, which is safe;
- the index alignment is correct;
- phase 2 loses nothing, because its slices are suffixes too and a correction moves only the lock.

The reviewer reproduced both tests: 26 and 511 attempts, 23.6 s in a debug build, default suite.

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 4 | should-fix | The engine's `rs_interleaved_decode_prefix` built the same message as a literal, so it matched the parser only by accident. | **Taken.** Both producers now build the error through `openpulse_core::fec::rs_short_of_block_error`. |
| 5 | should-fix | The addendum said HARQ never runs on the `robust` rungs. False: `FecMode::Rs` is admitted to HARQ and BPSK supports soft demodulation. | **Taken.** The wording is corrected; HARQ's share of the remaining cost is stated as unmeasured. |
| 1 | nit | `decode_variants` returns the last arm's error, so the length read can differ by at most one symbol. | Covered by the margin; no change. |
| 6 | nit | The test's 511 is mostly uncoded-fallback attempts, so it can drift with unrelated changes. | **Stated** in the note; there is no separate counter for coded attempts. |

