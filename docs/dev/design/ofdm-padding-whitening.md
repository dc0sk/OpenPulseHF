---
project: openpulsehf
doc: docs/dev/design/ofdm-padding-whitening.md
status: proposed
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
3. So for a 520-byte frame, 48 of the last symbol's 52 data subcarriers on 16QAM and ~49 on 32QAM carry
   the same point: the impulse the whitener exists to prevent.
4. **The modulator, measured:**

   | mode | padded SCs | frame peak over the rest of the frame |
   |---|---|---|
   | 8PSK | 12 | +0.0 dB |
   | 16QAM | 48 | +3.5 dB |
   | 32QAM | 49 | +1.6 dB |
   | 64QAM | 32 | +1.1 dB |

   - The higher-order modes scale the frame to a 0.9 peak, so the whole 16QAM frame goes out 3.5 dB
     below what its data symbols need.
   - QPSK (OFDM52) is clipped to the PAPR target, so its impulse is clipped away.
5. **On air**, the impulse is a transient that the transmitter's ALC and the receiver's AGC answer
   inside the symbol. Which one does it is UNCHECKED. The symbol arrives broken.

**What it costs:**
- `climb_on_snr` reads a 4–7 dB SL9;
- SL9 transmits 3.5 dB under its peak budget, SL10 1.6 dB, SL13 1.1 dB.

The decode survives for 64-byte traffic, because the broken symbol carries only 16 real bits.
**For a frame whose last symbol is mostly data, those bits are lost.** That is UNCHECKED on air.

## Decision — proposed: whiten the padding

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

- **A modulator property test** (`ofdm-plugin`): for every OFDM mode and a range of payload lengths
  (one byte to a full symbol of padding), no data symbol's peak exceeds the others' by more than a
  bound.
  - Sabotage: whitening after the padding, as now, fails it.
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

- **SC-FDMA and the other non-`hpx_hf` multicarrier plugins:** they may pad the same way.
  UNCHECKED; not Release 1.
- **The PSK/FSK plugins:** single carrier; a run of identical symbols is a tone, not an impulse.
  Not affected.
