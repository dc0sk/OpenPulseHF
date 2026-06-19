use std::f32::consts::PI;

use rustfft::{num_complex::Complex32, FftPlanner};

/// OFDM configuration for simulation sweep.
#[derive(Debug, Clone)]
pub struct OfdmConfig {
    /// Number of data-bearing subcarriers (excludes pilots and reserved).
    pub n_subcarriers: usize,
    /// Cyclic prefix length in samples.
    pub cp_samples: usize,
    /// Sample rate (Hz).
    pub fs: f32,
    /// Number of pilot subcarriers interleaved among data subcarriers.
    pub pilot_count: usize,
    /// Bits per data subcarrier: 1=BPSK, 2=QPSK, 4=QAM16.
    pub mod_order: usize,
}

impl OfdmConfig {
    /// OFDM symbol period in samples (FFT size = n_subcarriers, CP prepended).
    pub fn symbol_samples(&self) -> usize {
        self.n_subcarriers + self.cp_samples
    }

    /// Number of usable one-sided data subcarriers (indices 1..n/2, excluding pilots).
    fn usable_data_carriers(&self) -> usize {
        let one_sided = self.n_subcarriers / 2 - 1; // indices 1..n/2
        one_sided.saturating_sub(self.pilot_count)
    }

    /// Net bits per OFDM symbol (pilots carry no payload).
    pub fn bits_per_ofdm_symbol(&self) -> usize {
        self.usable_data_carriers() * self.mod_order
    }

    /// Gross bit rate (bps).
    pub fn gross_bps(&self) -> f32 {
        let sym_duration = self.symbol_samples() as f32 / self.fs;
        self.bits_per_ofdm_symbol() as f32 / sym_duration
    }

    /// Occupied bandwidth (Hz) — one-sided Nyquist bandwidth.
    pub fn bw_hz(&self) -> f32 {
        self.fs / 2.0
    }
}

/// Per-sweep statistics.
#[derive(Debug, Clone)]
pub struct OfdmStats {
    pub papr_db: f32,
    pub ber: f64,
    pub gross_bps: f32,
    pub bw_hz: f32,
}

// ── Constellation helpers ─────────────────────────────────────────────────────

fn bpsk_mod(bit: u8) -> Complex32 {
    if bit == 0 {
        Complex32::new(1.0, 0.0)
    } else {
        Complex32::new(-1.0, 0.0)
    }
}

fn qpsk_mod(bits: u8) -> Complex32 {
    let inv_sqrt2 = 0.70710677_f32;
    match bits & 0x3 {
        0 => Complex32::new(inv_sqrt2, inv_sqrt2),
        1 => Complex32::new(-inv_sqrt2, inv_sqrt2),
        2 => Complex32::new(inv_sqrt2, -inv_sqrt2),
        _ => Complex32::new(-inv_sqrt2, -inv_sqrt2),
    }
}

fn qam16_mod(bits: u8) -> Complex32 {
    let i = match (bits >> 2) & 0x3 {
        0 => -3.0_f32,
        1 => -1.0,
        2 => 3.0,
        _ => 1.0,
    };
    let q = match bits & 0x3 {
        0 => -3.0_f32,
        1 => -1.0,
        2 => 3.0,
        _ => 1.0,
    };
    Complex32::new(
        i / 3.0_f32.sqrt() / 3.0_f32.sqrt(),
        q / 3.0_f32.sqrt() / 3.0_f32.sqrt(),
    )
}

fn map_symbol(bits: u8, mod_order: usize) -> Complex32 {
    match mod_order {
        1 => bpsk_mod(bits),
        2 => qpsk_mod(bits),
        4 => qam16_mod(bits),
        _ => bpsk_mod(bits),
    }
}

fn nearest_bpsk(c: Complex32) -> u8 {
    if c.re >= 0.0 {
        0
    } else {
        1
    }
}

fn nearest_qpsk(c: Complex32) -> u8 {
    let i_bit = if c.re >= 0.0 { 0u8 } else { 1u8 };
    let q_bit = if c.im >= 0.0 { 0u8 } else { 1u8 };
    i_bit | (q_bit << 1)
}

fn nearest_qam16(c: Complex32) -> u8 {
    let scale = 3.0_f32.sqrt() * 3.0_f32.sqrt();
    let i = (c.re * scale).round().clamp(-3.0, 3.0) as i32;
    let q = (c.im * scale).round().clamp(-3.0, 3.0) as i32;
    let i_idx = match i {
        -3 => 0u8,
        -1 => 1,
        1 => 3,
        _ => 2,
    };
    let q_idx = match q {
        -3 => 0u8,
        -1 => 1,
        1 => 3,
        _ => 2,
    };
    (i_idx << 2) | q_idx
}

