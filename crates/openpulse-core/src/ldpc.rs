//! Systematic LDPC codec with two presets:
//! - [`LdpcCodec::new`] — rate-1/2 (k=1024, n=2048), H_s built from xorshift32.
//! - [`LdpcCodec::high_rate`] — rate ≈8/9 (k=1024, n=1152), H_s built by
//!   Progressive Edge-Growth (PEG) for girth (a random H_s at this rate is
//!   useless); waterfall ≈4 dB Es/N0.
//!
//! Both keep the systematic `H = [H_s | I_m]` structure, so encoding is one XOR
//! pass and decoding runs min-sum belief propagation for up to 50 iterations.

use crate::error::ModemError;

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Shared interface for iterative FEC codecs (LDPC, Turbo).
///
/// All methods operate on whole blocks; callers must split larger payloads.
pub trait IterativeDecoder: Send + Sync {
    /// Encode `data` bytes and return the codeword (data + parity).
    fn encode(&self, data: &[u8]) -> Vec<u8>;

    /// Soft-decision decode `llrs` (one `f32` per coded bit, positive = likely 0)
    /// and return the recovered data bytes.
    ///
    /// Returns `Err` if the decoder fails to converge within `max_iterations()`.
    fn decode_soft(&self, llrs: &[f32]) -> Result<Vec<u8>, ModemError>;

    /// Maximum belief-propagation (or BCJR) iterations before declaring failure.
    fn max_iterations(&self) -> u32;

    /// Size of one information block in bits (before encoding).
    fn block_bits(&self) -> usize;
}

// ── PRNG ──────────────────────────────────────────────────────────────────────

