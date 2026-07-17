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
    /// High-rate LDPC, rate ≈8/9 (k=1024, n=1152), via Progressive Edge-Growth.
    ///
    /// For the dense, high-SNR rungs (8PSK / 16QAM / 32APSK), where the channel
    /// can afford minimal redundancy: a soft-decision code at nearly the RS code
    /// rate but with real coding gain (waterfall ≈4 dB Es/N0).  CPU implementation
    /// is `openpulse_core::ldpc::LdpcCodec::high_rate`.  Requires a soft-capable
    /// demodulator — paired with a hard-decision plugin it gains nothing over RS.
    LdpcHighRate,
    /// Rate-1/3 PCCC turbo code (3GPP QPP interleaver, Max-Log-MAP BCJR, 8 iterations).
    ///
    /// Higher coding gain than LDPC for short block sizes (≤ 256 bits).
    Turbo,
}

impl FecMode {
    /// Numeric strength for negotiation; higher = stronger / preferred.
    pub fn strength(self) -> u8 {
        match self {
            FecMode::None => 0,
            // High-rate LDPC carries the least redundancy of any FEC mode
            // (rate ≈8/9), so it is the weakest non-None option for negotiation:
            // a peer falls back to it only when no more-protective mode is mutual.
            FecMode::LdpcHighRate => 1,
            FecMode::Rs => 2,
            FecMode::RsInterleaved => 3,
            FecMode::Concatenated => 4,
            FecMode::ShortRs => 5,
            FecMode::RsStrong => 6,
            FecMode::SoftConcatenated => 7,
            FecMode::Ldpc => 8,
            FecMode::Turbo => 9,
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

/// Number of 255-byte RS blocks [`FecCodec::encode`] produces for a `data`-byte input.
///
/// Mirrors `encode`'s blocking exactly: a 4-byte length prefix is prepended, then the result is
/// split into `data_per_block`-byte chunks (≥ 1 block even for empty input).
fn rs_block_count(data_len: usize, data_per_block: usize) -> usize {
    (PREFIX_LEN + data_len).div_ceil(data_per_block).max(1)
}

/// Upgrade `Rs` (t=16) to `RsStrong` (t=32) **only when the stronger code is free on the wire** — it
/// needs no more 255-byte blocks than `Rs` would for the same `data`-byte input to [`FecCodec::encode`].
///
/// `RsStrong` roughly doubles the weak rungs' fading decode (BPSK31 @3 dB 0.25 → 1.00) at zero airtime
/// cost wherever both codes fill the same number of blocks — most real HF traffic is small frames. It
/// is emphatically **not** free in the bands where t=32's larger parity spills into an extra block
/// (e.g. a 200-byte payload frames to 210 B → RS input 214 B → 1 Rs block but 2 RsStrong blocks →
/// double the airtime), which is exactly what regressed `hpx_hf`'s AWGN goodput when RsStrong was
/// applied unconditionally. Anything but `Rs` is returned unchanged.
///
/// `encode_input_len` is the length handed to `FecCodec::encode` — i.e. the framed payload
/// (`Frame::encode()` bytes), before the codec's own 4-byte prefix.
pub fn free_rs_strengthening(fec: FecMode, encode_input_len: usize) -> FecMode {
    if fec == FecMode::Rs
        && rs_block_count(encode_input_len, BLOCK_TOTAL - FEC_ECC_LEN_STRONG)
            == rs_block_count(encode_input_len, BLOCK_TOTAL - FEC_ECC_LEN)
    {
        FecMode::RsStrong
    } else {
        fec
    }
}

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
        let prefix: [u8; PREFIX_LEN] = decoded[..PREFIX_LEN]
            .try_into()
            .map_err(|_| ModemError::Fec("length-prefix slice conversion failed".into()))?;
        let orig_len = u32::from_be_bytes(prefix) as usize;

        // `checked_add` (audit RX-2): the 4-byte length prefix is systematic (attacker-controlled), and
        // on a 32-bit/wasm `usize` `PREFIX_LEN + orig_len` can wrap, making the `< end` guard pass and the
        // slice panic. A wrapped/oversized end simply can't fit the buffer, so treat it as too-short.
        let end = match PREFIX_LEN.checked_add(orig_len) {
            Some(end) if end <= decoded.len() => end,
            _ => {
                return Err(ModemError::Fec(format!(
                    "decoded data shorter than expected: have {} bytes, need {}",
                    decoded.len(),
                    PREFIX_LEN as u64 + orig_len as u64
                )));
            }
        };

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
        // Upper bound (audit RX-1): a valid RS block is at most 255 bytes. The underlying reed-solomon
        // decoder is backed by a fixed 256-byte polynomial and PANICS on any longer input — and this
        // decodes attacker-length-controlled demodulator output (ACK-listen / short-FEC receive), so an
        // unbounded length would be a remote panic DoS. Reject rather than let it reach the decoder.
        if encoded.len() > 255 {
            return Err(ModemError::Fec(format!(
                "ShortFecCodec: encoded length {} exceeds the 255-byte RS block limit",
                encoded.len()
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

/// Combine soft-demodulated LLR vectors that are **not** already scaled by their noise variance.
///
/// Each attempt is a pair of `(llrs, noise_var)` and gets weight `1/noise_var`.
///
/// # When this is the wrong function
///
/// A true log-likelihood ratio `log P(b=0|y) / P(b=1|y)` already carries `1/σ²` — see
/// `openpulse_dsp::constellation::symbol_llrs`, which divides every distance by `noise_var`. For
/// independent observations of the same bit, the MAP combine of true LLRs is their plain **sum**:
/// use [`combine_llrs_map`]. Passing `noise_var = σ²` here on top of already-calibrated LLRs applies
/// `σ⁻²` twice, over-weighting the best attempt and discarding information from the others.
///
/// The general weight is the *calibration correction* `σ̂²_used_inside_the_LLR / σ²_true`, which is
/// `1` for a calibrated demodulator. This function exists for the demodulators whose LLRs carry an
/// arbitrary, noise-blind scale — the ±1.0 hard-decision fallback, and the plugins that pass
/// `noise_var = 1.0` to `symbol_llrs` (see the `ModulationPlugin::demodulate_soft` LLR-scale
/// contract, which does not normalise across plugins).
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

/// Combine calibrated LLR vectors from repeated observations of the same bits: their plain **sum**.
///
/// For independent observations, log-likelihood ratios add. The sum is therefore the exact MAP
/// combine — and, because each LLR already carries `1/σ²`, it *is* inverse-noise weighting. No
/// per-attempt weight is needed or wanted; see [`combine_llrs_weighted`] for the uncalibrated case.
///
/// The magnitude grows with the attempt count (K-fold confidence), which is the correct posterior.
/// Consumers that only take the sign are unaffected.
///
/// **Length mismatch**: the output is truncated to the shortest input, as in [`combine_llrs_weighted`].
pub fn combine_llrs_map(attempts: &[&[f32]]) -> Vec<f32> {
    let len = attempts.iter().map(|l| l.len()).min().unwrap_or(0);
    if len == 0 {
        return Vec::new();
    }
    let mut out = vec![0.0f32; len];
    for llrs in attempts {
        for (o, &l) in out.iter_mut().zip(llrs.iter()) {
            *o += l;
        }
    }
    out
}

/// Hard-decide a bit-LLR stream to bytes in the engine convention: LSB-first, a negative LLR is bit 1.
pub fn hard_decide(llrs: &[f32]) -> Vec<u8> {
    llrs.chunks(8)
        .map(|chunk| {
            chunk.iter().enumerate().fold(0u8, |acc, (i, &llr)| {
                acc | ((llr.is_sign_negative() as u8) << i)
            })
        })
        .collect()
}

/// [`combine_llrs_map`] applied only inside `feedback.ranges`; the first attempt is preserved elsewhere.
///
/// The combined ranges are divided by the attempt count so the whole vector stays on the
/// single-attempt LLR scale that the preserved region uses.
///
/// Assumes an LLR layout of exactly 8 bit-LLRs per protected byte (LSB-first),
/// i.e. byte offsets are converted to LLR offsets by multiplying by 8.
pub fn combine_llrs_map_in_ranges(attempts: &[&[f32]], feedback: &WindowArqFeedback) -> Vec<f32> {
    if attempts.is_empty() {
        return Vec::new();
    }
    let len = attempts.iter().map(|l| l.len()).min().unwrap_or(0);
    if len == 0 {
        return Vec::new();
    }
    let mut out = attempts[0][..len].to_vec();
    let inv_n = 1.0 / attempts.len() as f32;

    for range in &feedback.ranges {
        let start = (range.start as usize).saturating_mul(8).min(len);
        let end = start
            .saturating_add((range.len as usize).saturating_mul(8))
            .min(len);
        if start >= end {
            continue;
        }
        let slices: Vec<&[f32]> = attempts.iter().map(|l| &l[start..end]).collect();
        let combined = combine_llrs_map(&slices);
        for (o, c) in out[start..end].iter_mut().zip(combined.iter()) {
            *o = c * inv_n;
        }
    }
    out
}

/// Fixed wire size (bytes) for Window-ARQ failed-range feedback.
pub const WINDOW_ARQ_FEEDBACK_SIZE: usize = 8;

/// Maximum number of byte ranges encoded in a Window-ARQ feedback frame.
pub const WINDOW_ARQ_MAX_RANGES: usize = 2;

/// A contiguous byte range inside a protected frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    /// Start byte offset from the beginning of the protected frame.
    pub start: u16,
    /// Number of bytes in the range (max 255 by wire format design).
    ///
    /// Callers must split larger contiguous failures into multiple ranges.
    pub len: u8,
}

impl ByteRange {
    fn end_exclusive(self) -> usize {
        self.start as usize + self.len as usize
    }
}

/// Receiver feedback for selective Window-ARQ retries.
///
/// The codec uses a fixed 8-byte wire format:
/// `count(1) | range0(start_le:2,len:1) | range1(start_le:2,len:1) | pad(1)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowArqFeedback {
    /// Non-overlapping failed byte ranges sorted by `start`.
    pub ranges: Vec<ByteRange>,
}

impl WindowArqFeedback {
    /// Build and validate a feedback object.
    pub fn new(mut ranges: Vec<ByteRange>) -> Result<Self, ModemError> {
        // Canonicalise construction to sorted order.
        ranges.sort_by_key(|r| r.start);
        Self::validate_ranges(&ranges, true)?;
        Ok(Self { ranges })
    }

    fn validate_ranges(ranges: &[ByteRange], require_sorted: bool) -> Result<(), ModemError> {
        if ranges.len() > WINDOW_ARQ_MAX_RANGES {
            return Err(ModemError::Frame(format!(
                "window-arq feedback supports at most {WINDOW_ARQ_MAX_RANGES} ranges"
            )));
        }
        if ranges.iter().any(|r| r.len == 0) {
            return Err(ModemError::Frame(
                "window-arq feedback range length must be >= 1".into(),
            ));
        }
        for pair in ranges.windows(2) {
            let left = pair[0];
            let right = pair[1];
            if require_sorted && left.start > right.start {
                return Err(ModemError::Frame(
                    "window-arq feedback ranges must be sorted by start".into(),
                ));
            }
            if left.end_exclusive() > right.start as usize {
                return Err(ModemError::Frame(
                    "window-arq feedback ranges must not overlap".into(),
                ));
            }
        }
        Ok(())
    }

    /// Return the total number of failed bytes represented by `ranges`.
    pub fn failed_byte_count(&self) -> usize {
        self.ranges.iter().map(|r| r.len as usize).sum()
    }

    /// Encode feedback to a fixed-size 8-byte wire frame.
    pub fn encode(&self) -> Result<[u8; WINDOW_ARQ_FEEDBACK_SIZE], ModemError> {
        if self.ranges.len() > WINDOW_ARQ_MAX_RANGES {
            return Err(ModemError::Frame(format!(
                "window-arq feedback supports at most {WINDOW_ARQ_MAX_RANGES} ranges"
            )));
        }
        let mut out = [0u8; WINDOW_ARQ_FEEDBACK_SIZE];
        out[0] = self.ranges.len() as u8;
        for (i, range) in self.ranges.iter().enumerate() {
            let off = 1 + i * 3;
            out[off..off + 2].copy_from_slice(&range.start.to_le_bytes());
            out[off + 2] = range.len;
        }
        Ok(out)
    }

    /// Decode a fixed-size 8-byte feedback wire frame.
    pub fn decode(encoded: &[u8]) -> Result<Self, ModemError> {
        if encoded.len() != WINDOW_ARQ_FEEDBACK_SIZE {
            return Err(ModemError::Frame(format!(
                "window-arq feedback must be exactly {WINDOW_ARQ_FEEDBACK_SIZE} bytes, got {}",
                encoded.len()
            )));
        }
        let count = encoded[0] as usize;
        if count > WINDOW_ARQ_MAX_RANGES {
            return Err(ModemError::Frame(format!(
                "window-arq feedback range count {count} exceeds max {WINDOW_ARQ_MAX_RANGES}"
            )));
        }

        let mut ranges = Vec::with_capacity(count);
        for i in 0..count {
            let off = 1 + i * 3;
            let start = u16::from_le_bytes([encoded[off], encoded[off + 1]]);
            let len = encoded[off + 2];
            ranges.push(ByteRange { start, len });
        }
        Self::validate_ranges(&ranges, true)?;
        Ok(Self { ranges })
    }
}

/// Encode a selective retransmit packet containing only failed byte ranges.
///
/// Wire format: `MAGIC(1)=0xA5 | count(1) | repeated(start_le:2,len:1,data:len)`.
///
/// Packet overhead is `2 + 3*N` bytes (`N` = number of ranges).  Therefore,
/// the "<=120% of failed bytes" criterion is not universal for tiny failure
/// sets; callers should apply that gate only when failed bytes are large enough
/// to amortize header/range descriptors.
pub fn encode_window_retransmit(
    protected_frame: &[u8],
    feedback: &WindowArqFeedback,
) -> Result<Vec<u8>, ModemError> {
    let mut out = Vec::with_capacity(2 + feedback.ranges.len() * 3 + feedback.failed_byte_count());
    out.push(0xA5);
    out.push(feedback.ranges.len() as u8);

    for range in &feedback.ranges {
        let start = range.start as usize;
        let end = range.end_exclusive();
        if end > protected_frame.len() {
            return Err(ModemError::Frame(format!(
                "window-arq range [{start}, {end}) exceeds frame length {}",
                protected_frame.len()
            )));
        }
        out.extend_from_slice(&range.start.to_le_bytes());
        out.push(range.len);
        out.extend_from_slice(&protected_frame[start..end]);
    }

    Ok(out)
}

/// Apply a selective retransmit packet to an existing protected frame buffer.
pub fn apply_window_retransmit(
    protected_frame: &mut [u8],
    retransmit_packet: &[u8],
) -> Result<(), ModemError> {
    if retransmit_packet.len() < 2 {
        return Err(ModemError::Frame(
            "window-arq packet too short for header".into(),
        ));
    }
    if retransmit_packet[0] != 0xA5 {
        return Err(ModemError::Frame("window-arq packet bad magic".into()));
    }
    let count = retransmit_packet[1] as usize;
    if count > WINDOW_ARQ_MAX_RANGES {
        return Err(ModemError::Frame(format!(
            "window-arq packet range count {count} exceeds max {WINDOW_ARQ_MAX_RANGES}"
        )));
    }

    let mut cursor = 2usize;
    for _ in 0..count {
        if cursor + 3 > retransmit_packet.len() {
            return Err(ModemError::Frame(
                "window-arq packet truncated at range header".into(),
            ));
        }
        let start =
            u16::from_le_bytes([retransmit_packet[cursor], retransmit_packet[cursor + 1]]) as usize;
        let len = retransmit_packet[cursor + 2] as usize;
        cursor += 3;

        let end = start + len;
        if cursor + len > retransmit_packet.len() {
            return Err(ModemError::Frame(
                "window-arq packet truncated at range payload".into(),
            ));
        }
        if end > protected_frame.len() {
            return Err(ModemError::Frame(format!(
                "window-arq patch range [{start}, {end}) exceeds frame length {}",
                protected_frame.len()
            )));
        }

        protected_frame[start..end].copy_from_slice(&retransmit_packet[cursor..cursor + len]);
        cursor += len;
    }

    if cursor != retransmit_packet.len() {
        return Err(ModemError::Frame(
            "window-arq packet has trailing bytes".into(),
        ));
    }

    Ok(())
}

/// Combine soft LLR attempts only inside failed byte ranges.
///
/// Outside `feedback.ranges`, values from the first attempt are preserved.
///
/// Assumes an LLR layout of exactly 8 bit-LLRs per protected byte (LSB-first),
/// i.e. byte offsets are converted to LLR offsets by multiplying by 8.
pub fn combine_llrs_weighted_in_ranges(
    attempts: &[(&[f32], f32)],
    feedback: &WindowArqFeedback,
) -> Vec<f32> {
    if attempts.is_empty() {
        return Vec::new();
    }
    let len = attempts.iter().map(|(l, _)| l.len()).min().unwrap_or(0);
    if len == 0 {
        return Vec::new();
    }

    let mut out = attempts[0].0[..len].to_vec();

    for range in &feedback.ranges {
        let start = (range.start as usize).saturating_mul(8).min(len);
        let end = start
            .saturating_add((range.len as usize).saturating_mul(8))
            .min(len);
        if start >= end {
            continue;
        }

        let mut slices: Vec<(&[f32], f32)> = Vec::with_capacity(attempts.len());
        for (llrs, noise_var) in attempts {
            slices.push((&llrs[start..end], *noise_var));
        }
        let combined = combine_llrs_weighted(&slices);
        out[start..end].copy_from_slice(&combined);
    }

    out
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// `free_rs_strengthening` must upgrade Rs→RsStrong exactly when RsStrong costs no extra block,
    /// and its notion of "block" must match `FecCodec::encode` byte-for-byte — a mismatch would
    /// either miss free upgrades or (worse) upgrade into an airtime regression.
    #[test]
    fn free_rs_strengthening_matches_actual_block_counts() {
        let rs = FecCodec::new();
        let strong = FecCodec::strong();
        // Sweep across the first two block boundaries (prefix pushes the transitions off the round
        // numbers): the helper's verdict must equal "same encoded block count".
        for len in [
            0usize, 64, 177, 186, 187, 188, 200, 209, 210, 300, 378, 379, 500,
        ] {
            let data = vec![0u8; len];
            let rs_blocks = rs.encode(&data).len() / BLOCK_TOTAL;
            let strong_blocks = strong.encode(&data).len() / BLOCK_TOTAL;
            let free = rs_blocks == strong_blocks;
            let got = free_rs_strengthening(FecMode::Rs, len);
            assert_eq!(
                got == FecMode::RsStrong,
                free,
                "len {len}: helper says {got:?}, but Rs={rs_blocks} blocks vs RsStrong={strong_blocks}"
            );
        }
    }

    #[test]
    fn free_rs_strengthening_only_touches_rs_and_protects_goodput() {
        // A 200-byte payload frames to 210 B, which needs a 2nd RsStrong block — must stay Rs (this
        // is the linksim goodput gate's frame size; upgrading it is the v0.14.0 regression).
        let framed_200 = 200 + crate::frame::Frame::WIRE_OVERHEAD;
        assert_eq!(free_rs_strengthening(FecMode::Rs, framed_200), FecMode::Rs);
        // A small frame is free.
        let framed_64 = 64 + crate::frame::Frame::WIRE_OVERHEAD;
        assert_eq!(
            free_rs_strengthening(FecMode::Rs, framed_64),
            FecMode::RsStrong
        );
        // Never touches any other FEC.
        for fec in [
            FecMode::None,
            FecMode::RsInterleaved,
            FecMode::SoftConcatenated,
            FecMode::LdpcHighRate,
            FecMode::RsStrong,
        ] {
            assert_eq!(free_rs_strengthening(fec, framed_64), fec);
        }
    }

    /// Audit RX-1: `ShortFecCodec::decode` must reject an over-255-byte input rather than pass it to
    /// the underlying reed-solomon decoder, whose fixed 256-byte polynomial buffer panics. The input is
    /// attacker-length-controlled demodulator output on the ACK-listen / short-FEC receive path.
    #[test]
    fn short_fec_decode_rejects_oversized_input_without_panicking() {
        let codec = ShortFecCodec::with_ecc_len(32);
        for len in [256usize, 300, 1024, 65_000] {
            let err = codec.decode(&vec![0u8; len]);
            assert!(
                err.is_err(),
                "an over-255-byte input ({len} B) must be rejected, not decoded"
            );
        }
        // A valid-length block still round-trips.
        let data = b"short fec payload";
        let encoded = codec.encode(data).unwrap();
        assert!(encoded.len() <= 255);
        assert_eq!(codec.decode(&encoded).unwrap(), data);
    }

    /// Calibrated LLRs already carry `1/σ²`, so summing them IS inverse-noise weighting. Weighting
    /// the sum again by `1/σ²` (the shape `1 / mean(|LLR|)` produces) applies σ⁻² twice and recovers
    /// fewer bits from a graded-SNR attempt set. This pins the bug the engine used to ship.
    #[test]
    fn map_combine_beats_double_inverse_noise_weighting_on_calibrated_llrs() {
        // Three observations of the same 256 bits at σ² = 1, 4, 16 (i.e. 0, −6, −12 dB).
        // A calibrated LLR is (2/σ²)·y for a ±1 BPSK symbol y = x + n.
        let sigmas2 = [1.0f32, 4.0, 16.0];
        let mut state = 0x5EEDu64;
        let mut next_gauss = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u1 = (((state >> 11) as f64) / ((1u64 << 53) as f64)).clamp(1e-12, 1.0);
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u2 = ((state >> 11) as f64) / ((1u64 << 53) as f64);
            ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
        };

        let n = 4096;
        let truth: Vec<bool> = (0..n).map(|i| i % 3 == 0).collect();
        let llrs: Vec<Vec<f32>> = sigmas2
            .iter()
            .map(|s2| {
                truth
                    .iter()
                    .map(|&b| {
                        let x = if b { -1.0 } else { 1.0 };
                        let y = x + s2.sqrt() * next_gauss();
                        2.0 * y / s2 // the true LLR: positive => bit 0
                    })
                    .collect()
            })
            .collect();

        let refs: Vec<&[f32]> = llrs.iter().map(|l| l.as_slice()).collect();
        let map = combine_llrs_map(&refs);

        // The old engine weight: 1 / mean(|LLR|), which for calibrated LLRs is ∝ σ².
        let weighted_refs: Vec<(&[f32], f32)> = llrs
            .iter()
            .map(|l| {
                let mean_abs = l.iter().map(|v| v.abs()).sum::<f32>() / l.len() as f32;
                (l.as_slice(), 1.0 / mean_abs)
            })
            .collect();
        let double = combine_llrs_weighted(&weighted_refs);

        let correct = |v: &[f32]| {
            v.iter()
                .zip(truth.iter())
                .filter(|(l, &b)| (**l < 0.0) == b)
                .count()
        };
        let (map_ok, double_ok) = (correct(&map), correct(&double));
        assert!(
            map_ok > double_ok,
            "MAP sum of calibrated LLRs must beat double inverse-noise weighting: \
             map={map_ok} double_weighted={double_ok} of {n}"
        );
    }

    /// `combine_llrs_weighted` is still the right tool for LLRs that carry no noise scaling —
    /// there, the weight is the only place reliability can enter.
    #[test]
    fn weighted_combine_beats_map_sum_on_uncalibrated_llrs() {
        // Same observations, but the demodulator forgot to divide by σ² (emits 2·y).
        let sigmas2 = [1.0f32, 16.0];
        let n = 4096;
        let truth: Vec<bool> = (0..n).map(|i| i % 3 == 0).collect();
        let mut state = 0x5EEDu64;
        let mut next_gauss = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u1 = (((state >> 11) as f64) / ((1u64 << 53) as f64)).clamp(1e-12, 1.0);
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u2 = ((state >> 11) as f64) / ((1u64 << 53) as f64);
            ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
        };
        let llrs: Vec<Vec<f32>> = sigmas2
            .iter()
            .map(|s2| {
                truth
                    .iter()
                    .map(|&b| {
                        let x = if b { -1.0 } else { 1.0 };
                        2.0 * (x + s2.sqrt() * next_gauss())
                    })
                    .collect()
            })
            .collect();

        let refs: Vec<&[f32]> = llrs.iter().map(|l| l.as_slice()).collect();
        let map = combine_llrs_map(&refs);
        let weighted: Vec<(&[f32], f32)> = llrs
            .iter()
            .zip(sigmas2.iter())
            .map(|(l, &s2)| (l.as_slice(), s2))
            .collect();
        let wt = combine_llrs_weighted(&weighted);

        let correct = |v: &[f32]| {
            v.iter()
                .zip(truth.iter())
                .filter(|(l, &b)| (**l < 0.0) == b)
                .count()
        };
        assert!(
            correct(&wt) > correct(&map),
            "with uncalibrated LLRs the σ²-weighted combine must beat the plain sum: \
             weighted={} map={} of {n}",
            correct(&wt),
            correct(&map)
        );
    }

    #[test]
    fn map_combine_is_the_llr_sum() {
        let a = [1.0f32, -2.0, 3.0];
        let b = [0.5f32, -0.5, -4.0];
        assert_eq!(combine_llrs_map(&[&a, &b]), vec![1.5, -2.5, -1.0]);
        assert!(combine_llrs_map(&[]).is_empty());
        // Truncates to the shortest input.
        let c = [1.0f32];
        assert_eq!(combine_llrs_map(&[&a, &c]).len(), 1);
    }

    #[test]
    fn fec_mode_strength_ordering() {
        // High-rate LDPC is the weakest non-None mode (least redundancy).
        assert!(FecMode::LdpcHighRate.strength() > FecMode::None.strength());
        assert!(FecMode::Rs.strength() > FecMode::LdpcHighRate.strength());
        assert!(FecMode::RsInterleaved.strength() > FecMode::Rs.strength());
        assert!(FecMode::Concatenated.strength() > FecMode::RsInterleaved.strength());
        assert!(FecMode::RsStrong.strength() > FecMode::Concatenated.strength());
        assert!(FecMode::SoftConcatenated.strength() > FecMode::RsStrong.strength());
        assert!(FecMode::Ldpc.strength() > FecMode::SoftConcatenated.strength());
        assert!(FecMode::Turbo.strength() > FecMode::Ldpc.strength());
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
        for item in enc.iter_mut().take(17) {
            *item ^= 0xFF;
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
        for item in corrupted
            .iter_mut()
            .skip(burst_start)
            .take(DEFAULT_INTERLEAVER_DEPTH)
        {
            *item ^= 0xFF;
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

    #[test]
    fn window_arq_feedback_codec_is_fixed_8_bytes_round_trip() {
        let fb = WindowArqFeedback::new(vec![
            ByteRange { start: 12, len: 8 },
            ByteRange { start: 64, len: 16 },
        ])
        .unwrap();
        let encoded = fb.encode().unwrap();
        assert_eq!(encoded.len(), WINDOW_ARQ_FEEDBACK_SIZE);
        let decoded = WindowArqFeedback::decode(&encoded).unwrap();
        assert_eq!(decoded, fb);
    }

    #[test]
    fn window_arq_feedback_rejects_invalid_ranges() {
        let overlap = WindowArqFeedback::new(vec![
            ByteRange { start: 10, len: 10 },
            ByteRange { start: 15, len: 3 },
        ]);
        assert!(overlap.is_err());

        let zero = WindowArqFeedback::new(vec![ByteRange { start: 2, len: 0 }]);
        assert!(zero.is_err());
    }

    #[test]
    fn window_arq_feedback_decode_rejects_unsorted_wire_order() {
        // count=2, range0 starts after range1.
        let encoded = [
            2, // count
            40, 0, 5, // r0: start=40 len=5
            10, 0, 5, // r1: start=10 len=5 (unsorted)
            0, // pad
        ];
        let decoded = WindowArqFeedback::decode(&encoded);
        assert!(decoded.is_err());
    }

    #[test]
    fn window_retransmit_payload_stays_within_120_percent_for_50_percent_loss() {
        let protected: Vec<u8> = (0u16..255).map(|v| (v & 0xFF) as u8).collect();
        let feedback = WindowArqFeedback::new(vec![
            ByteRange { start: 0, len: 64 },
            ByteRange {
                start: 128,
                len: 64,
            },
        ])
        .unwrap();

        let packet = encode_window_retransmit(&protected, &feedback).unwrap();
        let failed = feedback.failed_byte_count() as f32;
        let ratio = packet.len() as f32 / failed;
        assert!(
            ratio <= 1.20,
            "window retransmit ratio {ratio:.3} exceeds 1.20 limit"
        );
    }

    #[test]
    fn window_retransmit_small_failed_sets_can_exceed_120_percent() {
        let protected: Vec<u8> = (0u16..32).map(|v| (v & 0xFF) as u8).collect();
        let feedback = WindowArqFeedback::new(vec![
            ByteRange { start: 3, len: 1 },
            ByteRange { start: 9, len: 1 },
        ])
        .unwrap();

        let packet = encode_window_retransmit(&protected, &feedback).unwrap();
        let failed = feedback.failed_byte_count() as f32;
        let ratio = packet.len() as f32 / failed;
        assert!(
            ratio > 1.20,
            "expected tiny-failure ratio > 1.20, got {ratio}"
        );
    }

    #[test]
    fn apply_window_retransmit_patches_only_selected_ranges() {
        let original: Vec<u8> = (0u16..80).map(|v| (v & 0xFF) as u8).collect();
        let mut repaired = original.clone();
        let mut updated = original.clone();

        for b in &mut updated[10..20] {
            *b ^= 0x5A;
        }
        for b in &mut updated[40..50] {
            *b ^= 0xA5;
        }

        let fb = WindowArqFeedback::new(vec![
            ByteRange { start: 10, len: 10 },
            ByteRange { start: 40, len: 10 },
        ])
        .unwrap();

        let packet = encode_window_retransmit(&updated, &fb).unwrap();
        apply_window_retransmit(&mut repaired, &packet).unwrap();
        assert_eq!(repaired, updated);
    }

    #[test]
    fn combine_llrs_weighted_in_ranges_keeps_unselected_bits_from_first_attempt() {
        let first = vec![1.0f32; 32];
        let second = vec![-1.0f32; 32];
        let fb = WindowArqFeedback::new(vec![ByteRange { start: 2, len: 1 }]).unwrap();

        let out = combine_llrs_weighted_in_ranges(&[(&first, 1.0), (&second, 1.0)], &fb);

        // Byte 0..1 unchanged from first attempt.
        assert!(out[..16].iter().all(|&v| (v - 1.0).abs() < 1e-6));
        // Byte 2 (bits 16..24) averaged.
        assert!(out[16..24].iter().all(|&v| v.abs() < 1e-6));
        // Remaining bits unchanged from first attempt.
        assert!(out[24..].iter().all(|&v| (v - 1.0).abs() < 1e-6));
    }
}
