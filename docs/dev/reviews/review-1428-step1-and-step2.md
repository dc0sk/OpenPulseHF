---
project: openpulsehf
doc: docs/dev/reviews/review-1428-step1-and-step2.md
status: resolved
last_updated: 2026-09-22
---

# Adversarial review — #1428 step 1's write-up, and step 2's design (#1363)

Two reviews, both by Fable, both before the thing reviewed became permanent. The first covers the
**text** of the step-1 PR body and ledger entry; the second covers the **design** of step 2 before
any Rust was written. Both were prompted for falsification, with the apparatus attached.

## Prompt

Two prompts, both asking for falsification rather than confirmation, both sent with the apparatus.

**Review 1 — the write-up.** Sent the verbatim PR body and ledger entry for #1428 step 1, the raw
measured table, and the prior measurements the replication claim rests on. Asked specifically to
attack: the "survives its kill shot" headline; the SE ≈ 6.9 arithmetic and reasoning; the unmeasured
claim that in-dip SNR on an 8 dB fade passes through −2 dB; whether the replication paragraph dressed
up a null; the "first decode-rate measurement in the thread" claim; whether each of the four
"corrections" was itself correct; and whether the three named traps were real hazards actually
neutralised. Closing instruction: *"Anything else wrong, unproven, or overstated. Where you think I
am wrong, say what the correct sentence is."*

**Review 2 — the step-2 design.** Sent the E2 prototype's full numbers, the NumPy apparatus, and
three conclusions (C1 E2 beats `sign_dd`; C2 the kill criterion is unmeetable by a bare sign
predicate; C3 the product regressor is unusable). Asked whether C2's reasoning held or whether a fire
rule existed that met the criterion without a magnitude threshold; whether C3's stated mechanism was
right; what the static two-ray model licensed and what it did not; and whether a cheaper estimator
existed given the target is only a sign.

## Verdict

**Review 1: six errors, none in the measurement.** The table reproduces bit-for-bit (the reviewer
re-ran the harness). Every correction listed below was accepted and applied. The headline was
self-contradictory, the SE paragraph analysed a design nobody would run, one cell's SNR was
mis-transcribed in a way that carried an argument, and two novelty claims were false.

**Review 2: the design was replaced, not amended.** The premise that the fix must be a per-symbol
gate was never examined; the union of the two arms is computable from step 1's own discordant counts,
dominates any whole-frame selector, and needs no predicate. C1 survives but is narrower than I wrote.
C2 is wrong as stated — I substituted a signal-free null for the signal-present cell. C3's conclusion
is right and its mechanism was wrong.

Both verdicts were acted on before anything was pushed: the harness header, ledger entry and commit
message were rewritten, and a correction comment was posted to #1428.

## Consumer

- `crates/openpulse-modem/tests/engine_cancellation_ab.rs` — the instrument whose header, PR body
  and ledger entry review 1 corrected.
- #1363 / #1428 / #1429 — the decision thread. Review 2's union analysis is what re-scopes step 2.
- `docs/dev/project/traceability.md` — the ledger entry rewritten as a result of review 1.

## Prior art

`git log --oneline --grep='#1363'` and the #1363 thread: three prior measurements of the same 8 dB
cell (opening 28/26 engine-level, bad-byte proxy 33/45 lost, this run 49/38). #694's "take the
union" finding in CLAUDE.md is the precedent review 2 invokes; `grep -n 'take the union' CLAUDE.md`
confirms it is already project doctrine on the soft path.

## Twins

The union argument applies to any site choosing between two decoders of the same symbols. The
sibling paths: `receive_with_fec_mode` (single-shot), `receive_from_samples_with_fec` (scanning),
and the daemon's OTA HARQ branch — which already folds the *uncancelled* arm's LLRs into a retry,
so on air a retransmission is neither arm alone.

---

## Review 1 — the step-1 write-up: six errors, none in the measurement

The table reproduces bit-for-bit; the reviewer re-ran the harness. What was wrong was the framing.

1. **σ = 0.9 is −3.34 dB, not −8.3 dB.** The harness prints
   `tx len 66560 rms 0.6126; sigma0.9 = -3.34 dB`. I computed "8.34 dB below SL5's floor" and wrote
   that down as the absolute SNR. Load-bearing: it was the basis for treating −2 dB and σ = 0.9
   asymmetrically, and the two cells are 1.3 dB apart. On the Rayleigh model ~9.5 % of an 8 dB fade
   sits below −2 dB and ~7.1 % below −3.34 dB — **nothing in the table distinguishes them.**
2. **"The first decode-rate measurement in the thread" is false** — #1363 opens with frame counts.
   The checkable claim: the first since that opening, and the first whose apparatus is known to put
   both arms through the same hard RS.
