---
project: openpulsehf
doc: docs/backlog-fec-improvements.md
status: living
last_updated: 2026-05-09
---

# FEC Improvements Backlog

Research conducted 2026-05-09. Current state: RS(255,223) as primary, K=3 ConvCodec as
optional alternative; concatenated Conv+RS and short-block RS both shipped.

---

## BL-FEC-1 — Concatenated Conv+RS session mode ✅ Done (PR #169)

**Problem:** RS(255,223) fails completely at ≥1% random bit BER (accumulates ~18 byte errors/block, exceeding t=16). The block interleaver helps with burst errors but not AWGN.

**Solution:** Chain the two existing codecs serially.

```
TX: payload → RS(255,223) outer → ConvCodec(rate-1/2) inner → modulate
RX: demodulate → ConvCodec Viterbi decode → RS decode → payload
```

**What was delivered:**
- `transmit_with_concatenated_fec()` / `receive_with_concatenated_fec()` in `ModemEngine`
- `FecMode::Concatenated` (strength 3) in handshake negotiation
- Loopback tests, BER-injection test, overhead assertion (≥ 2× RS-only size)

---

## BL-FEC-2 — Increase RS ECC to t=32 for AWGN robustness (High priority)

**Problem:** RS(255,223) with t=16 corrects up to 16 byte errors per block. AWGN at 1% BER produces ~18 byte errors, which the standard codec cannot recover.

**Solution:** Add a `FecCodec::strong()` constructor using `FEC_ECC_LEN_STRONG = 64` ECC bytes.
New params: RS(255,191), t=32. Overhead increases from 14% to 25% (ECC bytes / block).

**Implementation:** New constant and constructor alongside the existing `FecCodec::new()`;
separate engine methods `transmit_with_strong_fec` / `receive_with_strong_fec`; new
`FecMode::RsStrong` variant (strength 5) for handshake negotiation.

**Wire compat:** Fully additive — existing sessions using `FecMode::Rs` are unaffected.

---

## BL-FEC-3 — Short-block RS for ACK/control frames ✅ Done (PR #170)

**Problem:** RS(255,223) bloats a 5-byte FSK4-ACK frame to 255 bytes, making FEC impractical for short control frames.

**What was delivered:**
- `ShortFecCodec` (no padding, no length prefix); 5-byte ACK → 13 bytes (5 + 8 ECC, t=4)
- `FecMode::ShortRs` (strength 4) in handshake negotiation
- `transmit_ack_with_short_fec` / `receive_ack_with_short_fec` in `ModemEngine`
- Corrects ≤4 byte errors per frame; validated with FSK4-ACK engine loopback

---

## BL-FEC-4 — Memory-ARQ soft combining (Medium priority)

**From docs/pactor-research.md.** Soft-combine signal samples from multiple NACK
retransmissions before decoding (maximal-ratio combining). Each retransmission of the same
frame is captured and the sample buffers are averaged element-wise before demodulation.
Reduces required SNR by ~3 dB per doubling of retransmissions (coherent averaging gain).

**Implementation:** `SoftCombiner` struct in `fec.rs` accumulates `Vec<f32>` sample
buffers and computes an element-wise mean on `combine()`. Engine method
`receive_with_soft_combining(mode, device, n_frames)` captures n_frames sample buffers,
combines them, then demodulates and RS-decodes the result.

No wire protocol change — the sender simply retransmits the same frame; the receiver
accumulates. Compatible with standard RS FEC and the strong RS(255,191) codec.

---

## BL-FEC-5 — Soft-decision K=7 Viterbi (Deferred — no crate)

**From docs/vara-research.md.** K=7 soft-decision Viterbi gives ~5 dB additional coding gain over the current hard-decision K=3 ConvCodec. No pure-Rust soft-decision K=7 implementation available on crates.io as of 2026-05-09. Revisit when a suitable crate ships or when bespoke implementation is warranted.

---

## BL-FEC-6 — Turbo / LDPC codes (Long-term, deferred)

**From docs/vara-research.md.** Turbo codes offer near-Shannon-limit performance and are used by VARA. Deferred because:
- No pure-Rust iterative BCJR/MAP crate available
- Variable decoder latency is incompatible with fixed ARQ cycle budgets
- LDPC requires long blocks (thousands of bits) and belief-propagation with unpredictable iteration count; applicable only with GPU acceleration

Revisit if GPU path (openpulse-gpu) matures sufficiently to absorb the iterative decode cost.
