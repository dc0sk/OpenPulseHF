---
project: openpulsehf
doc: docs/backlog-fec-improvements.md
status: living
last_updated: 2026-05-09
---

# FEC Improvements Backlog

Research conducted 2026-05-09. Current state: RS(255,223) as primary, K=3 ConvCodec as optional alternative. Neither is concatenated.

---

## BL-FEC-1 — Concatenated Conv+RS session mode (High priority)

**Problem:** RS(255,223) fails completely at ≥1% random bit BER (accumulates ~18 byte errors/block, exceeding t=16). The block interleaver helps with burst errors but not AWGN.

**Solution:** Chain the two existing codecs serially.

```
TX: payload → RS(255,223) outer → ConvCodec(rate-1/2) inner → modulate
RX: demodulate → ConvCodec Viterbi decode → RS decode → payload
```

The Conv inner layer reduces raw BER by ~10 dB; RS outer corrects residual Viterbi burst failures. At 2% raw channel BER, Conv reduces this to ~0.01% residual — well within RS t=16 capacity.

**Implementation:** `transmit_with_concatenated_fec()` / `receive_with_concatenated_fec()` in `crates/openpulse-modem/src/engine.rs` chaining `FecCodec` → `ConvCodec` on TX and `ConvCodec` → `FecCodec` on RX. Both codecs exist; this is ~50 lines of wiring + a new session negotiation flag in the HPX handshake.

**Overhead:** 2× (Conv rate-1/2) × 1.14 (RS) = 2.28× total; acceptable for robust/slow modes.

**Reference:** DVB-S uses the same architecture (RS outer + Conv inner).

---

## BL-FEC-2 — Increase RS ECC to t=32 for AWGN robustness (Medium priority)

**Problem:** RS(255,223) with t=16 corrects up to 16 byte errors per block. AWGN at 1% BER produces ~18 byte errors.

**Solution:** Change `FEC_ECC_LEN` constant from 32 to 64 in `crates/openpulse-core/src/fec.rs`.

New params: RS(255, 191), t=32. Overhead increases from 14% to 25% (ECC bytes / block).

**Cost:** No code changes beyond the constant and updating tests. Breaks wire compatibility with existing sessions using the smaller block — requires a session negotiation flag.

---

## BL-FEC-3 — Short-block RS for ACK/control frames (Low priority)

**Problem:** RS(255,223) bloats a 5-byte FSK4-ACK frame to 255 bytes, making FEC impractical for short control frames.

**Solution:** Add `ShortFecCodec` using RS(63,55) [t=4] or RS(31,25) [t=3] from the `reed-solomon-erasure` crate (supports configurable GF field sizes, unlike the current `reed-solomon` v0.2 which is hardcoded to GF(2^8)/n=255).

**Impact:** Unlocks FEC for ACK frames, small beacons, and control payloads ≤55 bytes.

**Dependency change:** Replace or supplement `reed-solomon = "0.2"` with `reed-solomon-erasure`.

---

## BL-FEC-4 — Memory-ARQ soft combining (Mid-term, from pactor-research.md)

**From docs/pactor-research.md.** Soft-combine signal samples from multiple NACK retransmissions before decoding (maximal-ratio combining). Reduces required SNR by ~3 dB per doubling of retransmissions.

No wire protocol change needed — only receiver buffering. Compatible with current RS FEC.

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
