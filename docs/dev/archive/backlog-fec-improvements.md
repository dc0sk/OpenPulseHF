---
project: openpulsehf
doc: docs/backlog-fec-improvements.md
status: living
last_updated: 2026-05-09
---

# FEC Improvements Backlog

**Frozen research summary (2026-05-09).** Archived; see docs/dev/steering/backlog.md for live open work.

Research conducted 2026-05-09. Current state: RS(255,223) as primary, K=3 ConvCodec as
optional alternative; concatenated Conv+RS, short-block RS, strong RS(255,191), and
Memory-ARQ soft combining all shipped.

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

## BL-FEC-2 — Increase RS ECC to t=32 for AWGN robustness ✅ Done (PR #171)

**Problem:** RS(255,223) with t=16 corrects up to 16 byte errors per block. AWGN at 1% BER produces ~18 byte errors, which the standard codec cannot recover.

**What was delivered:**
- `FecCodec::strong()` — RS(255,191) with 64 ECC bytes per block (t=32); corrects up to 32 byte errors vs. 16 for standard RS; 25% overhead vs. 14%
- `FecMode::RsStrong` (strength 5, highest) in handshake negotiation
- `transmit_with_strong_fec` / `receive_with_strong_fec` in `ModemEngine`
- `FecCodec` refactored with `ecc_len` field so both `new()` and `strong()` share the same encode/decode path
- Loopback tests, 32-byte error correction test, behavioral comparison test proving strong corrects where standard fails

---

## BL-FEC-3 — Short-block RS for ACK/control frames ✅ Done (PR #170)

**Problem:** RS(255,223) bloats a 5-byte FSK4-ACK frame to 255 bytes, making FEC impractical for short control frames.

**What was delivered:**
- `ShortFecCodec` (no padding, no length prefix); 5-byte ACK → 13 bytes (5 + 8 ECC, t=4)
- `FecMode::ShortRs` (strength 4) in handshake negotiation
- `transmit_ack_with_short_fec` / `receive_ack_with_short_fec` in `ModemEngine`
- Corrects ≤4 byte errors per frame; validated with FSK4-ACK engine loopback

---

## BL-FEC-4 — Memory-ARQ soft combining ✅ Done (PR #171)

**From docs/pactor-research.md.** Soft-combine signal samples from multiple NACK
retransmissions before decoding (maximal-ratio combining). Each retransmission of the same
frame is captured and the sample buffers are averaged element-wise before demodulation.
Reduces required SNR by ~3 dB per doubling of retransmissions (coherent averaging gain).

**What was delivered:**
- `SoftCombiner` struct in `fec.rs` — accumulates `Vec<f32>` sample buffers; `combine()` returns element-wise mean; `count()` and `reset()`
- `receive_with_soft_combining(mode, device, n_frames)` engine method — captures n_frames sample buffers, combines, demodulates, RS-decodes
- No wire protocol change — sender retransmits the same frame; receiver accumulates
- Decodes using the standard RS codec (t=16); pair with `transmit_with_fec` on the sender side

---

## BL-FEC-5 — Soft-decision K=7 Viterbi ✅ Done (PR #177)

**From docs/vara-research.md.** K=7 soft-decision Viterbi gives ~5 dB additional coding gain over the current hard-decision K=3 ConvCodec. Implemented in `crates/openpulse-core/src/soft_viterbi.rs` — no external crate needed.

Generators: G0=0o133, G1=0o171 (NASA/3GPP standard).  64 states.  Branch metric = sum of `llr * (1 - 2*encoded_bit)`.

**What was delivered:**
- `SoftViterbiCodec` in `soft_viterbi.rs` — K=7 rate-1/2 encoder + soft-decision Viterbi decoder; `decode_soft()` returns `Result<Vec<u8>, ModemError>`; traceback from state 0 (flush-terminated trellis)
- `ModulationPlugin::demodulate_soft()` default method in `plugin.rs` — hard ±1.0 fallback; BPSK overrides with real differential dot products; QPSK overrides with I/Q projections; 8PSK uses hard fallback + TODO for max-log-MAP (TODO since **resolved** — max-log-MAP `demodulate_soft()` shipped in PR #187–#192, `plugins/psk8/src/demodulate.rs:162`)
- `FecMode::SoftConcatenated` (strength 6): K=7 soft Conv inner + RS(255,223) outer
- `transmit_with_soft_viterbi_fec` / `receive_with_soft_viterbi_fec` in `ModemEngine`
- Integration tests: round-trip, 5% BER soft vs hard comparison, coding gain over uncoded, soft vs hard at low SNR

---

## BL-FEC-6 — Turbo / LDPC codes (Deferred — GPU required)

**From docs/vara-research.md.** Turbo codes offer near-Shannon-limit performance and are used by VARA. Deferred because:
- Variable decoder latency is incompatible with fixed ARQ cycle budgets
- LDPC requires long blocks (≥ 1 kbit) and belief-propagation with unpredictable iteration count; viable only with GPU acceleration

**Architecture decision (2026-05-09):** `IterativeDecoder` trait defined in `crates/openpulse-core/src/ldpc.rs`:
```rust
pub trait IterativeDecoder: Send + Sync {
    fn encode(&self, data: &[u8]) -> Vec<u8>;
    fn decode_soft(&self, llrs: &[f32]) -> Result<Vec<u8>, ModemError>;
    fn max_iterations(&self) -> u32;
    fn block_bits(&self) -> usize;
}
```
`LdpcCodec` stub compiles and satisfies the trait; returns `ModemError` on decode.  `FecMode::Ldpc` (strength 7) reserved in the enum.

**GPU path**: LDPC belief-propagation ≥ 50 iterations over a sparse parity-check matrix — each iteration is a vectorised message-passing pass suited to wgpu compute shaders.  Revisit when `crates/openpulse-gpu` matures sufficiently.
