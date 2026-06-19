//! Pilot layout, LS channel estimation, ZF equalization, and CFO estimation.

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::params::{
    OfdmParams, CP, FFT_SIZE, PILOT_AMPLITUDE, PILOT_SPACING, SAMPLE_RATE, SYM_LEN,
};

/// Return the absolute SC indices of all pilot subcarriers for `p`.
///
/// Pilots are placed at `first_sc + PILOT_SPACING - 1`, then every
/// `PILOT_SPACING` SCs thereafter, staying within `[first_sc, last_sc]`.
pub fn pilot_positions(p: &OfdmParams) -> Vec<usize> {
    let mut pilots = Vec::with_capacity(p.n_pilots);
    let mut sc = p.first_sc + PILOT_SPACING - 1;
    while sc <= p.last_sc {
        pilots.push(sc);
        sc += PILOT_SPACING;
    }
    pilots
}

/// `true` when absolute SC index `sc` is a pilot for this mode.
pub fn is_pilot(p: &OfdmParams, sc: usize) -> bool {
    if sc < p.first_sc || sc > p.last_sc {
        return false;
    }
    let offset = sc - p.first_sc;
    offset % PILOT_SPACING == (PILOT_SPACING - 1)
}

/// Least-squares channel estimate at each pilot SC, then linearly interpolated
/// across all data SCs in `[first_sc, last_sc]`.
///
/// Returns a vector of complex channel estimates indexed by absolute SC
/// position relative to `first_sc` (i.e., `h[0]` = estimate for SC
/// `first_sc`, `h[total_sc-1]` = estimate for SC `last_sc`).
pub fn ls_estimate(p: &OfdmParams, freq: &[Complex32]) -> Vec<Complex32> {
    let total = p.total_sc();
    let pilots = pilot_positions(p);

    // LS estimates at pilot positions: H_est[k] = Y[k] / X_pilot[k].
    // Pilot symbols are real BPSK +1, so division is trivial.
    let known: Vec<(usize, Complex32)> = pilots
        .iter()
        .map(|&sc| {
            let h = freq[sc] / Complex32::new(PILOT_AMPLITUDE, 0.0);
            (sc - p.first_sc, h)
        })
        .collect();

    // Clamp to at least one estimate to avoid empty iteration.
    if known.is_empty() {
        return vec![Complex32::new(1.0, 0.0); total];
    }

    let mut h_est = vec![Complex32::new(1.0, 0.0); total];

    // Fill edges by extending the nearest pilot estimate as a constant.
    let (first_pilot_rel, first_h) = known[0];
    let (last_pilot_rel, last_h) = *known.last().unwrap();

    for h in h_est[..first_pilot_rel].iter_mut() {
        *h = first_h;
    }
    for h in h_est[(last_pilot_rel + 1)..].iter_mut() {
        *h = last_h;
    }

    // Linear interpolation between adjacent pilot pairs.
    for window in known.windows(2) {
        let (rel0, h0) = window[0];
        let (rel1, h1) = window[1];
        h_est[rel0] = h0;
        h_est[rel1] = h1;
        if rel1 > rel0 + 1 {
            let steps = (rel1 - rel0) as f32;
            for (i, h) in h_est[(rel0 + 1)..rel1].iter_mut().enumerate() {
                let t = (i + 1) as f32 / steps;
                *h = h0 * (1.0 - t) + h1 * t;
            }
        }
    }
    // Set the known pilot positions.
    for (rel, h) in &known {
        h_est[*rel] = *h;
    }

    h_est
}

/// Remove the dominant linear phase ramp across subcarriers, in place.
///
/// A residual symbol-timing offset of `δ` samples imprints a phase slope of
/// `−2πδ/N` rad per subcarrier on the FFT output.  Linearly interpolating the
/// channel estimate between sparse pilots across such a ramp is lossy, so we
/// first estimate the slope from adjacent pilot pairs and de-rotate every bin.
/// The residual (channel magnitude ripple) is then smooth enough for the
/// existing pilot interpolation.  On a flat, perfectly-timed channel the slope
/// is ≈ 0 and this is a no-op.
pub fn deramp_timing(p: &OfdmParams, freq: &mut [Complex32]) {
    let pilots = pilot_positions(p);
    if pilots.len() < 2 {
        return;
    }
    // Pilots are real BPSK +1, evenly spaced by `PILOT_SPACING`.  The vector sum
    // of adjacent conjugate products yields the average per-pilot-step rotation.
    let mut acc = Complex32::new(0.0, 0.0);
    for w in pilots.windows(2) {
        acc += freq[w[1]] * freq[w[0]].conj();
    }
    if acc.norm_sqr() < 1e-12 {
        return;
    }
    let slope = acc.arg() / PILOT_SPACING as f32; // rad per subcarrier
    let k_ref = pilots[0] as f32;
    for (k, c) in freq.iter_mut().enumerate() {
        let (sin_p, cos_p) = (-slope * (k as f32 - k_ref)).sin_cos();
        *c *= Complex32::new(cos_p, sin_p);
    }
}