fn demap_symbol(c: Complex32, mod_order: usize) -> u8 {
    match mod_order {
        1 => nearest_bpsk(c),
        2 => nearest_qpsk(c),
        4 => nearest_qam16(c),
        _ => nearest_bpsk(c),
    }
}

// ── Frame encode/decode ───────────────────────────────────────────────────────

/// Encode `payload` bytes into real-valued OFDM time-domain samples.
///
/// Uses Hermitian symmetry so the IFFT output is purely real: data subcarriers
/// are placed at positive-frequency bins 1..fft_size/2; bins fft_size/2+1..N-1
/// are set to the complex conjugate of the corresponding positive-frequency bin.
/// DC (bin 0) and Nyquist (bin fft_size/2) are zeroed.
///
/// Pilot subcarriers are inserted at regular intervals within the positive
/// half and carry a known real BPSK +1 symbol.
pub fn generate_ofdm_frame(cfg: &OfdmConfig, payload: &[u8]) -> Vec<f32> {
    let fft_size = cfg.n_subcarriers;
    let half = fft_size / 2; // usable positive-frequency range: 1..half
    let data_carriers = cfg.usable_data_carriers();
    let bits_per_sym = cfg.mod_order;
    let bits_needed = payload.len() * 8;
    let bits_per_ofdm = if data_carriers > 0 {
        data_carriers * bits_per_sym
    } else {
        1
    };
    let n_ofdm_syms = bits_needed.div_ceil(bits_per_ofdm);

    let mut planner = FftPlanner::new();
    let ifft = planner.plan_fft_inverse(fft_size);

    let mut out = Vec::with_capacity(n_ofdm_syms * (fft_size + cfg.cp_samples));
    let bits = bytes_to_bits(payload);
    let mut bit_idx = 0usize;

    for _ in 0..n_ofdm_syms {
        let mut freq = vec![Complex32::new(0.0, 0.0); fft_size];

        // Fill positive-frequency bins 1..half with data/pilots.
        // bin 0 (DC) and bin half (Nyquist) remain zero.
        for sc in 1..half {
            let pos = sc - 1; // 0-indexed position within positive half
            let sym = if is_pilot_one_sided(pos, cfg.pilot_count, half - 1) {
                Complex32::new(1.0, 0.0) // known pilot
            } else {
                let mut sym_bits = 0u8;
                for b in 0..bits_per_sym {
                    if bit_idx < bits.len() {
                        sym_bits |= (bits[bit_idx] as u8) << b;
                        bit_idx += 1;
                    }
                }
                map_symbol(sym_bits, bits_per_sym)
            };
            freq[sc] = sym;
            // Hermitian symmetry: negative-frequency mirror.
            freq[fft_size - sc] = sym.conj();
        }

        // IFFT — output is purely real due to Hermitian symmetry.
        ifft.process(&mut freq);
        // Normalise by 1/sqrt(N); the IFFT output will be real-valued.
        let scale = 1.0 / (fft_size as f32).sqrt();
        let time: Vec<f32> = freq.iter().map(|c| c.re * scale).collect();

        // Prepend cyclic prefix.
        let cp_start = fft_size.saturating_sub(cfg.cp_samples);
        out.extend_from_slice(&time[cp_start..]);
        out.extend_from_slice(&time);
    }

    out
}

/// Decode real-valued OFDM samples back to payload bytes (best-effort, no FEC).
///
/// Reads only the positive-frequency half (bins 1..fft_size/2) of the FFT
/// to match the Hermitian-symmetric transmitter.  Both TX and RX apply the
/// same `1/sqrt(N)` normalisation, so the FFT/IFFT round-trip recovers the
/// original frequency-domain symbols exactly.
pub fn demodulate_ofdm_frame(samples: &[f32], cfg: &OfdmConfig) -> Vec<u8> {
    let fft_size = cfg.n_subcarriers;
    let half = fft_size / 2;
    let sym_len = fft_size + cfg.cp_samples;
    let data_carriers = cfg.usable_data_carriers();
    let bits_per_sym = cfg.mod_order;
    let bits_per_ofdm = if data_carriers > 0 {
        data_carriers * bits_per_sym
    } else {
        1
    };

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let n_ofdm_syms = samples.len() / sym_len;
    let mut bits: Vec<bool> = Vec::with_capacity(n_ofdm_syms * bits_per_ofdm);

    let scale = 1.0 / (fft_size as f32).sqrt();
    for sym_idx in 0..n_ofdm_syms {
        let start = sym_idx * sym_len + cfg.cp_samples; // strip CP
        if start + fft_size > samples.len() {
            break;
        }
        let mut freq: Vec<Complex32> = samples[start..start + fft_size]
            .iter()
            .map(|&s| Complex32::new(s * scale, 0.0))
            .collect();

        fft.process(&mut freq);

        // Decode only positive frequencies 1..half.
        for (pos, sym) in freq[1..half].iter().enumerate() {
            if is_pilot_one_sided(pos, cfg.pilot_count, half - 1) {
                continue;
            }
            let sym_bits = demap_symbol(*sym, bits_per_sym);
            for b in 0..bits_per_sym {
                bits.push((sym_bits >> b) & 1 == 1);
            }
        }
    }

    bits_to_bytes(&bits)
}

