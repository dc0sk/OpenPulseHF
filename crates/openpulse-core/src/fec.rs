//! Reed-Solomon forward error correction (FEC) codec.
//!
//! Provides transparent byte-level error correction that can be layered on top
//! of any modulation plugin.  The codec splits input data into 223-byte blocks,
//! appends 32 ECC bytes per block (GF(2^8), ECC_LEN = 32), and can correct up
//! to **16 byte errors per block** on the receive side.
//!
//! # Wire layout
//!
//! ```text
//! ┌─────────────────────┬──────────────────────────────────────────────────┐
//! │ original length (4B)│  data (padded to BLOCK_DATA_STANDARD multiples)   │
//! └─────────────────────┴──────────────────────────────────────────────────┘
//!       ↓  split into 223-byte chunks, RS-encode each
//! ┌──────────────┬──────────────────────────────────────────────────────────┐
//! │ block 0 (255B) │ block 1 (255B) │ … │ block N (255B)                  │
//! └──────────────┴──────────────────────────────────────────────────────────┘
//! ```

use reed_solomon::{Decoder, Encoder};
use serde::{Deserialize, Serialize};

use crate::error::ModemError;

/// Which FEC scheme is applied above the modulation layer.
///
/// Used in the HPX handshake to negotiate FEC between two stations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FecMode {
    /// No FEC — raw transmit/receive.
    #[default]
    None,
    /// Reed-Solomon RS(255,223) only.
    Rs,
    /// Reed-Solomon with stride interleaver (burst-error dispersion).
    RsInterleaved,
    /// Concatenated: Conv(rate-1/2) inner + RS outer (DVB-S architecture).
    ///
    /// Conv layer reduces random-noise BER; RS corrects residual Viterbi
    /// burst failures.  Total overhead ≈ 2.28× raw payload.
    Concatenated,
    /// Short-block RS for ACK and control frames (no padding, no length prefix).
    ///
    /// Encodes a payload of up to 247 bytes into `payload.len() + 8` bytes
    /// (8 ECC bytes, t=4, corrects up to 4 byte errors).  A 5-byte FSK4-ACK
    /// frame becomes 13 bytes — versus 255 bytes with the standard RS codec.
    ShortRs,
    /// Strong RS: RS(255,191) with t=32 (64 ECC bytes per block).
    ///
    /// Corrects up to 32 byte errors per block — double the standard t=16
    /// capacity.  Use on AWGN-dominant paths where ≥ 1% raw BER is expected.
    /// Overhead: 25% ECC bytes per block vs. 14% for the standard codec.
    RsStrong,
    /// Soft-decision concatenated: K=7 Conv (soft Viterbi) inner + RS(255,223) outer.
    ///
    /// The inner K=7 decoder receives true LLRs from the demodulator instead of
    /// hard bits, gaining ~5 dB over the hard-decision `Concatenated` mode.
    /// The RS outer code corrects residual Viterbi burst failures.
    /// Overhead: ≈ 2.28× raw payload (same as `Concatenated`).
    SoftConcatenated,
    /// Rate-1/2 LDPC (k=1024, n=2048) via min-sum belief propagation.
    ///
    /// CPU implementation lives in `openpulse_core::ldpc::LdpcCodec`.
    /// A GPU-accelerated path via `openpulse-gpu` is reserved for future work.
    Ldpc,
}

impl FecMode {
    /// Numeric strength for negotiation; higher = stronger / preferred.
    pub fn strength(self) -> u8 {
        match self {
            FecMode::None => 0,
            FecMode::Rs => 1,
            FecMode::RsInterleaved => 2,
            FecMode::Concatenated => 3,
            FecMode::ShortRs => 4,
            FecMode::RsStrong => 5,
            FecMode::SoftConcatenated => 6,
            FecMode::Ldpc => 7,
        }
    }

    /// Select the strongest mode that appears in both slices.
    /// Returns `FecMode::None` if there is no overlap between `offered` and `accepted`.
    pub fn negotiate(offered: &[FecMode], accepted: &[FecMode]) -> FecMode {
        offered
            .iter()
            .filter(|m| accepted.contains(m))
            .copied()
            .max_by_key(|m| m.strength())
            .unwrap_or(FecMode::None)
    }
}

/// ECC bytes appended per 255-byte RS block (standard codec, t=16).
pub const FEC_ECC_LEN: usize = 32;

/// ECC bytes appended per 255-byte RS block by the strong codec (t=32).
pub const FEC_ECC_LEN_STRONG: usize = 64;

