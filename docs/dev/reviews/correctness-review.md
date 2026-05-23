---
doc: docs/dev/reviews/correctness-review.md
date: 2026-05-22
status: resolved
resolved: 2026-05-23
---

# Correctness Review

## Summary

LLR sign convention is consistent across all codecs (positive = likely 0). Bit ordering
is LSB-first throughout. All `FecMode` variants are handled in the dispatch table.
OFDM Hermitian symmetry Q=0 claim is correct. One minor error-suppression gap found in
the QPSK demodulator path.

---

## Findings

### COR-01 — LLR sign convention: consistent · Severity: Pass

All codecs document and implement **positive LLR = bit more likely 0**:

| Component | File | Evidence |
|---|---|---|
| `TurboCodec` | `crates/openpulse-core/src/turbo.rs:12` | Module-level doc: *"positive = likely 0"* |
| `LdpcCodec` | `crates/openpulse-core/src/ldpc.rs:19` | `decode_soft` doc: *"positive = likely 0"* |
| `SoftViterbi` | `crates/openpulse-core/src/soft_viterbi.rs:16` | *"Positive = bit more likely 0"* |
| BPSK `demodulate_soft` | `plugins/bpsk/src/demodulate.rs:168,172` | *"positive = bit more likely 0"* |
| QPSK `demodulate_soft` | (inherited convention) | No explicit comment — see COR-03 |
| 8PSK `demodulate_soft` | `plugins/psk8/src/demodulate.rs:79,167` | *"Positive LLR means bit=0"* |
| 64QAM `demodulate_soft` | `plugins/64qam/src/demodulate.rs:364` | *"Positive LLR → bit more likely 0"* |
| SC-FDMA `demodulate_soft` | `plugins/scfdma/src/demodulate.rs:48` | *"positive => likely 0"* |

---

### COR-02 — Bit ordering: LSB-first throughout · Severity: Pass

`bytes_to_bits` implementations in:

- `crates/openpulse-core/src/turbo.rs:92–97` — `(0..8).map(|i| (b >> i) & 1)` — LSB-first ✓
- `plugins/bpsk/src/modulate.rs:153–160` — `for shift in 0..8u8 { (b >> shift) & 1 }` — LSB-first ✓
- `plugins/ofdm/src/modulate.rs:100–107` — `for shift in 0..8u8 { (b >> shift) & 1 }` — LSB-first ✓
- SC-FDMA uses the same `bytes_to_bits` from OFDM via shared helper — LSB-first ✓

No MSB-first outlier found.

---

### COR-03 — QPSK `demodulate_soft` missing sign-convention doc comment · Severity: Low

**File:** `plugins/qpsk/src/demodulate.rs`

The QPSK soft demodulator does not carry an explicit `/// positive = likely 0` doc
comment. The convention is correct (the implementation mirrors BPSK's), but a future
maintainer could introduce a sign flip without a visible contract to check against.

**Recommendation:** Add `/// Returns LLRs with positive = bit more likely 0.` to the
`demodulate_soft` function doc in `plugins/qpsk/src/demodulate.rs`.

---

### COR-04 — All `FecMode` variants handled in dispatch · Severity: Pass

`FecMode` has 9 variants: `None`, `Rs`, `RsInterleaved`, `Concatenated`, `ShortRs`,
`RsStrong`, `SoftConcatenated`, `Ldpc`, `Turbo`.

`fec_mode_dispatch.rs` tests cover all 8 dispatchable variants. `ShortRs` is
intentionally rejected by `transmit_with_fec_mode` / `receive_with_fec_mode` (tested by
`dispatch_short_rs_returns_err`) — correct per the short-block-only contract.

---

### COR-05 — OFDM Hermitian symmetry Q=0 claim is correct · Severity: Pass

**File:** `plugins/ofdm/src/modulate.rs:49–79`

Every data subcarrier is assigned `freq[sc] = sym` and `freq[FFT_SIZE - sc] = sym.conj()`.
DC (`freq[0]`) and Nyquist (`freq[FFT_SIZE/2]`) remain zero. The IFFT of a Hermitian-
symmetric input is a purely real sequence, confirming that Q = 0. The `ofdm_modulate_iq`
function correctly exploits this by emitting `[real_sample, 0.0]` pairs without a
separate complex IFFT computation.

---

### COR-06 — `sar.rs:164` invariant `expect` is sound · Severity: Pass

**File:** `crates/openpulse-core/src/sar.rs:164`

```rust
.expect("received == total guarantees all present")
```

This `expect` is reached only after the guard `if self.received == self.total` passes,
meaning all fragment slots have been filled. The `Option::unwrap` on a fragment slot
is therefore guaranteed to succeed. This is a provable invariant, not a panic hazard.

---

## Action Items

| ID | Severity | Action | Resolution |
|---|---|---|---|
| COR-03 | Low | Add `/// positive = bit more likely 0` doc to `qpsk/src/demodulate.rs::demodulate_soft` | ✅ Full LLR sign convention doc block added to `qpsk_demodulate_soft` |