/// Measure PAPR (peak-to-average power ratio) in dB.
pub fn measure_papr(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let peak_sq = samples.iter().map(|&s| s * s).fold(0.0_f32, f32::max);
    let mean_sq = samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32;
    if mean_sq < 1e-12 {
        return 0.0;
    }
    10.0 * (peak_sq / mean_sq).log10()
}

// ── PAPR reduction techniques ─────────────────────────────────────────────────

/// Clip-and-filter PAPR reduction.
///
/// Clips at `rms × 10^(clip_db/20)` then applies a simple rectangular
/// bandpass to remove out-of-band spectral regrowth.
pub fn clip_and_filter(samples: &[f32], clip_db: f32) -> Vec<f32> {
    if samples.is_empty() {
        return vec![];
    }
    let rms = (samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    let threshold = rms * 10.0_f32.powf(clip_db / 20.0);

    // Clip.
    let clipped: Vec<f32> = samples
        .iter()
        .map(|&s| s.clamp(-threshold, threshold))
        .collect();

    // Simple moving-average (5-tap) to smooth clipping discontinuities.
    let n = clipped.len();
    let mut filtered = vec![0.0f32; n];
    for (i, val) in filtered.iter_mut().enumerate() {
        let lo = i.saturating_sub(2);
        let hi = (i + 3).min(n);
        let sum: f32 = clipped[lo..hi].iter().sum();
        *val = sum / (hi - lo) as f32;
    }
    filtered
}

/// Iterative clip-and-filter PAPR reduction.
///
/// Clips repeatedly until `peak/RMS ≤ 10^(target_papr_db/20)` or `max_iter`
/// is reached.  Guaranteed to converge because each clip is non-expansive:
/// energy is monotonically non-increasing, so the threshold is non-increasing
/// and the signal eventually lies fully within `[-T, T]`.
pub fn clip_iterative(samples: &[f32], target_papr_db: f32, max_iter: usize) -> Vec<f32> {
    if samples.is_empty() {
        return vec![];
    }
    let mut signal = samples.to_vec();
    let factor = 10.0_f32.powf(target_papr_db / 20.0);
    for _ in 0..max_iter {
        let mean_sq = signal.iter().map(|&s| s * s).sum::<f32>() / signal.len() as f32;
        let rms = mean_sq.sqrt();
        let threshold = rms * factor;
        let peak = signal.iter().map(|&s| s.abs()).fold(0.0_f32, f32::max);
        if peak <= threshold * 1.001 {
            break;
        }
        signal = signal
            .iter()
            .map(|&s| s.clamp(-threshold, threshold))
            .collect();
    }
    signal
}

///
/// Reserves the first and last `n_reserved / 2` subcarriers as PAPR-reduction
/// tones.  Uses a simple iterative peak-cancellation algorithm.
pub fn tone_reservation(cfg: &OfdmConfig, samples: &[f32], n_reserved: usize) -> Vec<f32> {
    if samples.is_empty() || n_reserved == 0 {
        return samples.to_vec();
    }
    let fft_size = cfg.n_subcarriers;
    let sym_len = fft_size + cfg.cp_samples;
    let n_syms = samples.len() / sym_len;

    let mut planner = FftPlanner::new();
    let ifft = planner.plan_fft_inverse(fft_size);

    let half = n_reserved / 2;
    let scale_inv = 1.0 / (fft_size as f32).sqrt();

    let mut out = samples.to_vec();

    for sym_idx in 0..n_syms {
        let base = sym_idx * sym_len;
        let data_start = base + cfg.cp_samples;
        if data_start + fft_size > out.len() {
            break;
        }

        // 50 iterations of peak-cancellation.
        for _iter in 0..50 {
            // Find peak sample in the OFDM symbol body.
            let body = &out[data_start..data_start + fft_size];
            let (peak_idx, &peak_val) = body
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
                .unwrap_or((0, &0.0));

            let rms = (body.iter().map(|&s| s * s).sum::<f32>() / fft_size as f32).sqrt();
            if peak_val.abs() < rms * 1.4 {
                // Below ~3 dB above RMS — good enough.
                break;
            }

            // Build a cancellation signal using only reserved tones.
            // The cancellation tone is an impulse at `peak_idx` in time domain.
            let target = peak_val * 0.3; // partial cancellation per iteration
            let mut tone_freq = vec![Complex32::new(0.0, 0.0); fft_size];
            // Synthesise a time-domain impulse at peak_idx via all reserved bins.
            for k in 0..half {
                let phase = -2.0 * PI * k as f32 * peak_idx as f32 / fft_size as f32;
                tone_freq[k] =
                    Complex32::new(target * phase.cos(), target * phase.sin()) / half as f32;
                let k2 = fft_size - 1 - k;
                let phase2 = -2.0 * PI * k2 as f32 * peak_idx as f32 / fft_size as f32;
                tone_freq[k2] =
                    Complex32::new(target * phase2.cos(), target * phase2.sin()) / half as f32;
            }

            ifft.process(&mut tone_freq);
            let cancel: Vec<f32> = tone_freq.iter().map(|c| c.re * scale_inv).collect();

            // Subtract cancellation from symbol body and CP.
            for i in 0..fft_size {
                out[data_start + i] -= cancel[i];
                // Update CP (last cp_samples of body map to start of CP region).
                if cfg.cp_samples > 0 && i + cfg.cp_samples >= fft_size {
                    let cp_idx = i + cfg.cp_samples - fft_size;
                    if cp_idx < cfg.cp_samples {
                        out[base + cp_idx] -= cancel[i];
                    }
                }
            }
        }
    }

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_pilot(sc: usize, pilot_count: usize, fft_size: usize) -> bool {
    if pilot_count == 0 {
        return false;
    }
    let spacing = fft_size / (pilot_count + 1);
    if spacing == 0 {
        return sc < pilot_count;
    }
    sc > 0 && sc.is_multiple_of(spacing) && (sc / spacing) <= pilot_count
}

/// Pilot detection for one-sided (positive-frequency) subcarrier indexing.
///
/// `pos` is 0-indexed position within the positive half (0 = bin 1, etc.).
/// `n_pos` is the total number of positive-frequency subcarriers (half - 1).
fn is_pilot_one_sided(pos: usize, pilot_count: usize, n_pos: usize) -> bool {
    is_pilot(pos + 1, pilot_count, n_pos + 1)
}

fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for shift in 0..8u8 {
            bits.push((b >> shift) & 1 == 1);
        }
    }
    bits
}

fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|c| {
            c.iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | ((b as u8) << i))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cfg() -> OfdmConfig {
        OfdmConfig {
            n_subcarriers: 16,
            cp_samples: 4,
            fs: 8000.0,
            pilot_count: 2,
            mod_order: 2,
        }
    }

    #[test]
    fn ofdm_clean_loopback_qpsk() {
        let cfg = base_cfg();
        let payload = b"OFDM test";
        let samples = generate_ofdm_frame(&cfg, payload);
        let rx = demodulate_ofdm_frame(&samples, &cfg);
        assert_eq!(&rx[..payload.len()], payload);
    }

    #[test]
    fn measure_papr_returns_reasonable_value() {
        let cfg = base_cfg();
        let samples = generate_ofdm_frame(&cfg, b"PAPR test payload");
        let papr = measure_papr(&samples);
        // OFDM PAPR is typically 8–12 dB; accept a wide range for this test.
        assert!(papr > 0.0 && papr < 30.0, "unexpected PAPR: {papr}");
    }

    #[test]
    fn clip_iterative_reduces_papr_to_target() {
        let cfg = base_cfg();
        let samples = generate_ofdm_frame(&cfg, b"clip test payload");
        let papr_before = measure_papr(&samples);
        let clipped = clip_iterative(&samples, 4.0, 50);
        let papr_after = measure_papr(&clipped);
        assert!(
            papr_after <= papr_before,
            "iterative clip raised PAPR: before={papr_before:.1} after={papr_after:.1}"
        );
        assert!(
            papr_after <= 4.1,
            "iterative clip did not converge to target: {papr_after:.1} dB"
        );
    }
}