/// Zero-forcing equalization: divide each data SC bin by its channel estimate.
///
/// `freq` is the full FFT output (length `FFT_SIZE`).
/// `h_est` is indexed by `sc - first_sc` (length = `p.total_sc()`).
/// Returns the equalized frequency-domain symbols for data SCs only,
/// in order of increasing SC index.
pub fn zf_equalize(p: &OfdmParams, freq: &[Complex32], h_est: &[Complex32]) -> Vec<Complex32> {
    let mut out = Vec::with_capacity(p.n_data);
    for (rel, &h_in) in freq[p.first_sc..=p.last_sc].iter().enumerate() {
        let sc = p.first_sc + rel;
        if is_pilot(p, sc) {
            continue;
        }
        let h = h_est[rel];
        let eq = if h.norm_sqr() < 1e-6 { h_in } else { h_in / h };
        out.push(eq);
    }
    out
}

/// Estimate the carrier frequency offset (CFO) in Hz using inter-symbol pilot
/// phase drift across consecutive OFDM symbols.
///
/// Averages the conjugate-product phase at each pilot SC across consecutive
/// symbol pairs (up to 8 symbols).  The phase rotation per symbol period is
/// `2π × CFO × SYM_LEN / Fs`, so the unambiguous range is
/// `±Fs / (2 × SYM_LEN) ≈ ±13.9 Hz`.
///
/// Returns `None` when there are fewer than two complete symbols or no pilots.
pub fn estimate_cfo_hz(samples: &[f32], p: &OfdmParams) -> Option<f32> {
    use std::f32::consts::PI;

    // Skip the (pilotless) timing-acquisition preamble; CFO is measured across
    // consecutive DATA symbols only.  Fall back to sample 0 when no preamble is
    // detected so callers passing bare symbol streams still get an estimate.
    let data_start = crate::demodulate::find_first_data_body(samples, p).unwrap_or(CP);
    let usable = samples.len().saturating_sub(data_start) + CP;
    let n_syms = usable / SYM_LEN;
    if n_syms < 2 {
        return None;
    }
    let pilots = pilot_positions(p);
    if pilots.is_empty() {
        return None;
    }

    let n_use = n_syms.min(8);
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);

    let mut spectra: Vec<Vec<Complex32>> = Vec::with_capacity(n_use);
    for sym_idx in 0..n_use {
        let start = data_start + sym_idx * SYM_LEN;
        if start + FFT_SIZE > samples.len() {
            break;
        }
        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * scale, 0.0))
            .collect();
        fft.process(&mut freq);
        spectra.push(freq);
    }
    if spectra.len() < 2 {
        return None;
    }

    // Vector-average the conjugate products (more robust than averaging
    // individual phase angles, which suffer wrap-around and over-weight
    // low-magnitude bins).
    let mut acc = Complex32::new(0.0, 0.0);
    for i in 0..(spectra.len() - 1) {
        for &k in &pilots {
            if k < FFT_SIZE {
                acc += spectra[i][k].conj() * spectra[i + 1][k];
            }
        }
    }
    if acc.norm_sqr() < 1e-12 {
        return None;
    }

    let mean_phase = acc.arg();
    let t_sym = SYM_LEN as f32 / SAMPLE_RATE as f32;
    Some(mean_phase / (2.0 * PI * t_sym))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{OFDM16, OFDM52};

    #[test]
    fn ofdm16_pilot_positions() {
        let pilots = pilot_positions(&OFDM16);
        // first_sc=38, PILOT_SPACING=5 → pilots at 38+4=42, 47, 52, 57
        assert_eq!(pilots, vec![42, 47, 52, 57]);
        assert_eq!(pilots.len(), OFDM16.n_pilots);
    }

    #[test]
    fn ofdm52_pilot_positions() {
        let pilots = pilot_positions(&OFDM52);
        // first_sc=16, pilots at 20,25,30,35,40,45,50,55,60,65,70,75,80
        assert_eq!(pilots.len(), OFDM52.n_pilots);
        assert_eq!(pilots[0], 20);
        assert_eq!(*pilots.last().unwrap(), 80);
    }

    #[test]
    fn ls_estimate_unity_channel() {
        // Under a unity channel (H=1+0j at all SCs), the estimate should be ~1.
        let p = &OFDM16;
        let mut freq = vec![Complex32::new(0.0, 0.0); 256];
        // Set all occupied SCs to data or pilot symbols.
        for item in freq.iter_mut().take(p.last_sc + 1).skip(p.first_sc) {
            *item = Complex32::new(1.0, 0.0);
        }
        let h_est = ls_estimate(p, &freq);
        for h in &h_est {
            assert!((h.re - 1.0).abs() < 0.01, "H.re={}", h.re);
            assert!(h.im.abs() < 0.01, "H.im={}", h.im);
        }
    }
}