fn xorshift32(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

// ── Public size constants ─────────────────────────────────────────────────────

/// Maximum number of information bytes per LDPC block (k = 1024 bits = 128 bytes).
///
/// Callers of `ModemEngine::transmit_with_ldpc` must ensure the *encoded frame*
/// (HPX header + payload + CRC, as returned by `stage_encode_frame`) does not
/// exceed this value.  Typical HPX frame overhead is 8–10 bytes, leaving
/// ~118–120 bytes of usable user payload per call.
pub const LDPC_MAX_INFO_BYTES: usize = 128;

/// Number of coded bits per LDPC codeword (n = 2048 bits = 256 bytes).
pub const LDPC_CODEWORD_BYTES: usize = 256;

// ── LdpcCodec ────────────────────────────────────────────────────────────────

/// Rate-1/2 LDPC codec: 1024 info bits, 2048 codeword bits.
///
/// H = [H_s | I_m] where H_s is a regular 1024×1024 matrix with variable
/// degree d_v=3, constructed deterministically from xorshift32.  The identity
/// block I_m makes encoding a single XOR pass and gives each parity bit a
/// degree-1 check connection that anchors BP convergence.
pub struct LdpcCodec {
    k: usize,
    m: usize,
    /// For each check c: variable indices connected to it.
    /// Info vars from H_s come first; the last entry is always k+c (I_m).
    check_to_vars: Vec<Vec<usize>>,
}

impl Default for LdpcCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl LdpcCodec {
    /// Construct the codec.  H is built once at construction; no I/O.
    pub fn new() -> Self {
        const K: usize = 1024;
        const M: usize = 1024;
        const DV: usize = 3;

        let mut check_to_vars_info: Vec<Vec<usize>> = vec![Vec::new(); M];

        let mut state = 0xDEAD_BEEFu32;
        for v in 0..K {
            let mut chosen: Vec<usize> = Vec::with_capacity(DV);
            while chosen.len() < DV {
                let c = (xorshift32(&mut state) as usize) % M;
                if !chosen.contains(&c) {
                    chosen.push(c);
                    check_to_vars_info[c].push(v);
                }
            }
        }

        // Append the parity variable (k+c) to each check's variable list.
        let check_to_vars: Vec<Vec<usize>> = (0..M)
            .map(|c| {
                let mut vars = check_to_vars_info[c].clone();
                vars.push(K + c);
                vars
            })
            .collect();

        Self {
            k: K,
            m: M,
            check_to_vars,
        }
    }

    /// High-rate codec: k = 1024 info bits, m = 128 parity bits → n = 1152, rate
    /// ≈ 8/9, for the dense higher-SNR rungs (8PSK / 16QAM / 32APSK).
    ///
    /// The info-part Tanner graph is built by **Progressive Edge-Growth (PEG)** to
    /// maximise girth — a random H_s at this rate corrects barely one error,
    /// whereas the PEG graph corrects a useful spread.  It keeps the systematic
    /// `[H_s | I_m]` structure, so encoding stays a single XOR pass and the
    /// min-sum decoder is reused unchanged.  The info block is ≤
    /// `LDPC_MAX_INFO_BYTES`, so it fits the engine's single-block LDPC dispatch.
    pub fn high_rate() -> Self {
        Self::with_peg(1024, 128, 3)
    }

    /// Construct a codec with `k` info bits, `m` parity bits, variable-node degree
    /// `dv`, and a PEG-built info-part graph (systematic `[H_s | I_m]`).  `k` and
    /// `m` must be multiples of 8.
    pub fn with_peg(k: usize, m: usize, dv: usize) -> Self {
        Self {
            k,
            m,
            check_to_vars: peg_check_to_vars(k, m, dv),
        }
    }

    /// Code rate `k / (k + m)`.
    pub fn code_rate(&self) -> f32 {
        self.k as f32 / (self.k + self.m) as f32
    }

    /// Information block size in bytes (`k / 8`).
    pub fn info_bytes(&self) -> usize {
        self.k / 8
    }

    /// Codeword size in bytes (`(k + m) / 8`).
    pub fn codeword_bytes(&self) -> usize {
        (self.k + self.m) / 8
    }

    /// Number of information bits `k`.
    pub fn k(&self) -> usize {
        self.k
    }

    /// Number of parity-check rows `m`.
    pub fn m(&self) -> usize {
        self.m
    }

    /// Tanner-graph adjacency: for each check, the variable indices it connects to.
    pub fn check_to_vars(&self) -> &[Vec<usize>] {
        &self.check_to_vars
    }
}

/// Progressive Edge-Growth construction of the info-part Tanner graph.
///
/// Places each info variable's `dv` edges one at a time, choosing the check that
/// is farthest from the variable in the current graph (BFS), breaking ties toward
/// the lowest-degree check — maximising the local girth.  Returns `check_to_vars`
/// with the degree-1 I_m parity variable (`k + c`) appended to each check, so the
/// result drops straight into the existing systematic encode/decode.
fn peg_check_to_vars(k: usize, m: usize, dv: usize) -> Vec<Vec<usize>> {
    let dv = dv.min(m.max(1));
    let mut var_to_checks: Vec<Vec<usize>> = vec![Vec::new(); k];
    let mut check_to_info: Vec<Vec<usize>> = vec![Vec::new(); m];

    for v in 0..k {
        for _ in 0..dv {
            // BFS from v over the current bipartite graph, tracking reached checks.
            let mut reached = vec![false; m];
            let mut visited_var = vec![false; k];
            visited_var[v] = true;
            let mut frontier = vec![v];
            let mut last_level: Vec<usize> = Vec::new();
            let mut reached_count = 0usize;
            loop {
                let mut next_checks: Vec<usize> = Vec::new();
                for &u in &frontier {
                    for &c in &var_to_checks[u] {
                        if !reached[c] {
                            reached[c] = true;
                            reached_count += 1;
                            next_checks.push(c);
                        }
                    }
                }
                if next_checks.is_empty() {
                    break; // graph saturated from v
                }
                last_level = next_checks.clone();
                if reached_count >= m {
                    break; // every check reachable
                }
                let mut next_vars: Vec<usize> = Vec::new();
                for &c in &next_checks {
                    for &u in &check_to_info[c] {
                        if !visited_var[u] {
                            visited_var[u] = true;
                            next_vars.push(u);
                        }
                    }
                }
                if next_vars.is_empty() {
                    break;
                }
                frontier = next_vars;
            }

            // Candidates: unreached checks (largest new girth) if any, else the
            // deepest BFS level; never a check already on v (no duplicate edges).
            let mut candidates: Vec<usize> = if reached_count < m {
                (0..m).filter(|c| !reached[*c]).collect()
            } else {
                last_level
            };
            candidates.retain(|c| !var_to_checks[v].contains(c));
            let chosen = candidates
                .into_iter()
                .min_by_key(|&c| (check_to_info[c].len(), c))
                .or_else(|| (0..m).find(|c| !var_to_checks[v].contains(c)))
                .unwrap_or(0);
            var_to_checks[v].push(chosen);
            check_to_info[chosen].push(v);
        }
    }

    (0..m)
        .map(|c| {
            let mut vars = check_to_info[c].clone();
            vars.push(k + c);
            vars
        })
        .collect()
}

impl IterativeDecoder for LdpcCodec {
    fn encode(&self, data: &[u8]) -> Vec<u8> {
        let k = self.k;
        let m = self.m;

        // Unpack info bytes into k bits (LSB-first per byte).
        let mut info = vec![false; k];
        for (bi, &byte) in data.iter().take(k / 8).enumerate() {
            for bit in 0..8usize {
                info[bi * 8 + bit] = (byte >> bit) & 1 == 1;
            }
        }

        // p[c] = XOR of info bits connected to check c via H_s.
        // vars[..len-1] are the info vars; vars[len-1] = k+c is the parity var itself.
        let mut parity = vec![false; m];
        for (c, vars) in self.check_to_vars.iter().enumerate() {
            for &v in &vars[..vars.len() - 1] {
                parity[c] ^= info[v];
            }
        }

        // Pack codeword (info ++ parity) into bytes.
        let n = k + m;
        let mut out = vec![0u8; n / 8];
        for (i, &b) in info.iter().chain(parity.iter()).enumerate() {
            if b {
                out[i / 8] |= 1u8 << (i % 8);
            }
        }
        out
    }

    fn decode_soft(&self, llrs: &[f32]) -> Result<Vec<u8>, ModemError> {
        let k = self.k;
        let n = k + self.m;

        if llrs.len() < n {
            return Err(ModemError::Fec(format!(
                "LDPC: expected {n} LLRs, got {}",
                llrs.len()
            )));
        }
        let ch = &llrs[..n];

        // c2v[c][i] = current check-to-variable message (initialised to 0).
        let mut c2v: Vec<Vec<f32>> = self
            .check_to_vars
            .iter()
            .map(|vars| vec![0.0f32; vars.len()])
            .collect();

        let mut total = vec![0.0f32; n];

        for _ in 0..self.max_iterations() {
            // Accumulate total LLR per variable: channel + all incoming checks.
            total.copy_from_slice(ch);
            for (c, vars) in self.check_to_vars.iter().enumerate() {
                for (i, &v) in vars.iter().enumerate() {
                    total[v] += c2v[c][i];
                }
            }

            // Check syndrome; return on convergence.
            let bits: Vec<bool> = total.iter().map(|&l| l < 0.0).collect();
            if syndrome_ok(&bits, &self.check_to_vars) {
                return Ok(pack_bits(&bits[..k]));
            }

            // Min-sum check → variable update.
            for (c, vars) in self.check_to_vars.iter().enumerate() {
                // Extrinsic v→c: subtract this check's own prior contribution.
                let ext: Vec<f32> = vars
                    .iter()
                    .zip(&c2v[c])
                    .map(|(&v, &msg)| total[v] - msg)
                    .collect();

                for (i, msg) in c2v[c].iter_mut().enumerate() {
                    let mut prod_sign = 1.0f32;
                    let mut min_abs = f32::INFINITY;
                    for (j, &e) in ext.iter().enumerate() {
                        if j == i {
                            continue;
                        }
                        prod_sign *= if e >= 0.0 { 1.0 } else { -1.0 };
                        if e.abs() < min_abs {
                            min_abs = e.abs();
                        }
                    }
                    *msg = prod_sign * min_abs;
                }
            }
        }

        Err(ModemError::Fec("LDPC did not converge".into()))
    }

    fn max_iterations(&self) -> u32 {
        50
    }

    fn block_bits(&self) -> usize {
        self.k
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn syndrome_ok(bits: &[bool], check_to_vars: &[Vec<usize>]) -> bool {
    // A valid codeword satisfies every check: XOR of connected bits = 0 (false).
    check_to_vars
        .iter()
        .all(|vars| !vars.iter().fold(false, |acc, &v| acc ^ bits[v]))
}

fn pack_bits(bits: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; bits.len().div_ceil(8)];
    for (i, &b) in bits.iter().enumerate() {
        if b {
            out[i / 8] |= 1u8 << (i % 8);
        }
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_codec() -> LdpcCodec {
        LdpcCodec::new()
    }

    #[test]
    fn ldpc_metadata() {
        let c = make_codec();
        assert_eq!(c.max_iterations(), 50);
        assert_eq!(c.block_bits(), 1024);
    }

    #[test]
    fn ldpc_encode_output_size() {
        let c = make_codec();
        // k=1024 bits → 128 bytes in; n=2048 bits → 256 bytes out.
        let data = vec![0xA5u8; 128];
        let cw = c.encode(&data);
        assert_eq!(cw.len(), 256);
    }

    #[test]
    fn ldpc_encode_systematic_prefix() {
        let c = make_codec();
        let data: Vec<u8> = (0u8..128).collect();
        let cw = c.encode(&data);
        // First 128 bytes of codeword must equal the info bytes (systematic).
        assert_eq!(&cw[..128], &data[..]);
    }

    #[test]
    fn ldpc_syndrome_of_valid_codeword_is_zero() {
        let c = make_codec();
        let data: Vec<u8> = (0u8..128).collect();
        let cw = c.encode(&data);
        // Decode the codeword bits into booleans.
        let bits: Vec<bool> = cw
            .iter()
            .flat_map(|&b| (0..8u8).map(move |i| (b >> i) & 1 == 1))
            .collect();
        assert!(
            syndrome_ok(&bits, &c.check_to_vars),
            "syndrome must be zero"
        );
    }

    #[test]
    fn ldpc_clean_loopback() {
        let c = make_codec();
        let data: Vec<u8> = (0u8..128).collect();
        let cw = c.encode(&data);

        // Build noiseless LLRs: bit=0 → +10.0, bit=1 → -10.0.
        let llrs: Vec<f32> = cw
            .iter()
            .flat_map(|&b| {
                (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 10.0f32 } else { -10.0f32 })
            })
            .collect();

        let decoded = c
            .decode_soft(&llrs)
            .expect("should converge on clean input");
        assert_eq!(decoded, data);
    }

    #[test]
    fn ldpc_wrong_llr_count_returns_err() {
        let c = make_codec();
        let err = c.decode_soft(&[1.0f32; 100]).unwrap_err();
        assert!(matches!(err, ModemError::Fec(_)));
    }

    #[test]
    fn ldpc_var_node_degree_is_three() {
        // Every info variable must appear in exactly 3 check rows (d_v = 3).
        // Verify by scanning check_to_vars (excluding the trailing parity var).
        let c = make_codec();
        let mut degree = vec![0usize; c.k];
        for vars in &c.check_to_vars {
            for &v in &vars[..vars.len() - 1] {
                degree[v] += 1;
            }
        }
        for (v, &d) in degree.iter().enumerate() {
            assert_eq!(d, 3, "info variable {v} should have degree 3, got {d}");
        }
    }

    #[test]
    fn ldpc_decode_invariant_to_global_llr_scaling() {
        // Min-sum BP hard decisions depend only on LLR signs and *relative*
        // magnitudes: scaling every channel LLR by a positive constant scales
        // `total`, the extrinsics, and the min-abs check messages by the same
        // factor, leaving every sign-based syndrome check (and thus the whole
        // convergence trajectory) unchanged. This pins the verified finding that
        // σ²/noise-variance normalisation of the LLRs is a *no-op* for this
        // decoder on a homogeneous channel — it would only matter for a
        // scale-sensitive decoder (tanh sum-product) or when combining LLRs of
        // differing reliability. Guards against silently losing that property.
        let c = make_codec();
        let data: Vec<u8> = (0u8..128).collect();
        let cw = c.encode(&data);

        let mut llrs: Vec<f32> = cw
            .iter()
            .flat_map(|&b| (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 5.0f32 } else { -5.0f32 }))
            .collect();
        // Single parity-bit flip that BP corrects (cf. the test below).
        llrs[1024] = -llrs[1024];

        let baseline = c.decode_soft(&llrs).expect("baseline decode converges");
        assert_eq!(baseline, data);

        for scale in [0.1f32, 10.0, 1000.0] {
            let scaled: Vec<f32> = llrs.iter().map(|&l| l * scale).collect();
            let got = c
                .decode_soft(&scaled)
                .expect("scaled decode should also converge");
            assert_eq!(
                got, baseline,
                "min-sum decode must be invariant to global LLR scale {scale}"
            );
        }
    }

    #[test]
    fn ldpc_parity_bit_corrected_on_single_flip() {
        let c = make_codec();
        let data: Vec<u8> = (0u8..128).collect();
        let cw = c.encode(&data);

        // Build LLRs with a single parity-bit flip (flip bit 1024 = first parity bit).
        let mut llrs: Vec<f32> = cw
            .iter()
            .flat_map(|&b| (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 5.0f32 } else { -5.0f32 }))
            .collect();
        // Negate bit 1024's LLR to flip it.
        llrs[1024] = -llrs[1024];

        let decoded = c
            .decode_soft(&llrs)
            .expect("should correct single parity flip");
        assert_eq!(decoded, data);
    }

    // ── High-rate (PEG) codec ──────────────────────────────────────────────

    /// xorshift64 → uniform [0,1); deterministic for reproducible AWGN tests.
    fn u01(s: &mut u64) -> f32 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        ((*s >> 11) as f32) / ((1u64 << 53) as f32)
    }

    fn gauss(s: &mut u64) -> f32 {
        let u1 = u01(s).max(1e-9);
        let u2 = u01(s);
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
    }

    #[test]
    fn high_rate_dims_and_rate() {
        let c = LdpcCodec::high_rate();
        assert_eq!(c.info_bytes(), 128); // k = 1024 info bits
        assert_eq!(c.codeword_bytes(), 144); // n = 1152 (m = 128 parity)
        assert_eq!(c.block_bits(), 1024);
        assert!(
            (c.code_rate() - 8.0 / 9.0).abs() < 0.01,
            "rate {} should be ~8/9",
            c.code_rate()
        );
    }

    #[test]
    fn high_rate_clean_loopback() {
        let c = LdpcCodec::high_rate();
        let data: Vec<u8> = (0u8..128).collect();
        let cw = c.encode(&data);
        assert_eq!(cw.len(), 144);
        let llrs: Vec<f32> = cw
            .iter()
            .flat_map(|&b| {
                (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 10.0f32 } else { -10.0f32 })
            })
            .collect();
        assert_eq!(c.decode_soft(&llrs).unwrap(), data);
    }

    #[test]
    fn high_rate_soft_awgn_coding_gain() {
        // The meaningful LDPC metric is soft-decision performance. The PEG rate-8/9
        // code has a sharp waterfall around 4 dB Es/N0; at 5 dB it decodes every
        // frame (well above its threshold), where the uncoded stream still errs.
        let c = LdpcCodec::high_rate();
        let esn0_db = 5.0f32;
        let snr = 10f32.powf(esn0_db / 10.0);
        let sigma = (1.0 / (2.0 * snr)).sqrt();
        let mut s = 0xABCDu64;
        let trials = 40;
        let mut frame_ok = 0usize;
        for _ in 0..trials {
            let data: Vec<u8> = (0..c.info_bytes())
                .map(|_| (u01(&mut s) * 256.0) as u8)
                .collect();
            let cw = c.encode(&data);
            let llrs: Vec<f32> = cw
                .iter()
                .flat_map(|&b| (0..8u8).map(move |i| (b >> i) & 1))
                .map(|bit| {
                    let x = if bit == 0 { 1.0 } else { -1.0 };
                    let y = x + sigma * gauss(&mut s);
                    2.0 * y / (sigma * sigma)
                })
                .collect();
            if c.decode_soft(&llrs).map(|d| d == data).unwrap_or(false) {
                frame_ok += 1;
            }
        }
        let rate = frame_ok as f32 / trials as f32;
        assert!(
            rate >= 0.9,
            "high-rate PEG code should clear ~all frames at 5 dB Es/N0; got {:.0}%",
            rate * 100.0
        );
    }
}
