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

## Third round: the fallback-rescue bound, capture-read log and runner warm-up (`f6dec5a`, pushed as `48c64ef`)

**Verdict:** no blocker, and it does not block Release 1. Confirmed:
- the phase-2 ladder walk is identical to the old loop, including `scan_end == 0`,
  `scan_end < step`, no doubled end, and the labelled break;
- the logging's hot-path cost is negligible, and 8000 samples is correct because the engine runs at
  8 kHz;
- the warm-up keys nothing by itself.

Tests run by the reviewer, debug build, all passing:
- `off_frequency_fallback_is_not_ladder` 2/2;
- `ota_arm_uncoded_dispatch` 6/6;
- `burst_carries_several_frames` 2/2;
- `ota_scan_onset_major` 2/2;
- `ota_rate_decision_events` 3/3;
- `pn_preamble_parity` 2 passed, 6 ignored;
- the `onsets_nearest` unit test;
- `muted_failures_move_no_ladder` 1/1;
- `twin_multi_fragment_file` 3/3.

`twin_daemon_bridge::a_file_crosses_the_bridge_with_ota_enabled` timed out under parallel load in 2 of
8 runs with the change, and in 1 of 3 on the parent commit `56dce3f`. Single-threaded, both pass. It is
a pre-existing load flake, not shown to be caused by this change.

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 2 | should-fix (by analysis) | The cap was centred on the trigger point, but a frame's head precedes the trigger by up to the 2048-sample pre-trigger ring. At BPSK250, 64 onsets covered only ±1024. | **Taken.** Centred 1024 samples before the trigger (`OTA_FALLBACK_RESCUE_LEAD_BIAS`), which covers the ring at BPSK250. The note now says *trigger point* and states the remaining loss class, and that the rank-0 premise is UNCHECKED on real frames. |
| 4 | nit | "fell behind" alarms on every read after a normal multi-second Pi decode. | **Taken.** The info line now states the seconds of audio the read holds. The log volume is noted. |
| 2b | nit | `lead` is unclamped in one call site and `lead.min(n)` in the other. | Harmless (`abs_diff`); left. |
| 5 | note | A daemon dying during the warm-up is noticed only when polling starts. Side recordings shift 30 s relative to the report. | Noted for the task text. |


## Fourth round (mandatory, keying): no NACK while the channel is busy (`18f6616`)

Mandatory because it changes when a NACK is keyed. The prompt asked for falsification:
- a path that keys more than before;
- a unit mismatch between the busy check and the DCD squelch;
- a station muted for ever on a hot band;
- a blocking read in the receive loop;
- order, loss or duplication in `rx_pending`;
- the verdict and budget rules of #1460/#1456;
- whether the ladder candidates' acquisition walk is unchanged.

It also asked "Does this block Release 1?".

**Verdict:** no blocker, and it does not block Release 1. Confirmed by reading:
- the change can only key less (a decode is still ACKed);
- the busy check's RMS is the quantity the DCD gate compares;
- a hot band cannot mute for ever, because then the DCD never drops either;
- the read does not block (cpal returns after at most 10 ms, loopback drains at once);
- `Muted` failures still emit `OtaRateDecision`, so A2 scoring sees them;
- the ladder walk starts at `0.min(scan_end) = 0`, as before.

Tests run by the reviewer: `nack_budget` unit tests 6/6, `muted_failures_move_no_ladder` 1/1,
`twin_daemon_bridge` 8/8. The full daemon run did not link there (disk full); the pre-push hook ran it.

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | should-fix | `rx_pending` survives every site that drops `rx_stream`. After an ACK, pre-transmit carrier could be prepended to post-transmit audio, open a burst, fail, and key a NACK right after our ACK. | **Taken.** `rx_pending` is cleared at every site that drops the stream. |
| 2 | nit | The DCD sees post-notch audio and the busy check sees raw audio, so with the notch enabled a strong out-of-band tone can read busy. | Left: the notch defaults off, and a false busy only mutes. |
| 3 | note | The settle tripwire is fitted to one fixture. | Recorded in the test's doc comment. |
| 4 | unconfirmed | A carrier dip longer than the read still NACKs over the frame. | Already stated in addendum 3. |
