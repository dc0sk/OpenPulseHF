---
project: openpulsehf
doc: docs/dev/design/ofdm-padding-whitening.md
status: implemented
last_updated: 2026-11-04
---

# The OFDM last symbol's padding is not whitened: an impulse that costs up to 3.5 dB and the SNR reading

Work plan M3; Release 1 `fast` profile (SL7–SL14). Found in the analysis of task 21, run f1
(`rx-b-f1`), on 2026-11-04.

## Problem — measured

**On air (run f1, B receiving, 93 OFDM frames classified by their length prefix and duration):**

| mode | frames | production SNR (median) | last symbol: pilot gain / EVM-SNR | pooled SNR without the last symbol |
|---|---|---|---|---|
| OFDM52 (SL7) | 3 | 6.8 dB | 0.88 / 4.5 dB | 6.8 dB |
| OFDM52-8PSK (SL8) | 28 | 14.5 dB | 0.94 / 15.4 dB | 14.5 dB |
| OFDM52-16QAM (SL9) | 47 | **6.8 dB** | **0.29 / −5.2 dB** | **15.5 dB** |
| OFDM52-32QAM (SL10) | 15 | 15.9 dB | 0.69 / 12.4 dB | 15.9 dB |

- **In a 16QAM frame, symbols 0–19 read 13.6–20.4 dB; symbol 20, the last, reads −7.8 dB.** Its
  pilots are at 0.08–0.38 of symbol 0's, and the per-subcarrier error has a structure across the
  band (frame at 368.67 s).
- **The production estimate pools the error energy of every symbol**, so the one broken symbol pulls a
  15.5 dB frame to 6.8 dB (median). This is the "low OFDM SNR" of the f1 review.
- **The frame is not cut short:** its energy ends 0–32 samples after the last FFT window's end, in
  every mode.
- **It is not the ALC compressing the frames:** at SL8 and above the amplitude tail is heavier than
  Gaussian (0.6–0.9 % of samples over 3σ, against 0.27 %), not cut off. CE-SSB acts on SL7 only
  (`cessb_benefits`), whose frames show its thin tail (peak/RMS 9.8 dB).

**The mechanism, in the modulator** (`plugins/ofdm/src/modulate.rs`, `modulate_with_params`):
1. The bit stream is the length prefix plus the payload, whitened (`crate::scramble`) so that no
   payload can put every subcarrier on the same constellation point. That point's IDFT is an
   impulse.
2. The last symbol's subcarriers beyond the stream get `sym_bits = 0` (`bit_idx < bits.len()`),
   **after** the whitening. They are not whitened.
3. So for a 520-byte frame (526 bytes with the 6-byte prefix), 40 of the last symbol's 52 data
   subcarriers on 16QAM and 42 on 32QAM carry the same point: the impulse the whitener exists to
   prevent.
4. **The modulator, measured** (`ofdm_modulate`, 520-byte random payloads, 40 seeds; the review's
   numbers, which correct a first table made with a 2-byte prefix):

   | mode | padded SCs | last symbol's peak over the others: min / median / max | power gained by the fix |
   |---|---|---|---|
   | 8PSK | 1–2 | — | — |
   | 16QAM | 40 | +1.7 / +3.6 / +5.6 dB | +1.6 / +3.5 / +5.4 dB |
   | 32QAM | 42 | median +0.9 dB | median +0.4 dB |
   | 64QAM | 27 | median +1.9 dB | median +1.6 dB |

   - The higher-order modes scale the frame to a 0.9 peak, so the whole 16QAM frame goes out a median
     3.5 dB below what its data symbols need.
   - QPSK (OFDM52) is clipped to the PAPR target at the transmitter, and that clip already costs the
     last symbol: at the modulator's output, pilot gain 0.89 and EVM-SNR 17 dB (60 dB with the fix);
     the on-air SL7 last-symbol gain of 0.88 is that clip. SL7's low reading on *every* symbol
     (about 7 dB at a 12 dB pilot SNR) is a separate matter, CE-SSB on SL7, not touched here.
