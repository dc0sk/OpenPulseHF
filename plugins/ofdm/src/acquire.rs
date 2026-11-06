//! Carrier-frequency acquisition for the OFDM receiver (`docs/dev/design/ofdm-carrier-offset.md`).
//!
//! The engine's AFC settle is built for single-carrier BPSK; on OFDM it read −11.5 Hz for a true
//! +2.5 Hz and converged on each window's own wrong value, so the plugin acquires on its own, as
//! MFSK16 does, and as codec2's OFDM modem does: coarse offsets on a grid, the pilot drift for the
//! fine part, then one exact mix of the whole frame.

use std::borrow::Cow;

use num_complex::Complex32;

use crate::channel::pilot_drift;
use crate::demodulate::find_first_data_body;
use crate::params::{OfdmParams, SAMPLE_RATE, SYM_LEN};
use crate::shift::{analytic, rotate_real};

/// Coarse grid step. The timing search (a real-valued Schmidl–Cox, L = 128) gates on
/// `M = P²/R² ≥ 0.5`, and `M` falls as `cos²(π·Δ/31.25 Hz)` times an in-band SNR factor; the pilot
/// drift is unambiguous only to ±13.9 Hz. 12.5 Hz leaves at most 6.25 Hz to either (`M` 0.65 of
/// clean), which by that formula fails below about 9 dB in-band SNR. Measured 2026-11-03 at offsets
/// halfway between grid points (6.25, −18.75, 31.25 Hz), 8 noise seeds each: SL10 near its floor
/// (noise 0.05) and SL14 (0.02) decoded 8/8 with this step, as with 6.25 Hz; at noise 0.1 SL10 fails
/// on carrier too. 6.25 Hz would double the search for a loss not seen where a frame decodes at all.
const COARSE_STEP_HZ: f32 = 12.5;

/// Coarse search span either side of the receiver's centre: codec2's DATAC burst search covers
/// ±50 Hz. Untrimmed 2 m rigs were 54–64 Hz apart (`cfo.rs`); the engine's BPSK settle has usually
/// moved the centre to within a few Hz before an OFDM frame is tried, so ±50 Hz is around that.
const COARSE_SPAN_HZ: f32 = 50.0;

/// Pilot coherence at which a candidate is taken without trying the rest.
const EARLY_ACCEPT_COHERENCE: f32 = 0.9;

/// Data symbols the coarse score and the fine estimate use (the pilot drift reads at most 8).
const SCORE_SYMS: usize = 9;

/// The timing search's reach (`find_first_data_body`'s `SEARCH_CAP`) plus the preamble.
const TIMING_REACH: usize = 8192 + 2 * SYM_LEN;

/// What the search found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Acquired {
    /// Carrier offset, Hz, relative to the samples' own 1500 Hz centre.
    pub hz: f32,
    /// Pilot coherence across symbols of the candidate the offset came from, 0–1.
    pub coherence: f32,
}

/// The carrier offset of the OFDM frame in `samples`, relative to the samples' own 1500 Hz centre, or
/// `None` when no candidate finds a preamble with pilots.
///
/// Candidates run outward from `told` (the offset the caller already believes) on a grid. Each mixes
/// a short window from the burst's start, finds the timing there, and scores the pilots' coherence
/// across symbols; a wrong offset puts data, not pilots, in the pilot bins. The first candidate at
/// [`EARLY_ACCEPT_COHERENCE`] ends the search, otherwise the best is taken. Its pilot drift is the
/// fine part, and one more pass at the sum removes what is left. Only the window is ever mixed, and
/// its analytic signal is computed once, when the first candidate needs a rotation.
pub fn acquire(samples: &[f32], p: &OfdmParams, told: f32) -> Option<Acquired> {
    let window = &samples[..samples.len().min(TIMING_REACH + SCORE_SYMS * SYM_LEN)];
    let fs = SAMPLE_RATE as f32;
    let mut z: Option<Vec<Complex32>> = None;
    let mut at = |hz: f32| -> Cow<'_, [f32]> {
        if hz == 0.0 {
            return Cow::Borrowed(window);
        }
        let z = z.get_or_insert_with(|| analytic(window));
        Cow::Owned(rotate_real(z, -hz, fs))
    };
    let steps = (COARSE_SPAN_HZ / COARSE_STEP_HZ).round() as i32;
    // (coherence, offset); `told` first, then outward, so a tie keeps the nearer offset.
    let mut best: Option<(f32, f32)> = None;
    for k in (0..=steps).flat_map(|k| if k == 0 { vec![0] } else { vec![k, -k] }) {
        let coarse = told + k as f32 * COARSE_STEP_HZ;
        let w = at(coarse);
        let Some(start) = find_first_data_body(&w, p) else {
            continue;
        };
        let Some((fine, coherence)) = pilot_drift(&w, start, p) else {
            continue;
        };
        if best.is_none_or(|(c, _)| coherence > c) {
            best = Some((coherence, coarse + fine));
        }
        if coherence >= EARLY_ACCEPT_COHERENCE {
            break;
        }
    }
    let (coherence, offset) = best?;
    // One more pass at the estimate: the fine estimate is closest to right near zero residual.
    let w = at(offset);
    let residual = find_first_data_body(&w, p)
        .and_then(|start| pilot_drift(&w, start, p))
        .map_or(0.0, |(hz, _)| hz);
    Some(Acquired {
        hz: offset + residual,
        coherence,
    })
}
