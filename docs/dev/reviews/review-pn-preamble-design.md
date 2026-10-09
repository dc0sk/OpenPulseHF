---
project: openpulsehf
doc: docs/dev/reviews/review-pn-preamble-design.md
status: resolved
last_updated: 2026-10-05
---

# Review: PN-63 preamble design (#1062), mandatory wire-format review (2026-10-05)

Adversarial review (Fable) of `docs/dev/design/pn-preamble.md` revision 1 (`31d6b898`), before any
implementation. Read-only: no build, no file changes. Revision 2 of the design folds in every
blocking and should-fix finding below.

## Prompt

Falsify, do not agree. Sources: the design, the #1062 thread dumped in full (with the instruction
that the body is superseded by the comments, naming revisions 2–4, the 2026-08-04 parity results and
the 2026-09-07/10/11 measurements), the BPSK plugin, `openpulse-dsp::preamble`, the engine's veto,
search plan, ranking and hard-coded geometry, `demod_parity.rs`, and the release criteria's break
package. Attack: (1) the symbol-domain specification and any receiver stage that assumes periodicity
or `--++` beyond the listed sites; (2) the validation plan's falsifiers, order, decidability and n;
(3) scope (BPSK31/63 on uniformity, QPSK left out, anything missed on the daemon/ARDOP/fallback
path); (4) the claims quoted from the thread; (5) the four open questions; (6) does it block
Release 1, and is there a smaller change. Classify blocking / should-fix / park.

## Verdict

Nothing falsifies the sequence choice (symbol-domain m-sequence, N = 63, chip rate = baud). It does
not block Release 1 as scoped **if F1 passes**. A smaller change exists: PN-63 on BPSK250 only (and
BPSK100 if its columns pass), same break, same re-record, at the cost of two generators.

**Blocking**
1. F1's rule ("within the paired 95 % CI, n ≥ 200") could not fail and got easier with less data;
   at p ≈ 0.72 a 200-trial CI hides a 5 % loss. → non-inferiority, δ = 0.03, n = 600 paired. F1 also
   named `RsStrong` for SL5, which is `Rs`.

**Should-fix**
2. Onset placement reaches one rung by construction: PN-63 templates exceed the 2 048-sample passband
   budget except at BPSK250 (1 984), and the DDC arm returns no ranking. → stated; a gate pins
   BPSK250's template to the passband arm.
3. The slow-rung blocker was misquoted: the 2026-08-04 elimination failed on the margin (noise 0.426
   vs decodable 0.569), and the tone/grid failure is a second, independent blocker. PN removes only
   the second. → problem statement corrected; F5 uses the pre-registered rule.
4. Distinct per-mode sequences had no consumer: no two BPSK rungs share a chip rate. → one sequence
   (`Pn63`, x⁶ + x + 1).
5. Receive cost was unmeasured, and every acquisition window scales with `preamble_samples`. → F1c,
   with a pass rule against the 9 s ACK window.
6. The AFC window would double silently (8 s at BPSK31). → decoupled, named constant at 128 symbols.
7. QPSK's exclusion was asserted, not measured. → F0 decides it.
8. Missing `--++`-premised tests (`the_gate_is_not_fooled_by_a_steady_tone`, the restricted-lock
   rescue, the #1454 gather counts) and phantom sites (no drift-fit; the SNR lock runs over data;
   the engine's 32/33 are a no-geometry fallback). → listed and corrected.
9. F3 was payload-scoped. → a fixed set of 32 whitened payloads.

**Park:** the "#1052" revert number is wrong in the thread; the acceptance table is ~100 rows; the
Hann crossfade term is unchanged to first order; transition density 31/63 confirmed.

**Open questions:** symbol domain confirmed by `demod_parity`'s existing `symbols_to_bits`; `p[62]`
is unconstrained; the AFC window is a separate parameter; uniformity is a defensible reason, but the
design must say what happens if a slow rung's parity fails (that rung keeps `--++`).

**Strongest residual risk:** F1 passing on BPSK250 while the Pi receive cost at SL2 crosses the 9 s
ACK window. F1c now measures it.

## Consumer

The design's consumer: `BpskPlugin` modulate/demodulate and `preamble_template` →
`build_preamble_veto` → `ranked_fallback_onsets` / `acquire_at_onset`, reached from `server.rs`'s
`ota_decode_burst`. Found by `grep -rn "preamble_bits\|preamble_template\|build_preamble_veto" plugins/bpsk/src crates/openpulse-modem/src`.

## Prior art

`openpulse-dsp::preamble::Pn63` (in production in the pilot plugin), MFSK16's Costas sync,
`demod_parity.rs`, the f7–f13 probes, and the #1062 thread's revisions 2–4. Found by
`grep -rn "Pn63\|fn symbols_to_bits" crates plugins --include=*.rs`.

## Twins

The GPU and RRC modulator paths; QPSK (decided by F0); 8PSK and 64QAM (off `hpx_hf`, unchanged).
Found by `grep -rln "PREAMBLE_SYMS" plugins crates --include=*.rs`.