5. **On air**, the impulse is a transient that a gain control answers inside the symbol, and the
   symbol arrives broken.
   - **The review's model reproduces it:** a gain loop with a 1–3 ms attack, a 100–300 ms release and
     a 2.5× over-reaction (in dB) gives last-symbol pilot gain ×0.33 (on air 0.29), EVM −5.1 to
     −6.3 dB (on air −5.2) and a pooled SNR of 6.9–8.1 dB (on air 6.8). With the fix, under the
     same model, the symbol is clean.
   - A hard clip or an instant AGC does not reproduce it.
   - The IC-9700 is in ALC through the run (median 0.617), so its ALC is the likelier actor than B's
     AGC. Which one does it is UNCHECKED on air.
   - B's software AGC is off (`config-b.toml`), and the recording is taken before the AGC seam.

**What it costs:**
- `climb_on_snr` reads a 4–7 dB SL9;
- SL9 transmits 3.5 dB under its peak budget, SL10 1.6 dB, SL13 1.1 dB.

The decode survives for 64-byte traffic: the broken symbol carries 48 data bits on 16QAM, inside
the FEC's margin. **For a frame whose last symbol is mostly data, more is lost.** That is UNCHECKED
on air.

## Decision — implemented: whiten the padding

Review: `docs/dev/reviews/review-ofdm-padding-whitening-2026-11-04.md`. It ran after the
implementation: this is a plugin waveform change that the receiver never reads, not a wire-format
change in the interop sense (`protocol-wire-spec.md` scopes plugin framing out of the byte layout).

Pad the bit stream to whole symbols **before** the whitening, so every padded subcarrier carries
keystream bits:
- `bits.resize(n_syms * bits_per_sym, false)`, then `scramble_bits`.
- **Interoperability:** the receiver descrambles the whole stream and cuts it at the length prefix
  (`frame_symbol_count`, `decode_len_prefix`, `decode_len_prefix_llrs`). It never reads a padding bit.
  - An old receiver decodes a new frame; a new receiver decodes an old one.
  - The core wire whitener (`openpulse-core/src/scramble.rs`, frozen in `protocol-wire-spec.md` §1a)
    is not touched.
- **Not changed:** the SNR estimator's pooling. A robust (median) estimate would hide a real
  impairment as well as this one; with the padding whitened, the pooled estimate measures the frame.

## Tests

- **A modulator property test**, `no_symbol_puts_most_subcarriers_on_one_point` (`ofdm-plugin`).
  - For every OFDM mode and every payload length up to three symbols, no data symbol puts more of
    its subcarriers on one point than 5σ over the binomial mean (at least a quarter of them).
  - **Why not a peak bound, as first proposed:** the old order's excess is payload-dependent
    (+1.7 to +5.6 dB), so a 2–3 dB bound would let the bug through for some payloads. And the new
    order's last symbol can legitimately sit +0.6 dB over the rest.
  - **Sabotage:** the old order fails it on every mode (14/16, 49/52, 50/52, 50/52, 51/52, 51/52).
    The new order's worst counts are 9–10/16 and 6–24/52.
- **The existing loopback and soft-LLR tests** must pass unchanged (the receiver ignores padding).

## Consumer

- `ModulationPlugin::modulate` for every OFDM mode (`plugins/ofdm/src/lib.rs`), i.e. every OFDM frame
  the engine transmits: SL7–SL14 on `hpx_hf`'s `fast` profile.
- Found with `grep -n "fn modulate_with_params" -A60 plugins/ofdm/src/modulate.rs`.

## Prior art

- **`plugins/ofdm/src/scramble.rs`:** the whitener, which exists for exactly this impulse; this
  extends its coverage to the padding.
- **DVB-T / 802.11:** the scrambler runs over the padded stream (802.11 scrambles the pad bits).
- **`docs/dev/design/ofdm-carrier-offset.md`:** the last OFDM change on this path.

## Twins

- **SC-FDMA** (`plugins/scfdma/src/modulate.rs`): pads with zeros and has no whitener at all.
  - A zero run through its DFT precoding is a single-subcarrier tone, not an impulse.
  - Not on `hpx_hf`; not Release 1.
  - Found with `grep -rln scramble plugins/*/src` (only `bpsk` and `ofdm`) and
    `grep -rn "bit_idx < bits.len()" plugins` (only `ofdm` and `scfdma`).
- **The PSK/FSK plugins:** single carrier; a run of identical symbols is a tone, not an impulse.
  Not affected.
