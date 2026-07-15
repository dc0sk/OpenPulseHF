---
doc: docs/dev/reviews/2026-07-15-rx-decode-audit.md
date: 2026-07-15
status: resolved
scope: RX decode path — frame/SAR/FEC/conv/LDPC in openpulse-core, modem-engine receive, HPX/ACK/rate
---

# RX decode-path audit (untrusted RF input)

Two-finder adversarial sweep (refute-by-default, source-verified — both finders traced the pinned
`reed-solomon 0.2.1` internals byte-for-byte) of the code that turns untrusted, RF-derived bytes into
frames. A panic or unbounded allocation here is a **remote DoS**: any station on-air can be crashed by a
crafted transmission. Four defects fixed; the frame/LDPC/compression/ACK/rate/HPX paths refuted.

## Fixed

### RX-1 — [CRITICAL] `ShortFecCodec::decode` panics on input ≥ 256 bytes (two finders converged)

**File:** `crates/openpulse-core/src/fec.rs` (`ShortFecCodec::decode`)

`decode` checked only the *lower* bound (`encoded.len() > ecc_len`), never the upper. The pinned
`reed-solomon 0.2.1` decoder is backed by a fixed `[u8; 256]` polynomial and **panics** (an ordinary
out-of-bounds slice, present in every build profile) on any input ≥ 256 bytes. This decodes
attacker-length-controlled demodulator output on two receive paths — `decode_fsk4_ack` (called every
OTA ACK-listen tick on the raw, unwindowed capture) and `receive_with_short_fec_data` — where plugin
demod output scales with capture length and `AudioInputStream::read()` returns all buffered samples with
no cap. A peer that transmits ≥ ~256 wire bytes' worth of samples crashes the receive call.

**Fix:** reject `encoded.len() > 255` (a valid RS block is ≤ 255 bytes) before the decoder, mirroring the
guard `FecCodec::decode` already applies by chunking to exactly 255. Test:
`short_fec_decode_rejects_oversized_input_without_panicking`.

### RX-2 — [HIGH] `FecCodec::decode` length-prefix add overflows on 32-bit/wasm

**File:** `crates/openpulse-core/src/fec.rs` (`FecCodec::decode`)

The 4-byte length prefix is systematic (attacker-controlled). `PREFIX_LEN + orig_len` was an unchecked
`usize` add; on a 32-bit/wasm `usize` (wasm32 is a real build target) a prefix near `u32::MAX` wraps
`end` small, the `decoded.len() < end` guard passes, and `decoded[PREFIX_LEN..end]` panics.

**Fix:** `checked_add`, treating a wrapped/oversized `end` as too-short.

### RX-3 — [HIGH] `ConvCodec`/`SoftViterbiCodec` length-prefix multiply overflows on 32-bit/wasm

**Files:** `crates/openpulse-core/src/conv.rs`, `crates/openpulse-core/src/soft_viterbi.rs`

Same class, easier to hit: `32 + orig_len * 8` (unchecked multiply) overflows a 32-bit `usize` for any
`orig_len ≥ 2^29`, wrapping the length check and panicking the slice. Reachable on the RX path
(`FecMode::Concatenated` / `SoftConcatenated`).

**Fix:** `checked_mul(8).and_then(checked_add(32))`, treating overflow/oversize as too-short.

### RX-4 — [MEDIUM] `SarReassembler` had no pending-slot cap

**File:** `crates/openpulse-core/src/sar.rs`

Completed segments are removed immediately, but *incomplete* ones accumulated with no explicit cap —
bounded only by the incidental `u16` segment-id key space (~4 GB worst case) before the caller-driven
`expire()` runs. A sender flooding distinct, one-fragment-short segment ids grows memory.

**Fix:** `MAX_PENDING_SLOTS = 4096` — a new incomplete slot is rejected with
`SarError::TooManyPendingSegments` once the table is full (existing slots still accept fragments and
complete). Far above any legitimate in-flight transfer. Test: `ingest_caps_pending_incomplete_segments`.

## Refuted (no finding)

- **`Frame::decode`** — all length fields are `u8`-bounded (max 255), every slice bounds-checked, CRC
  verified before trusting the payload. Safe on all word sizes.
- **`SarReassembler` fragment logic** — `fragment_index >= fragment_total` rejected before indexing; a
  `fragment_total` mismatch errors without mutating the existing slot; reassembly fires only when
  `received == total`, so by pigeonhole no gaps.
- **`compression::decompress` (LZ4/Zstd)** — the claimed-size prefix is checked against
  `MAX_DECOMPRESSED_SIZE` before calling the library; `unpack()` never panics on malformed input.
- **`FecCodec` per-block RS call** — blocks are always exactly 255 bytes, so the RX-1 fixed-buffer panic
  doesn't apply; only the length-prefix arithmetic (RX-2) was a problem.
- **`ldpc.rs`** — block size is a fixed construction constant, not attacker-derived; `llrs.len() < n`
  rejected before indexing; no attacker-controlled length drives allocation.
- **`AckFrame::decode`** — fixed 5-byte input, CRC-8 checked first, every field validated via
  `from_u8`/`from_u8` that reject out-of-range codes.
- **`RateAdapter::apply_ack`** — exhaustive match, clamped at SL1/SL20, `saturating_add`; a crafted ACK
  flood only moves the level within `[SL1, SL20]`.
- **`HpxReactor`/`HpxSession`** — exhaustive-match transitions returning `Err(InvalidTransition)`; not
  wired to attacker-controlled decoded-frame bytes.
- **Burst / spectrum-tap buffers** — `rx_burst` capped at `BURST_MAX_SAMPLES`; `last_audio` at
  `SPECTRUM_TAP_MAX`, cleared each call.