/// Total bytes per encoded RS block (always 255 — GF(2^8) block size).
const BLOCK_TOTAL: usize = 255;

/// Byte width of the big-endian original-length prefix.
const PREFIX_LEN: usize = 4;

/// Data bytes per block for the standard codec (255 − 32 = 223).
///
/// Used by unit tests that hard-code exact block boundaries.
pub const BLOCK_DATA_STANDARD: usize = BLOCK_TOTAL - FEC_ECC_LEN;

/// Reed-Solomon codec.
///
/// Construct with [`FecCodec::new`] (t=16, standard) or [`FecCodec::strong`]
/// (t=32, AWGN-robust).  The same codec instance can be reused for multiple
/// encode/decode operations.
pub struct FecCodec {
    ecc_len: usize,
    encoder: Encoder,
    decoder: Decoder,
}

impl FecCodec {
    /// Create a codec with `ECC_LEN = 32` (RS(255,223), t=16, corrects up to
    /// 16 byte errors per 255-byte block).
    pub fn new() -> Self {
        Self::with_ecc_len(FEC_ECC_LEN)
    }

    /// Create a codec with `ECC_LEN = 64` (RS(255,191), t=32, corrects up to
    /// 32 byte errors per 255-byte block).  Use when AWGN produces more than
    /// ~16 byte errors per block (≥ 1% raw BER on a 255-byte block).
    pub fn strong() -> Self {
        Self::with_ecc_len(FEC_ECC_LEN_STRONG)
    }

    fn with_ecc_len(ecc_len: usize) -> Self {
        Self {
            ecc_len,
            encoder: Encoder::new(ecc_len),
            decoder: Decoder::new(ecc_len),
        }
    }

    fn block_data(&self) -> usize {
        BLOCK_TOTAL - self.ecc_len
    }

    /// Encode `data` into a sequence of RS-protected 255-byte blocks.
    ///
    /// A 4-byte big-endian length prefix is prepended before blocking so the
    /// decoder can strip trailing padding without side-channel information.
    pub fn encode(&self, data: &[u8]) -> Vec<u8> {
        let block_data = self.block_data();
        let orig_len = data.len() as u32;

        // Prefix + data.
        let mut input = Vec::with_capacity(PREFIX_LEN + data.len());
        input.extend_from_slice(&orig_len.to_be_bytes());
        input.extend_from_slice(data);

        // Pad up to the next multiple of block_data.
        let padded_len = input.len().div_ceil(block_data) * block_data;
        input.resize(padded_len, 0u8);

        let n_blocks = input.len() / block_data;
        let mut out = Vec::with_capacity(n_blocks * BLOCK_TOTAL);

        for chunk in input.chunks(block_data) {
            let encoded = self.encoder.encode(chunk);
            out.extend_from_slice(&encoded);
        }

        out
    }

    /// Decode and error-correct a sequence of RS-protected 255-byte blocks.
    ///
    /// Returns the original bytes that were passed to [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// Returns [`ModemError::Fec`] when:
    /// - The input length is not a multiple of 255.
    /// - A block has more than `ecc_len / 2` byte errors.
    /// - The embedded length prefix is inconsistent with decoded content.
    pub fn decode(&self, data: &[u8]) -> Result<Vec<u8>, ModemError> {
        let block_data = self.block_data();

        if data.is_empty() || !data.len().is_multiple_of(BLOCK_TOTAL) {
            return Err(ModemError::Fec(format!(
                "FEC data length {} is not a non-zero multiple of {BLOCK_TOTAL}",
                data.len()
            )));
        }

        let mut decoded = Vec::with_capacity(data.len() / BLOCK_TOTAL * block_data);

        for (i, block) in data.chunks(BLOCK_TOTAL).enumerate() {
            let corrected = self.decoder.correct(block, None).map_err(|e| {
                ModemError::Fec(format!("RS correction failed at block {i}: {e:?}"))
            })?;
            decoded.extend_from_slice(&corrected[..block_data]);
        }

        // Read original length from 4-byte big-endian prefix.
        if decoded.len() < PREFIX_LEN {
            return Err(ModemError::Fec(
                "decoded data too short to contain length prefix".into(),
            ));
        }
        let orig_len = u32::from_be_bytes(decoded[..PREFIX_LEN].try_into().unwrap()) as usize;

        let end = PREFIX_LEN + orig_len;
        if decoded.len() < end {
            return Err(ModemError::Fec(format!(
                "decoded data shorter than expected: have {} bytes, need {end}",
                decoded.len()
            )));
        }

        Ok(decoded[PREFIX_LEN..end].to_vec())
    }
}

