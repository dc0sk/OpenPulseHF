//! Pilot layout, LS channel estimation, and ZF equalization for SC-FDMA.
//!
//! Pilot layout is identical to OFDM: every 5th SC starting at first_sc+4.

use num_complex::Complex32;

use crate::params::{ScFdmaParams, PILOT_AMPLITUDE, PILOT_SPACING};

/// Return the absolute SC indices of all pilot subcarriers for `p`.
pub fn pilot_positions(p: &ScFdmaParams) -> Vec<usize> {
    let mut pilots = Vec::with_capacity(p.n_pilots);
    let mut sc = p.first_sc + PILOT_SPACING - 1;
    while sc <= p.last_sc {
        pilots.push(sc);
        sc += PILOT_SPACING;
    }
    pilots
}

/// `true` when absolute SC index `sc` is a pilot for this mode.
pub fn is_pilot(p: &ScFdmaParams, sc: usize) -> bool {
    if sc < p.first_sc || sc > p.last_sc {
        return false;
    }
    let offset = sc - p.first_sc;
    offset % PILOT_SPACING == (PILOT_SPACING - 1)
}

/// Least-squares channel estimate at each pilot SC, linearly interpolated
/// across all data SCs.
///
/// Returns estimates indexed by `sc - first_sc` (length = `p.total_sc()`).
pub fn ls_estimate(p: &ScFdmaParams, freq: &[Complex32]) -> Vec<Complex32> {
    let total = p.total_sc();
    let pilots = pilot_positions(p);

    let known: Vec<(usize, Complex32)> = pilots
        .iter()
        .map(|&sc| {
            let h = freq[sc] / Complex32::new(PILOT_AMPLITUDE, 0.0);
            (sc - p.first_sc, h)
        })
        .collect();

    if known.is_empty() {
        return vec![Complex32::new(1.0, 0.0); total];
    }

    let mut h_est = vec![Complex32::new(1.0, 0.0); total];

    let (first_pilot_rel, first_h) = known[0];
    let (last_pilot_rel, last_h) = *known.last().unwrap();

    for h in h_est[..first_pilot_rel].iter_mut() {
        *h = first_h;
    }
    for h in h_est[(last_pilot_rel + 1)..].iter_mut() {
        *h = last_h;
    }

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
    for (rel, h) in &known {
        h_est[*rel] = *h;
    }

    h_est
}

/// Zero-forcing equalization: divide each data SC bin by its channel estimate.
///
/// Returns equalized frequency-domain symbols for data SCs only.
pub fn zf_equalize(p: &ScFdmaParams, freq: &[Complex32], h_est: &[Complex32]) -> Vec<Complex32> {
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

/// Estimate noise variance from pilot residuals.
///
/// Computes the mean squared error between the received pilots and the
/// LS channel estimate applied to the known pilot amplitude.  This gives a
/// per-symbol noise power estimate used to regularise MMSE equalization.
pub fn estimate_noise_var(p: &ScFdmaParams, freq: &[Complex32], h_est: &[Complex32]) -> f32 {
    let pilots = pilot_positions(p);
    if pilots.is_empty() {
        return 1e-3;
    }
    let sum: f32 = pilots
        .iter()
        .map(|&sc| {
            let rel = sc - p.first_sc;
            let received = freq[sc];
            let predicted = h_est[rel] * Complex32::new(PILOT_AMPLITUDE, 0.0);
            let diff = received - predicted;
            diff.norm_sqr()
        })
        .sum();
    (sum / pilots.len() as f32).max(1e-6)
}

/// Minimum mean-square-error equalization.
///
/// Regularises the ZF solution with the estimated noise variance so that
/// weak subcarriers do not amplify noise — critical for 16QAM and 64QAM.
///
/// `W_MMSE[k] = H*[k] / (|H[k]|² + σ²)`
pub fn mmse_equalize(
    p: &ScFdmaParams,
    freq: &[Complex32],
    h_est: &[Complex32],
    noise_var: f32,
) -> Vec<Complex32> {
    let mut out = Vec::with_capacity(p.n_data);
    for (rel, &h_in) in freq[p.first_sc..=p.last_sc].iter().enumerate() {
        let sc = p.first_sc + rel;
        if is_pilot(p, sc) {
            continue;
        }
        let h = h_est[rel];
        let denom = h.norm_sqr() + noise_var;
        let eq = if denom < 1e-9 {
            h_in
        } else {
            h_in * h.conj() / denom
        };
        out.push(eq);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{SCFDMA16, SCFDMA52};

    #[test]
    fn scfdma16_pilot_positions() {
        let pilots = pilot_positions(&SCFDMA16);
        assert_eq!(pilots, vec![42, 47, 52, 57]);
        assert_eq!(pilots.len(), SCFDMA16.n_pilots);
    }

    #[test]
    fn scfdma52_pilot_positions() {
        let pilots = pilot_positions(&SCFDMA52);
        assert_eq!(pilots.len(), SCFDMA52.n_pilots);
        assert_eq!(pilots[0], 20);
        assert_eq!(*pilots.last().unwrap(), 80);
    }
}
