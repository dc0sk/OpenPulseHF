---
project: openpulsehf
doc: docs/dev/reviews/review-ofdm-padding-whitening-2026-11-04.md
status: resolved
last_updated: 2026-11-04
---

# Review: whiten the OFDM last symbol's padding (2026-11-04)

Adversarial review (Fable) of `docs/dev/design/ofdm-padding-whitening.md` and `3fe112e`. It ran
after the implementation. The change is a plugin waveform change that the receiver never reads, so
it is not a wire-format change in the interop sense (`protocol-wire-spec.md` scopes plugin framing
out of the byte layout).

## Prompt

The prompt asked for falsification, not agreement:
- the mechanism, against other causes of a broken last symbol;
- interop on every receive path;
- whether the test tests the right thing;
- twins;
- the 15–17 dB pilot-SNR floor.

It also asked "Does this block Release 1?".

## Verdict

**It does not block Release 1;** it serves M3. The mechanism holds, the fix is correct and
interop-safe both ways, and the committed test is the right shape. The numbers in the design note
were wrong.

## Findings and what was done

| # | Finding | Done |
|---|---|---|
| S1 | The "padded SCs" table was made with a 2-byte prefix; with the product's 6 bytes it is 8PSK 1–2, 16QAM 40, 32QAM 42, 64QAM 27. "+3.5 dB" is one payload's value; over 40 seeds the excess is +1.7 / +3.6 / +5.6 dB (min / median / max) on 16QAM | **Taken:** corrected in the design note, the task file, the ledger and the code comments (up to 51 of 52 across lengths). The commit message of `3fe112e` keeps the first numbers; this record corrects them |
| M | Reproduced: only a gain loop with a finite attack (1–3 ms, release 100–300 ms, 2.5× over-reaction) gives the air's numbers (pilot ×0.33, EVM −5.1 to −6.3 dB, pooled 6.9–8.1 dB); the fix makes the symbol clean under the same model. Ruled out: the analysis's timing, ID/ACK riding on the frame, B's software AGC (off), the TX limiter (off), CE-SSB (off above QPSK). The IC-9700's ALC is the likelier actor | Recorded in the design note; the actor stays UNCHECKED on air |
| Q | On OFDM52 the transmitter's own clip already costs the last symbol (pilot gain 0.89, EVM 17 dB); the on-air SL7 0.88 is that clip | Recorded; SL7's low reading on every symbol is CE-SSB, a separate matter |
| I | Interop both ways: 652 frames (six modes, many lengths, both orders) decode hard and soft to the payload. No golden waveform test exists; no v0.17 declaration change is required | Recorded |
| T | The label-count test fails under sabotage on every mode and is better than the proposed peak bound, which would let some payloads through | **Taken:** the design note's Tests section now describes it and why |
| N1 | The 30 dB cap in `ofdm_carrier_offset_ota` lost sensitivity above 27 dB | **Taken:** a fixed 35 dB floor above the cap (readings 37.7–39.7 dB) |
| N2 | `bits.resize(n_syms * bits_per_sym)` drops the stream if `bits_per_sym` were 0 (unreachable) | **Taken:** `bits_per_sym.max(1)` |
| N3 | The broken symbol carries 48 data bits on 16QAM, not 16 | **Taken** |
| Tw | SC-FDMA pads with zeros and has no whitener; a zero run through DFT precoding is a tone, not an impulse. Not on `hpx_hf` | **Taken** into Twins |
| F | The 15–17 dB floor: phase noise is not plausible (it would need −15 dBc integrated); the IC-9700's ALC regulating every frame, plus PA IMD, is. Falsifier: lower A's drive until the ALC reads zero | Added to task 22 Part B's report |