impl Default for FecCodec {
    fn default() -> Self {
        Self::new()
    }
}

// ── Short-block RS codec ──────────────────────────────────────────────────────

/// ECC bytes appended by [`ShortFecCodec`] (default instance).  t = 4, corrects
/// up to 4 byte errors per payload.
pub const SHORT_FEC_ECC_LEN: usize = 8;

/// Short-block Reed-Solomon codec for ACK and control frames.
///
/// Unlike [`FecCodec`], this codec operates on a single payload without block
/// padding or a length prefix.  Output is exactly `payload.len() + ecc_len` bytes,
/// making it practical for small fixed-size frames (e.g. 5-byte FSK4-ACK → 13 bytes).
///
/// Maximum input per call: `255 - ecc_len` bytes (247 bytes for the default
/// `ecc_len = 8`).
pub struct ShortFecCodec {
    ecc_len: usize,
    encoder: Encoder,
    decoder: Decoder,
}

impl ShortFecCodec {
    /// Create a codec with [`SHORT_FEC_ECC_LEN`] ECC bytes (t = 4).
    pub fn new() -> Self {
        Self::with_ecc_len(SHORT_FEC_ECC_LEN)
    }

    /// Create a codec with a custom ECC byte count (must be in 1..=254).
    pub fn with_ecc_len(ecc_len: usize) -> Self {
        assert!(
            (1..=254).contains(&ecc_len),
            "ShortFecCodec: ecc_len must be 1..=254, got {ecc_len}"
        );
        Self {
            ecc_len,
            encoder: Encoder::new(ecc_len),
            decoder: Decoder::new(ecc_len),
        }
    }

    /// Encode `data` → `data.len() + ecc_len` bytes.
    pub fn encode(&self, data: &[u8]) -> Result<Vec<u8>, ModemError> {
        let max = 255 - self.ecc_len; // safe: ecc_len ≤ 254 by construction
        if data.len() > max {
            return Err(ModemError::Fec(format!(
                "ShortFecCodec: payload {} bytes exceeds maximum {max}",
                data.len()
            )));
        }
        Ok(self.encoder.encode(data).to_vec())
    }

    /// Decode and error-correct `encoded`, returning the original data bytes.
    ///
    /// `encoded.len()` must be greater than `ecc_len`; the data portion is
    /// `encoded.len() - ecc_len` bytes.
    pub fn decode(&self, encoded: &[u8]) -> Result<Vec<u8>, ModemError> {
        if encoded.len() <= self.ecc_len {
            return Err(ModemError::Fec(format!(
                "ShortFecCodec: encoded length {} ≤ ecc_len {}",
                encoded.len(),
                self.ecc_len
            )));
        }
        let data_len = encoded.len() - self.ecc_len;
        let corrected = self
            .decoder
            .correct(encoded, None)
            .map_err(|e| ModemError::Fec(format!("ShortFecCodec: RS correction failed: {e:?}")))?;
        Ok(corrected[..data_len].to_vec())
    }
}

impl Default for ShortFecCodec {
    fn default() -> Self {
        Self::new()
    }
}

// ── Block interleaver ─────────────────────────────────────────────────────────

/// Gilbert-Elliott moderate-burst profile mean burst length (symbols).
const BURST_DURATION_SYMBOLS: usize = 20;

/// Default interleaver depth: 5 × the expected maximum burst duration in symbols.
///
/// At the Gilbert-Elliott moderate-burst profile (mean 20 symbols) this gives
/// depth 100. With 10 RS blocks of encoded data (2550 bytes), a burst of 100
/// distributes ≤ ⌈100×255/2550⌉ ≈ 10 errors per block — within the 16-byte
/// RS correction capacity.
///
/// When pairing with a convolutional code of constraint length `k`, depth must
/// be ≥ 2(k−1) to ensure burst fragments span distinct code constraint windows.
pub const DEFAULT_INTERLEAVER_DEPTH: usize = 5 * BURST_DURATION_SYMBOLS;

/// Stride-based block interleaver for burst-error dispersion.
///
/// Converts a burst of ≤ `depth` consecutive channel byte errors into at most
/// one error per `depth`-byte stride across the original data, enabling RS to
/// correct bursts that would otherwise overwhelm a single 255-byte block.
///
/// # Algorithm
///
/// Derived from the PACTOR-4 stride interleaver: output position `i` draws from
/// source position `P`, advanced by `depth` each step and reset to a sequential
/// fill pointer when it wraps past `n`.  The inverse (deinterleave) uses the
/// same permutation in reverse.
pub struct Interleaver {
    depth: usize,
}

