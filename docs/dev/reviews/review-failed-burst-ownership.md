---
project: openpulsehf
doc: docs/dev/reviews/review-failed-burst-ownership.md
status: resolved
last_updated: 2026-10-11
---

# Review: a failed burst is ladder evidence only if it looks like our frame (#1460), round 1

This is the adversarial review (Fable) of [`design/failed-burst-ownership.md`](../design/failed-burst-ownership.md)
as first drafted on 2026-10-11. It was read-only: no build, no file changes, no state-changing git.
The review is mandatory, because the design changes when a NACK is keyed (decision 8).

## Prompt

Falsify, do not agree. Read the design, its predecessor `reply-window-evidence.md` v3 and its review,
the not-evidence guards in `ota_decode_and_ack_inner`, `decode_variants`, `stage_demodulate_variants`,
`FecCodec::encode`, `scramble.rs`, `rx_candidates`, the daemon's NACK keying and `nack_budget.rs`. The
questions:

1. Is the header premise true on the wire for every SL1–SL6 frame?
2. Does every failed RS attempt pass through `decode_variants`?
3. Is the minimum over every wire still a discriminator? Is a foreign over really uncorrelated with
   the header?
4. Is M1's 99 % rule right, and is M2's signal set complete?
5. How does it interact with the existing guards, the budget, HARQ, and the ISS when our own frame is
   refused?
6. Is there a simpler or stronger discriminator?
7. Does this block Release 1?

## Verdict

**It does not block Release 1.** #1460's remainder is HF narrowband QRM, and A1/A2 run on 2 m. That is
the same conclusion as the three reply-window rounds. **Do not implement as written.** Landing it
before Release 1 adds a 2 m downside (finding 4) with no 2 m upside.

## Findings

1. **Blocking: the false-match number is wrong, and the minimum over every wire multiplies it.**
   - Σ_{k≤16} C(56,k)/2⁵⁶ = 9.2 × 10⁻⁴, not 2 × 10⁻⁴. D = 12 gives 1.0 × 10⁻⁵ and D = 8 gives
     2.3 × 10⁻⁸.
   - A burst at a BPSK rung is tried at roughly 1 000–3 000 wires: offset 0, the phase-1 scan (≤ 2
     passes × ~128–160 onsets × ≤ 4 arms), phase 2, and ≤ 2 candidates.
   - P(min ≤ D) over that many wires is 0.60 (D = 16, N = 1 000), 0.94 (N = 3 000), 0.010 (D = 12) and
     2 × 10⁻⁵ (D = 8).
   - Our own frames at the floor − 4 dB on a fade have a raw BER of about 0.15–0.25, which gives
     P(distance > 8) of 0.47–0.96. The feasible band for D is very likely empty.
   - Fix: record the distance only at the few wires the engine already ranks, about 4–8 of them.
2. **Blocking: a foreign over is not random bits.**
   - BPSK and QPSK250-D are differential and MFSK16 is argmax-tone, so a steady tone gives a constant
     bit stream. After descrambling, that is the keystream or its complement.
   - Distance to the 56 header bits:
     - all zeros (a tone at fc): 39;
     - all ones (a tone at fc ± (2k+1)·baud/2): **17**;
     - the best constant byte (0xF7): 16;
     - the best periodic pattern of period ≤ 8: 14;
     - MFSK16 tone 15: 17.
   - M2's signal set misses this. It must sweep a single tone in steps under baud/8 at every rung, and
     D must stay at about 10 or below.
3. **Blocking: SL1 is in the rule but outside M1.** MFSK16 returns an acquisition error before any wire
   reaches `decode_variants`, so every SL1 acquisition failure would be refused. Today each one counts,
   keys a NACK and retains LLRs for HARQ, worth about 2.5 dB at that rung. Exclude SL1 or measure it.
4. **Blocking: a refused frame of ours gives the ISS silence, which is worse than a NACK.**
   - A NACK retransmits up to three times. Two silent windows abandon the message, with no step-down.
   - Frames that lose their header and that M1 cannot see:
     - fade-split pieces without the frame start;
     - a plugin timing lock one symbol off, which descrambles every byte with a shifted keystream;
     - an off-frequency first frame whose phase-2 settle returns nothing.
   - M1 must report the fraction of failing *frames* that key at least one NACK, including those
     regimes and ±50 Hz.
5. **Should fix: the template was the stronger discriminator, and work plan item 2 asked for it.**
   - It refuses every lone tone (worst ρ 0.248 against 0.315), the exact class that defeats the header.
   - PN-63 is transmitted on BPSK63 and BPSK100 too, and the plugin documents how to derive their
     constants.
   - The right shape: the template where one is published, the header or nothing elsewhere.
6. **Should fix: simpler discriminators were missed.**
   - **Known zero padding.** A 64 B payload leaves about 1 160 known zero bits in block 0 after
     descrambling. A constant foreign stream descrambles to 50 % ones there.
   - **A soft header correlation** on the LLRs.
   - **The free consistency check:** prefix byte 3 = frame length byte + 10.
7. **Should fix: coverage holes at the seam.**
   - Plugin errors before the seam: MFSK16's gate, and BPSK "signal too short" or "no data symbols".
   - Guard the 9-byte slice on short wires.
   - Make the reset unconditional, with a tripwire counter.
   - `decode_combined_llrs` runs outside the seam, but only after the hard arms, so it adds no loss.
8. **Should fix: M2 sample size.** A 1 % rule needs about 300 or more counted bursts per cell. Report
   the binomial upper bound, add the tone sweep, and add RTTY with one tone at baud/2.
9. **Park:**
   - other OpenPulseHF stations still count;
   - rule (a) at the SL6/SL7 boundary;
   - M3's cost is negligible.

**Checked and fine:**
- The header premise on the wire: RS is systematic with data first, and prefix bytes 0–1 are 0 for
  every ladder frame.
- `RsStrong` uses the same layout.
- The descramble happens before the seam.
- MFSK16 and QPSK byte packing.
- Multi-block `decode_prefix`.
- The budget and HARQ interaction: a refused burst leaves the budget alone and retains nothing.
- The guard ordering.

## Disposition

Not implemented. Findings 1–4 make the header-only rule unsafe as drafted. A round-2 design rebases on:
- template corroboration where one is published;
- a known-padding or soft-header test at the few ranked wires;
- SL1 excluded or measured;
- M1 extended to fade-split pieces, a lock off by a symbol, and ±50 Hz, with a per-frame "at least one
  NACK" rule.

Since it does not block Release 1, where it lands (M2 under decision 21, or after M3) is the
maintainer's call.
