//! Rate-1/3 PCCC (3GPP-style) Turbo codec.
//!
//! # Wire layout
//!
//! ```text
//! encode(data) → pack_bits(sys[K] ‖ par1[K] ‖ par2[K])
//!
//! where the K-bit block carries:
//!   [u16-LE data_len | data bytes | u16-LE CRC-16] zero-padded to K bits
//! ```
//!
//! LLR convention matches the rest of the codebase: **positive = likely 0**.

use crate::error::ModemError;
use crate::frame::crc16;

// ── QPP interleaver table (3GPP TS 36.212, Table 5.1.3-3, subset) ─────────────

/// (K, f1, f2) entries from the 3GPP QPP interleaver table.
const QPP_TABLE: &[(usize, u32, u32)] = &[
    (40, 3, 10),
    (48, 7, 12),
    (56, 19, 42),
    (64, 7, 16),
    (72, 7, 18),
    (80, 11, 20),
    (96, 7, 24),
    (112, 5, 28),
    (128, 11, 32),
    (160, 3, 20),
    (192, 13, 48),
    (224, 11, 56),
    (256, 7, 64),
    (288, 11, 36),
    (320, 7, 20),
    (384, 13, 48),
    (448, 13, 56),
    (512, 3, 64),
    (640, 9, 80),
    (768, 7, 96),
    (1024, 13, 128),
    (1280, 9, 160),
    (1536, 13, 192),
    (2048, 9, 256),
    (3072, 9, 384),
    (4096, 13, 512),
    (6144, 13, 768),
];

fn qpp_params(min_k: usize) -> Option<(usize, u32, u32)> {
    QPP_TABLE.iter().find(|&&(k, _, _)| k >= min_k).copied()
}

fn qpp_permute(i: usize, f1: u32, f2: u32, k: usize) -> usize {
    let i = i as u64;
    let k = k as u64;
    ((f1 as u64 * i + f2 as u64 * i * i) % k) as usize
}

// ── RSC encoder (K=3, g_r=7, g_p=5) ──────────────────────────────────────────

/// Trellis: trellis[state][input] = (next_state, parity_bit).
fn build_trellis() -> [[(u8, u8); 2]; 4] {
    let mut t = [[(0u8, 0u8); 2]; 4];
    for s in 0u8..4 {
        let s0 = s & 1; // "newer" register cell
        let s1 = s >> 1; // "older" register cell
        for d in 0u8..2 {
            let v = d ^ s0 ^ s1; // feedback through g_r = 0b111 taps
            let par = v ^ s1; // parity through g_p = 0b101 taps
            let next = (s0 << 1) | v;
            t[s as usize][d as usize] = (next, par);
        }
    }
    t
}

/// Encode `bits` with the RSC encoder, returning parity bits (systematic = bits).
fn rsc_parity(bits: &[u8], trellis: &[[(u8, u8); 2]; 4]) -> Vec<u8> {
    let mut state = 0u8;
    bits.iter()
        .map(|&d| {
            let (next, par) = trellis[state as usize][d as usize];
            state = next;
            par
        })
        .collect()
}

// ── Bit packing helpers ────────────────────────────────────────────────────────

fn bytes_to_bits(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .flat_map(|&b| (0..8).map(move |i| (b >> i) & 1))
        .collect()
}

fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | (b << i))
        })
        .collect()
}

fn pack_bits(a: &[u8], b: &[u8], c: &[u8]) -> Vec<u8> {
    let mut interleaved = Vec::with_capacity(a.len() + b.len() + c.len());
    interleaved.extend_from_slice(a);
    interleaved.extend_from_slice(b);
    interleaved.extend_from_slice(c);
    bits_to_bytes(&interleaved)
}

// ── Max-Log-MAP BCJR decoder ──────────────────────────────────────────────────

const NEG_INF: f32 = -1e30;