impl Interleaver {
    /// Create an interleaver with the given stride depth.
    pub fn new(depth: usize) -> Self {
        assert!(depth > 0, "interleaver depth must be > 0");
        Self { depth }
    }

    /// Create an interleaver with [`DEFAULT_INTERLEAVER_DEPTH`].
    pub fn default_depth() -> Self {
        Self::new(DEFAULT_INTERLEAVER_DEPTH)
    }

    /// Build the forward permutation: `perm[i]` is the source index in the
    /// original buffer for output position `i`.
    fn permutation(n: usize, depth: usize) -> Vec<usize> {
        let mut perm = Vec::with_capacity(n);
        let mut p = 0usize;
        let mut s = 1usize;
        for _ in 0..n {
            perm.push(p);
            p += depth;
            if p >= n {
                p = s;
                s += 1;
            }
        }
        perm
    }

    /// Interleave `data` for transmission: stride-separates originally adjacent
    /// bytes so channel bursts hit widely-spaced positions in the original.
    pub fn interleave(&self, data: &[u8]) -> Vec<u8> {
        let perm = Self::permutation(data.len(), self.depth);
        perm.iter().map(|&src| data[src]).collect()
    }

    /// Invert the interleave permutation to restore original byte order.
    pub fn deinterleave(&self, data: &[u8]) -> Vec<u8> {
        let perm = Self::permutation(data.len(), self.depth);
        let mut out = vec![0u8; data.len()];
        for (i, &src) in perm.iter().enumerate() {
            out[src] = data[i];
        }
        out
    }
}

// ── Memory-ARQ soft combiner ──────────────────────────────────────────────────

/// Element-wise sample accumulator for Memory-ARQ maximal-ratio combining.
///
/// Each call to [`push`](Self::push) adds a received sample buffer to the
/// accumulator.  [`combine`](Self::combine) returns the element-wise mean,
/// which coherently averages noise and reinforces the signal component.
///
/// Combining N identical retransmissions improves effective SNR by ~3 dB per
/// doubling of N (10 log₁₀ N dB total).  No wire-protocol change is required —
/// the sender simply retransmits the same frame; only the receiver accumulates.
pub struct SoftCombiner {
    accumulator: Vec<f32>,
    count: usize,
}

impl SoftCombiner {
    /// Create an empty combiner.
    pub fn new() -> Self {
        Self {
            accumulator: Vec::new(),
            count: 0,
        }
    }

    /// Accumulate `samples` into the combiner.
    ///
    /// On the first call the buffer is cloned; subsequent calls add element-wise
    /// up to the shorter of the two lengths (trailing samples of the longer
    /// buffer are discarded to guard against framing drift).
    pub fn push(&mut self, samples: &[f32]) {
        if self.accumulator.is_empty() {
            self.accumulator = samples.to_vec();
        } else {
            let len = self.accumulator.len().min(samples.len());
            self.accumulator.truncate(len);
            for (a, &s) in self.accumulator.iter_mut().zip(samples) {
                *a += s;
            }
        }
        self.count += 1;
    }

    /// Return the element-wise mean of all pushed sample buffers.
    ///
    /// Returns an empty `Vec` if no buffers have been pushed.
    pub fn combine(&self) -> Vec<f32> {
        if self.count == 0 {
            return Vec::new();
        }
        let n = self.count as f32;
        self.accumulator.iter().map(|&s| s / n).collect()
    }

    /// Number of sample buffers accumulated so far.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Reset to the empty state.
    pub fn reset(&mut self) {
        self.accumulator.clear();
        self.count = 0;
    }
}

impl Default for SoftCombiner {
    fn default() -> Self {
        Self::new()
    }
}

// ── Weighted LLR combiner ─────────────────────────────────────────────────────