3. **The SE paragraph analysed the wrong design.** Paired by seed from the opening onward, so the
   unpaired SE ≈ 6.9 (and its p ≈ 0.11) belongs to an analysis nobody would run. Paired:
   SE = √(b + c − (b−c)²/n) = 3.97, threshold ≈ 2σ, and **+11 clears it by 3 frames — under one
   paired SE** (Wald 95 % CI [3.2, 18.8] contains the kill region). The switch to the paired
   statistic was made *after* seeing the data.
4. **The −2 dB cliff is not new** — #1363's proxy table already had it (56 lost vs 0). What was 2 dB
   too narrow was my own "equality in AWGN ≥ 0 dB" restatement; −1 dB is informative too (6:0).
5. **The replication paragraph dressed up a null** — the opening's 8 dB cell was +2/64. The 8 dB
   result is newly positive; 12 dB is what replicates. The proxy's "33/45" counts *lost* frames.
6. **The AFC assert is not the guarantee** — a fresh engine per arm per seed removes carry-over by
   construction; the assert pins the constructor's initial state.

Also recorded: the pre-registered "exact equality with the shipped arm in AWGN ≥ 0 dB" check is
**vacuous** — soft is itself 96/96 there, so only a fire-rate measurement carries that requirement.

## Review 2 — step 2's design: the premise I did not know I was asserting

Step 2 was specified as "port the pairwise-product estimator E2 as a gate arm". The review's first
finding is that **the fix need not be a gate at all**, and that step 1's own discordant counts
already contain a baseline no gate was measured against:

| cell | soft | hard | **union** |
|---|---|---|---|
| `moderate_f1` @ 8 dB | 49 | 38 | **52** |
| `moderate_f1` @ 12 dB | 69 | 56 | **71** |
| doppler-only @ 8 dB | 64 | 83 | **83** |
| awgn −2 / −1 / σ0.9 | 12 / 90 / 0 | 96 / 96 / 16 | **96 / 96 / 16** |

The union needs **no predicate** — decode both ways and let RS plus the 4-byte length prefix and
CRC-16 adjudicate — so it is implementable rather than an oracle, at one extra decode on symbols
already in memory. It is never worse than the better arm, beats it on both fade cells, is the
ceiling a whole-frame selector can reach, and is the floor a per-symbol one must beat. **A gate's
burden is 52 / 71, not `sign_dd`.**

### On the E2 prototype

The NumPy static two-ray prototype does show E2's constant term beating my port of `sign_dd` in the
deep-fade band (0.995 vs 0.790 sign accuracy at the aligned lock, 0.995 vs 0.744 at the late lock;
false-fire 0.003/0.021 vs 0.278/0.375). Three qualifications the review attached:

- The win is **confined** to |H| ∈ [0.05, 0.20) and the late lock; over all |H| at the aligned lock
  the two are indistinguishable (0.986 vs 0.983). Sign accuracy does not predict frame count.
- `e1` is **my port** of `sign_dd`, not `sign_dd`; its numbers must not be quoted as the shipped
  arm's.
- **W = 81 is not licensed by a static model**, where longer is monotonically better. At 250 baud it
  is 0.32 s against `moderate_f1`'s coherence time, so the target's sign can flip inside the window.

### On my claim that the kill criterion was unmeetable

I measured a **signal-free** null (pure noise) and found all estimators firing ~50 %, concluding no
bare sign predicate can meet "σ = 0.9 fire rate < 1 %". The review's correction: the harness's
σ = 0.9 cell has **signal present**, where the constant's expectation is +β|g_c|² > 0, so the fire
rate there is not 50 % and is SNR- and W-dependent. I substituted a null for the cell — the same
error as the criterion's, in the other direction.

What survives, and is the sharper rule: **a kill criterion names its statistic AND that statistic's
expected value in the cell where it is measured.** #1428's named neither.

### On the mechanism behind the discarded regressor

I attributed the `e_k e_{k+1}` coefficient's poor accuracy to "the regressor is a function of the
observation". The review supplies the exact identity: with `ê_k = sign(Re z_k)`,
**`Re z_k · ê_k ≡ |Re z_k|`**, so that coefficient is a half-wave-rectified statistic whose error
samples are exactly the large-|ν| ones — tail-weighted bias, tracking the raw decision-error rate.
My explanation was wrong in a way that matters: it is also true of `ê_k`, which does no damage.

Corollary: the column is exactly collinear with the constant on the alternating preamble, and the
target term vanishes there identically — so no pilot-aided or decision-free variant exists.

## Disposition

- Review 1's six corrections: applied to the harness header, the ledger entry, the commit message
  and a correction comment on #1428.
- Review 2: step 2 as specified is **not** the next move. The union is measured (from step 1's own
  counts) and dominates; implementing it in production is a product change and the maintainer's
  call, and it likely subsumes #1429 by using both arms rather than choosing one.
