---
project: openpulsehf
doc: docs/dev/reviews/review-1428-union.md
status: resolved
last_updated: 2026-09-23
---

# Adversarial review — #1428's union: the seam, the measurements, and the write-up

Three reviews by Fable, each before the thing it covered became permanent. Two earlier reviews of
the same line of work already have artifacts: `review-1428-step1-and-step2.md` (the review that
proposed the union in place of a per-symbol gate) and `review-1433-gpu-crossfade.md` (the union
design review that surfaced #1433, a prerequisite).

## Consumer

- `crates/openpulse-modem/src/engine.rs` — `decode_through_arms` / `decode_variants`, called by the
  six FEC-protected hard-decode chains: `receive_from_samples_with_fec_inner` (daemon, ARDOP, KISS,
  OTA, CLI `--listen-ms`) and `receive_with_fec` / `_interleaved` / `_concatenated` / `_strong` /
  `receive_with_short_fec_data` (CLI one-shot via `receive_with_fec_mode`, each reachable through
  `FecMode::ALL`).
- `plugins/bpsk/src/lib.rs` — `BpskPlugin::demodulate_variants`, CPU and GPU branches. The daemon is
  `default = ["gpu"]`, so the GPU branch is the one on air.

## Prior art

`stage_demodulate_payload` was the single-arm demod seam, with eleven callers each open-coding the
FEC and frame decode. `combine_and_decode_llrs` already takes a union of decode attempts on the soft
path, and CLAUDE.md records #694's "take the union" finding — the precedent this design follows.

## Twins

Six chains now go through the seam; five call sites remain single-arm, each with a stated reason at
`stage_demodulate_payload`. The GPU and CPU variant paths share `variants_from_parts` and
`bytes_from_symbol_stream`, so their framing cannot drift apart the way #1433's did. The uncoded path
(#1429) and `receive_with_soft_combining` (instruments-only) are deliberately not twins of this
change.

---

## Prompt

**Review A — the seam design, before implementation.** Sent the reachability table, the proposed
`demodulate_variants` trait method, `decode_through_arms` with a closure, the state-discipline rules,
and the scope. Asked hardest about the claim that computing both arms eagerly costs O(symbols), not
O(samples); whether `Vec<Vec<u8>>` was the right shape; whether a five-call-site helper, four of them
harness-only, is a real seam; the interaction with #1255 and #1123; sequencing against #1361; and
what the honest end-to-end number would be. Closing: *"anything else wrong or unproven — especially
any property of the code I have asserted that is not there."*

**Review B — the measurements and two hypotheses.** Sent the offset sweep, the cost measurement, the
rescued-frame SNR data with its paired control, and two hypotheses: H1 (the per-symbol carrier
rotation decides which offsets decode natively) and H2 (either decision-directed estimation or onset
misalignment explains the low SNR). Asked for an explanation, what separates H2's two mechanisms,
whether the tripwire should be reworded, and whether the SNR bias should block the union.

**Review C — the write-up, before commit.** Sent the ledger entry (which is also the PR body), the
two follow-up issues, the #1433 correction, and every code comment making a durable claim. Asked
first about provenance — every sentence stating the reviewer's measurements as the author's, or
stating something nobody measured — then headline order, the scope of "zero frames lost", and the
case for filing the AFC issue.

## Verdict

**Review A — two of my premises were false, and the design changed.** My reachability table called
two chains dormant on a grep; `receive_with_fec_mode` dispatches on `FecMode`, and the CLI iterates
`FecMode::ALL`, so every arm is reachable. The trait default would have returned ONE variant on the
GPU daemon. `estimate_snr_db` consumes the cancelled stream against a fitted constant, and the fade
gate's 3 dB tolerance could not see a swap. My own #1432 harness would go vacuous. The design moved
to routing every FEC-protected hard chain through one helper, with a closure that cannot reach
engine state.

**Review B — H1 had the right mechanism and the wrong criterion; H2 was not either/or.** The
cancellation leaves a residual larger than what it removes under rotation, but which offsets decode
is set by the crossfade term's phase against the symbol's own energy, not by the rotation's cosine.
The misalignment mechanism was ruled out as an account of the −19.3 dB reading. Two premises I had
asserted without knowing: that six idle starts were six alignments (they were one), and that the
timing lock survives 50 Hz (it collapses to ~5 % of its peak). The review also found a behaviour
change the decode columns hid — a 50 Hz station decoded before the settle learns no correction in
one step — which became the AFC sweep and a follow-up issue.

**Review C — "do not post as written".** Among its findings:
- A false correction: "72 days" (counted to the wrong end date; it is 71).
- A correction that claimed the merged commit message said "four months" (it gives no duration).
- A design doc giving the wrong mechanism for #1433 (it lived in the plugin, not the engine seam).
- A wrong reason for leaving `receive_with_soft_combining` single-arm (it is a hard chain of the
  cancelled arm).
- A test name that does not exist.
- "The union beats both arms in every measured cell" (it equals the better arm in most).
- My own AFC issue attributing two wrong corrections to the settle, when neither burst settled.
- Several of the reviewer's own measurements stated in my voice.
- A hypothesis written as a finding.
- An arithmetic error: a 16-symbol window is two bytes, not one.

"Zero frames lost" was re-derived from the raw logs and held. All findings were checked against
source or logs before being applied, and every one was applied.

**What this pattern says.** All three reviews found errors in claims about what the code or the
data *is*, not in the code. Review C found four of those errors in text written specifically to
*correct* earlier errors.
