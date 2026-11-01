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
