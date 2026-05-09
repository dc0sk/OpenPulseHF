//! Pilot layout, LS channel estimation, and ZF equalization.

use num_complex::Complex32;

use crate::params::{OfdmParams, PILOT_AMPLITUDE, PILOT_SPACING};

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

    for rel in 0..first_pilot_rel {
        h_est[rel] = first_h;
    }
    for rel in (last_pilot_rel + 1)..total {
        h_est[rel] = last_h;
    }

    // Linear interpolation between adjacent pilot pairs.
    for window in known.windows(2) {
        let (rel0, h0) = window[0];
        let (rel1, h1) = window[1];
        h_est[rel0] = h0;
        h_est[rel1] = h1;
        if rel1 > rel0 + 1 {
            let steps = (rel1 - rel0) as f32;
            for k in (rel0 + 1)..rel1 {
                let t = (k - rel0) as f32 / steps;
                h_est[k] = h0 * (1.0 - t) + h1 * t;
            }
        }
    }
    // Set the known pilot positions.
    for (rel, h) in &known {
        h_est[*rel] = *h;
    }

    h_est
}

/// Zero-forcing equalization: divide each data SC bin by its channel estimate.
///
/// `freq` is the full FFT output (length `FFT_SIZE`).
/// `h_est` is indexed by `sc - first_sc` (length = `p.total_sc()`).
/// Returns the equalized frequency-domain symbols for data SCs only,
/// in order of increasing SC index.
pub fn zf_equalize(p: &OfdmParams, freq: &[Complex32], h_est: &[Complex32]) -> Vec<Complex32> {
    let mut out = Vec::with_capacity(p.n_data);
    for sc in p.first_sc..=p.last_sc {
        if is_pilot(p, sc) {
            continue;
        }
        let rel = sc - p.first_sc;
        let h = h_est[rel];
        // Avoid division by near-zero (deep fade or uninitialized estimate).
        let eq = if h.norm_sqr() < 1e-6 {
            freq[sc]
        } else {
            freq[sc] / h
        };
        out.push(eq);
    }
    out
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
        for sc in p.first_sc..=p.last_sc {
            freq[sc] = Complex32::new(1.0, 0.0);
        }
        let h_est = ls_estimate(p, &freq);
        for h in &h_est {
            assert!((h.re - 1.0).abs() < 0.01, "H.re={}", h.re);
            assert!(h.im.abs() < 0.01, "H.im={}", h.im);
        }
    }
}