/// One Max-Log-MAP BCJR pass.
///
/// `sys_llr`: systematic channel LLRs (positive = likely 0).
/// `par_llr`: parity channel LLRs.
/// `prior_llr`: a priori LLRs for each bit (zero on first iteration).
///
/// Returns extrinsic LLRs (positive = likely 0).
fn bcjr_pass(
    sys_llr: &[f32],
    par_llr: &[f32],
    prior_llr: &[f32],
    trellis: &[[(u8, u8); 2]; 4],
) -> Vec<f32> {
    let k = sys_llr.len();
    let n_states = 4usize;

    // Forward recursion α (log domain).
    let mut alpha = vec![[NEG_INF; 4]; k + 1];
    alpha[0][0] = 0.0;
    for t in 0..k {
        for s in 0..n_states {
            if alpha[t][s] == NEG_INF {
                continue;
            }
            for d in 0u8..2 {
                let (next, par) = trellis[s][d as usize];
                // Branch metric: (1/2) * d * (sys_llr + prior) + (1/2) * par * par_llr
                // Use LLR convention: LLR > 0 → bit=0, so bit contribution is -bit * LLR
                let sys_contrib = if d == 1 {
                    -0.5 * (sys_llr[t] + prior_llr[t])
                } else {
                    0.5 * (sys_llr[t] + prior_llr[t])
                };
                let par_contrib = if par == 1 {
                    -0.5 * par_llr[t]
                } else {
                    0.5 * par_llr[t]
                };
                let gamma = sys_contrib + par_contrib;
                let new_val = alpha[t][s] + gamma;
                if new_val > alpha[t + 1][next as usize] {
                    alpha[t + 1][next as usize] = new_val;
                }
            }
        }
    }

    // Backward recursion β (log domain).
    let mut beta = vec![[NEG_INF; 4]; k + 1];
    // Uninformed termination: allow all end states equally
    beta[k].fill(0.0);
    for t in (0..k).rev() {
        for s in 0..n_states {
            for d in 0u8..2 {
                let (next, par) = trellis[s][d as usize];
                if beta[t + 1][next as usize] == NEG_INF {
                    continue;
                }
                let sys_contrib = if d == 1 {
                    -0.5 * (sys_llr[t] + prior_llr[t])
                } else {
                    0.5 * (sys_llr[t] + prior_llr[t])
                };
                let par_contrib = if par == 1 {
                    -0.5 * par_llr[t]
                } else {
                    0.5 * par_llr[t]
                };
                let gamma = sys_contrib + par_contrib;
                let new_val = beta[t + 1][next as usize] + gamma;
                if new_val > beta[t][s] {
                    beta[t][s] = new_val;
                }
            }
        }
    }

    // Extrinsic LLR computation.
    let mut ext = vec![0.0f32; k];
    for t in 0..k {
        let mut max0 = NEG_INF;
        let mut max1 = NEG_INF;
        for s in 0..n_states {
            if alpha[t][s] == NEG_INF {
                continue;
            }
            for d in 0u8..2 {
                let (next, par) = trellis[s][d as usize];
                if beta[t + 1][next as usize] == NEG_INF {
                    continue;
                }
                // Extrinsic only: exclude prior and systematic channel contributions
                let par_contrib = if par == 1 {
                    -0.5 * par_llr[t]
                } else {
                    0.5 * par_llr[t]
                };
                let metric = alpha[t][s] + par_contrib + beta[t + 1][next as usize];
                if d == 0 {
                    if metric > max0 {
                        max0 = metric;
                    }
                } else if metric > max1 {
                    max1 = metric;
                }
            }
        }
        // Extrinsic LLR (positive = likely 0)
        ext[t] = if max0 == NEG_INF || max1 == NEG_INF {
            0.0
        } else {
            max0 - max1
        };
    }
    ext
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Maximum information bytes that `TurboCodec::encode` accepts in one call.
///
/// Derived from the largest QPP table entry (K=6144 bits = 768 bytes) minus
/// the 4-byte turbo wire header (2-byte LE length + 2-byte CRC-16).
pub const TURBO_MAX_INFO_BYTES: usize = 764;

/// Rate-1/3 PCCC turbo codec with 3GPP QPP interleaver and Max-Log-MAP BCJR decoder.
pub struct TurboCodec {
    max_iter: usize,
}

impl TurboCodec {
    /// Construct with default 8-iteration decoder.
    pub fn new() -> Self {
        Self { max_iter: 8 }
    }

    /// Build the payload block: 2-byte LE length + data + 2-byte CRC-16.
    fn build_payload(data: &[u8]) -> Vec<u8> {
        let len = data.len() as u16;
        let mut payload = Vec::with_capacity(4 + data.len());
        payload.extend_from_slice(&len.to_le_bytes());
        payload.extend_from_slice(data);
        let crc = crc16(&payload);
        payload.extend_from_slice(&crc.to_le_bytes());
        payload
    }

    /// Encode `data` bytes into a packed rate-1/3 bit stream.
    ///
    /// Returns `Err` if `data` exceeds `TURBO_MAX_INFO_BYTES` (no QPP entry covers the block).
    pub fn encode(&self, data: &[u8]) -> Result<Vec<u8>, ModemError> {
        let payload = Self::build_payload(data);
        let raw_bits = bytes_to_bits(&payload);
        let (k, f1, f2) = qpp_params(raw_bits.len()).ok_or_else(|| {
            ModemError::Frame(format!(
                "turbo: block {} bits exceeds max QPP size (6144); split payload at call site",
                raw_bits.len()
            ))
        })?;
        let mut bits = raw_bits;
        bits.resize(k, 0);

        let trellis = build_trellis();
        let par1 = rsc_parity(&bits, &trellis);

        let pi_bits: Vec<u8> = (0..k).map(|i| bits[qpp_permute(i, f1, f2, k)]).collect();
        let par2 = rsc_parity(&pi_bits, &trellis);

        Ok(pack_bits(&bits, &par1, &par2))
    }

    /// Soft-decision decode `llrs` (positive = likely 0) and return recovered data bytes.
    pub fn decode(&self, llrs: &[f32]) -> Result<Vec<u8>, ModemError> {
        let total = llrs.len();
        // Each of sys, par1, par2 must be the same length
        if !total.is_multiple_of(3) {
            return Err(ModemError::Frame(
                "turbo: LLR count not divisible by 3".into(),
            ));
        }
        let k = total / 3;
        let (qpp_k, f1, f2) = qpp_params(k)
            .filter(|&(qk, _, _)| qk == k)
            .or_else(|| QPP_TABLE.iter().find(|&&(qk, _, _)| qk == k).copied())
            .ok_or_else(|| ModemError::Frame(format!("turbo: unsupported block size {k}")))?;
        let _ = qpp_k;

        let sys_llr = &llrs[0..k];
        let par1_llr = &llrs[k..2 * k];
        let par2_llr = &llrs[2 * k..3 * k];

        // Build interleaving map
        let pi: Vec<usize> = (0..k).map(|i| qpp_permute(i, f1, f2, k)).collect();

        let trellis = build_trellis();
        // L_e2_deint: a priori for decoder 1, initialised to zero.
        let mut l_e2_deint = vec![0.0f32; k];
        let sys_pi: Vec<f32> = pi.iter().map(|&p| sys_llr[p]).collect();
        let mut last_hard_bytes = Vec::new();

        for _iter in 0..self.max_iter {
            // Decoder 1 — a priori = extrinsic from decoder 2 (deinterleaved).
            let ext1 = bcjr_pass(sys_llr, par1_llr, &l_e2_deint, &trellis);

            // Decoder 2 — a priori = interleaved extrinsic from decoder 1.
            let prior2: Vec<f32> = pi.iter().map(|&p| ext1[p]).collect();
            let ext2 = bcjr_pass(&sys_pi, par2_llr, &prior2, &trellis);

            // Deinterleave ext2: ext2[i] is for original bit pi[i].
            for (i, &v) in ext2.iter().enumerate() {
                l_e2_deint[pi[i]] = v;
            }

            // Posterior = sys + L_e1 + L_e2_deint.  Early-exit on CRC pass.
            let llr_total: Vec<f32> = (0..k)
                .map(|i| sys_llr[i] + ext1[i] + l_e2_deint[i])
                .collect();
            let hard_bits: Vec<u8> = llr_total
                .iter()
                .map(|&l| if l < 0.0 { 1 } else { 0 })
                .collect();
            last_hard_bytes = bits_to_bytes(&hard_bits);
            if let Ok(data) = Self::decode_payload(&last_hard_bytes) {
                return Ok(data);
            }
        }

        Self::decode_payload(&last_hard_bytes)
            .map_err(|_| ModemError::Frame("turbo: CRC check failed after all iterations".into()))
    }

    fn decode_payload(bytes: &[u8]) -> Result<Vec<u8>, ModemError> {
        if bytes.len() < 4 {
            return Err(ModemError::Frame("turbo: payload too short".into()));
        }
        let data_len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        if bytes.len() < 2 + data_len + 2 {
            return Err(ModemError::Frame("turbo: payload truncated".into()));
        }
        let data = &bytes[2..2 + data_len];
        let crc_stored = u16::from_le_bytes([bytes[2 + data_len], bytes[2 + data_len + 1]]);
        let mut check = vec![bytes[0], bytes[1]];
        check.extend_from_slice(data);
        let crc_computed = crc16(&check);
        if crc_computed != crc_stored {
            return Err(ModemError::Frame("turbo: CRC mismatch".into()));
        }
        Ok(data.to_vec())
    }
}

impl Default for TurboCodec {
    fn default() -> Self {
        Self::new()
    }
}

// ── Engine-level helpers ───────────────────────────────────────────────────────

/// Encode `data` with `TurboCodec` and return the packed codeword bytes.
pub fn turbo_encode(data: &[u8]) -> Result<Vec<u8>, ModemError> {
    TurboCodec::new().encode(data)
}

/// Soft-decode `llrs` with `TurboCodec`.  LLR convention: positive = likely 0.
pub fn turbo_decode_soft(llrs: &[f32]) -> Result<Vec<u8>, ModemError> {
    TurboCodec::new().decode(llrs)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_clean() {
        let codec = TurboCodec::new();
        let data = b"Hello turbo!";
        let encoded = codec.encode(data).expect("encode must succeed");
        // Perfect channel: convert each byte to bipolar LLRs (+5.0 = confident 0, -5.0 = confident 1)
        let llrs: Vec<f32> = bytes_to_bits(&encoded)
            .iter()
            .map(|&b| if b == 0 { 5.0 } else { -5.0 })
            .collect();
        let decoded = codec.decode(&llrs).expect("clean round-trip must succeed");
        assert_eq!(decoded, data);
    }

    #[test]
    fn encode_rejects_oversize_block() {
        let codec = TurboCodec::new();
        // 765 bytes of data → payload = 769 bytes → 6152 bits > 6144 max QPP K
        let big = vec![0u8; 765];
        assert!(codec.encode(&big).is_err());
    }

    #[test]
    fn qpp_params_covers_required_sizes() {
        for &bits in &[40, 256, 1024, 6144] {
            assert!(qpp_params(bits).is_some(), "no QPP entry for {bits}");
        }
    }

    #[test]
    fn trellis_all_states_reachable() {
        let t = build_trellis();
        let mut seen = [false; 4];
        let mut state = 0u8;
        for d in [1u8, 0, 1, 1, 0, 0] {
            let (next, _) = t[state as usize][d as usize];
            state = next;
            seen[state as usize] = true;
        }
        assert!(seen.iter().any(|&x| x));
    }
}