/// Combine multiple soft-demodulated LLR vectors using inverse-noise-variance weighting.
///
/// Each attempt is a pair of `(llrs, noise_var)` where `noise_var` is the estimated
/// per-frame noise variance.  Frames with lower noise (higher confidence) receive
/// proportionally higher weight.
///
/// If `noise_var` is zero or negative it is clamped to `f32::MIN_POSITIVE` so the
/// call never panics.  If `attempts` is empty an empty vector is returned.
///
/// **Length mismatch**: the output length is truncated to the shortest input LLR vector.
/// This guards against framing drift while preserving the most-reliable samples, but
/// callers should ensure all attempts produce the same number of LLRs to avoid
/// discarding information from longer vectors.
pub fn combine_llrs_weighted(attempts: &[(&[f32], f32)]) -> Vec<f32> {
    if attempts.is_empty() {
        return Vec::new();
    }
    let len = attempts.iter().map(|(l, _)| l.len()).min().unwrap_or(0);
    if len == 0 {
        return Vec::new();
    }
    let mut out = vec![0.0f32; len];
    let mut weight_sum = 0.0f32;
    for (llrs, noise_var) in attempts {
        let w = 1.0 / noise_var.max(f32::MIN_POSITIVE);
        weight_sum += w;
        for (o, &l) in out.iter_mut().zip(llrs.iter()) {
            *o += w * l;
        }
    }
    if weight_sum > 0.0 {
        for o in &mut out {
            *o /= weight_sum;
        }
    }
    out
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fec_mode_strength_ordering() {
        assert!(FecMode::Rs.strength() > FecMode::None.strength());
        assert!(FecMode::RsInterleaved.strength() > FecMode::Rs.strength());
        assert!(FecMode::Concatenated.strength() > FecMode::RsInterleaved.strength());
        assert!(FecMode::RsStrong.strength() > FecMode::Concatenated.strength());
        assert!(FecMode::SoftConcatenated.strength() > FecMode::RsStrong.strength());
        assert!(FecMode::Ldpc.strength() > FecMode::SoftConcatenated.strength());
    }

    #[test]
    fn fec_mode_negotiate_picks_strongest_common() {
        let offered = [FecMode::None, FecMode::Rs, FecMode::SoftConcatenated];
        let accepted = [FecMode::None, FecMode::Rs, FecMode::Ldpc];
        assert_eq!(FecMode::negotiate(&offered, &accepted), FecMode::Rs);
    }

    #[test]
    fn round_trip_empty() {
        let codec = FecCodec::new();
        let enc = codec.encode(&[]);
        let dec = codec.decode(&enc).unwrap();
        assert!(dec.is_empty());
    }

    #[test]
    fn round_trip_small() {
        let codec = FecCodec::new();
        let payload = b"OpenPulse FEC test";
        let enc = codec.encode(payload);
        assert_eq!(
            enc.len() % BLOCK_TOTAL,
            0,
            "encoded length must be multiple of 255"
        );
        let dec = codec.decode(&enc).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn round_trip_exact_block() {
        // Payload that fills exactly one block (BLOCK_DATA_STANDARD - PREFIX_LEN = 219 bytes).
        let payload = vec![0x42u8; BLOCK_DATA_STANDARD - PREFIX_LEN];
        let codec = FecCodec::new();
        let enc = codec.encode(&payload);
        assert_eq!(enc.len(), BLOCK_TOTAL);
        let dec = codec.decode(&enc).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn round_trip_multi_block() {
        // Payload that requires multiple blocks.
        let payload: Vec<u8> = (0..500u16).map(|v| (v & 0xFF) as u8).collect();
        let codec = FecCodec::new();
        let enc = codec.encode(&payload);
        assert_eq!(enc.len() % BLOCK_TOTAL, 0);
        let dec = codec.decode(&enc).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn corrects_up_to_16_byte_errors_per_block() {
        let codec = FecCodec::new();
        let payload = b"error correction test payload abc";
        let mut enc = codec.encode(payload);

        // Flip 16 bytes in the first block (max correctable per block).
        for i in 0..16 {
            enc[i * 4] ^= 0xFF;
        }

        let dec = codec.decode(&enc).unwrap();
        assert_eq!(
            dec, payload,
            "FEC should recover payload after ≤16 byte errors"
        );
    }

    #[test]
    fn fails_on_excessive_errors() {
        let codec = FecCodec::new();
        let payload = b"too many errors";
        let mut enc = codec.encode(payload);

        // Corrupt 17 bytes (one more than the correction capacity).
        for i in 0..17 {
            enc[i] ^= 0xFF;
        }

        assert!(
            codec.decode(&enc).is_err(),
            "should fail when errors exceed correction capacity"
        );
    }

    #[test]
    fn rejects_invalid_length() {
        let codec = FecCodec::new();
        assert!(codec.decode(&[0u8; 100]).is_err());
        assert!(codec.decode(&[]).is_err());
    }

    // ── Interleaver tests ─────────────────────────────────────────────────────

    #[test]
    fn interleaver_round_trip() {
        let il = Interleaver::new(DEFAULT_INTERLEAVER_DEPTH);
        let data: Vec<u8> = (0..255).collect();
        assert_eq!(il.deinterleave(&il.interleave(&data)), data);
    }

    #[test]
    fn interleaver_round_trip_non_multiple() {
        let il = Interleaver::new(DEFAULT_INTERLEAVER_DEPTH);
        let data: Vec<u8> = (0u16..300).map(|v| (v & 0xFF) as u8).collect();
        assert_eq!(il.deinterleave(&il.interleave(&data)), data);
    }

    #[test]
    fn interleaver_changes_order() {
        let il = Interleaver::new(7);
        let data: Vec<u8> = (0..20).collect();
        let interleaved = il.interleave(&data);
        // Depth 7 on 20 bytes must actually reorder bytes.
        assert_ne!(interleaved, data);
        // And round-trips correctly.
        assert_eq!(il.deinterleave(&interleaved), data);
    }

    #[test]
    fn burst_correctable_through_fec_and_interleaver() {
        let codec = FecCodec::new();
        let il = Interleaver::new(DEFAULT_INTERLEAVER_DEPTH);
        // 10 RS blocks of encoded data (2550 bytes) ensures a burst of
        // DEFAULT_INTERLEAVER_DEPTH bytes distributes ≤ ⌈100×255/2550⌉ ≈ 10 errors
        // per block — within the 16-byte RS correction capacity.
        let payload: Vec<u8> = (0..2190u16).map(|v| (v & 0xFF) as u8).collect();

        let encoded = codec.encode(&payload);
        assert_eq!(encoded.len(), 2550, "expected 10 RS blocks");
        let interleaved = il.interleave(&encoded);

        // Inject DEFAULT_INTERLEAVER_DEPTH consecutive byte errors starting at offset 10.
        let mut corrupted = interleaved.clone();
        let burst_start = 10;
        for i in burst_start..burst_start + DEFAULT_INTERLEAVER_DEPTH {
            corrupted[i] ^= 0xFF;
        }

        let deinterleaved = il.deinterleave(&corrupted);
        let recovered = codec.decode(&deinterleaved).unwrap();
        assert_eq!(recovered, payload);
    }

    // ── combine_llrs_weighted tests ───────────────────────────────────────────

    #[test]
    fn combine_llrs_weighted_empty_returns_empty() {
        assert!(combine_llrs_weighted(&[]).is_empty());
    }

    #[test]
    fn combine_llrs_weighted_single_attempt_is_identity() {
        let llrs = [1.0f32, -2.0, 3.0];
        let out = combine_llrs_weighted(&[(&llrs, 1.0)]);
        assert_eq!(out.len(), 3);
        for (o, &e) in out.iter().zip(llrs.iter()) {
            assert!((o - e).abs() < 1e-5, "expected {e} got {o}");
        }
    }

    #[test]
    fn combine_llrs_weighted_equal_weights_is_mean() {
        // Two attempts, equal noise_var → result should equal element-wise mean.
        let a = [2.0f32, -4.0];
        let b = [4.0f32, -2.0];
        let out = combine_llrs_weighted(&[(&a, 1.0), (&b, 1.0)]);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 3.0).abs() < 1e-5);
        assert!((out[1] - (-3.0)).abs() < 1e-5);
    }

    #[test]
    fn combine_llrs_weighted_higher_weight_dominates() {
        // Attempt A: noise_var=0.1 (high confidence, high weight)
        // Attempt B: noise_var=10.0 (low confidence, low weight)
        // Result should be closer to A than B.
        let a = [10.0f32];
        let b = [-10.0f32];
        let out = combine_llrs_weighted(&[(&a, 0.1), (&b, 10.0)]);
        assert_eq!(out.len(), 1);
        assert!(out[0] > 0.0, "high-confidence positive LLR should dominate");
    }

    #[test]
    fn combine_llrs_weighted_length_mismatch_truncates_to_shorter() {
        // Shorter vector determines output length; trailing samples of the
        // longer vector are dropped (documented truncation behaviour).
        let a = [1.0f32, 2.0, 3.0];
        let b = [1.0f32, 2.0];
        let out = combine_llrs_weighted(&[(&a, 1.0), (&b, 1.0)]);
        assert_eq!(out.len(), 2, "output truncated to min length");
    }
}
