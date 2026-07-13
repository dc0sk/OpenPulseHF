//! Pilot-framed QPSK frame codec (symbol level).
//!
//! Frame layout (complex symbols):
//!
//! ```text
//! [ preamble (PN-63 BPSK, all known) | data region: pilot, data×(S-1), pilot, data×(S-1), … ]
//! ```
//!
//! The preamble is fully known and is used to **acquire** carrier phase and
//! frequency; the sparse data-region pilots (one every `pilot_spacing` symbols)
//! then **track** the residual. Both stages drive a single
//! [`PilotTracker`], so the carrier is recovered from known symbols only —
//! immune to the decision-directed cycle slips that limit the preamble-only QPSK
//! mode on dense constellations and through a carrier offset.
//!
//! This is the symbol-level core; the passband audio chain (pulse shaping,
//! up/down-conversion, symbol timing) wraps it in a follow-up. The QPSK mapping
//! is the shared [`openpulse_dsp::constellation`] mapping used by every other
//! multicarrier/PSK mode.

use num_complex::Complex32;
use openpulse_dsp::constellation::{
    apsk32_points, constellation_points, demap_apsk32, demap_symbol, map_apsk32, map_symbol,
    symbol_llrs,
};
use openpulse_dsp::pilot_tracker::PilotTracker;
use openpulse_dsp::preamble::{PreambleConstellation, PreambleSpec, PreambleType};

/// Default constellation: QPSK (2 bits/symbol).
const DEFAULT_BITS_PER_SC: usize = 2;
/// Preamble length in symbols (PN-63 truncated/cycled to this many).
const PREAMBLE_SYMBOLS: usize = 48;
/// Data-region pilot cadence: one known pilot every `PILOT_SPACING` symbols.
const PILOT_SPACING: usize = 16;
/// Known pilot symbol (BPSK +1, unit power).
const PILOT: (f32, f32) = (1.0, 0.0);
/// Pilot-tracker loop bandwidth.
const LOOP_BW: f32 = 0.1;

/// Pilot-framed symbol-level codec (QPSK / 8PSK / 16QAM / 32APSK data; BPSK pilots).
pub struct PilotFrame {
    preamble: Vec<(f32, f32)>,
    pilot_spacing: usize,
    bits_per_sc: usize,
    /// When set, the 5-bit data uses the DVB-S2 32APSK constellation instead of
    /// the Gray cross-32QAM that `bits_per_sc = 5` otherwise selects.
    apsk32: bool,
}

impl Default for PilotFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl PilotFrame {
    /// Construct with the default QPSK data constellation (2 bits/symbol).
    pub fn new() -> Self {
        Self::with_bits(DEFAULT_BITS_PER_SC)
    }

    /// Construct with a chosen data constellation: `bits_per_sc` = 2 (QPSK),
    /// 3 (8PSK), 4 (16QAM) — the shared [`openpulse_dsp::constellation`] orders.
    /// Pilots and preamble stay BPSK regardless.
    pub fn with_bits(bits_per_sc: usize) -> Self {
        Self::build(bits_per_sc, false)
    }

    /// Construct with DVB-S2 32APSK data (5 bits/symbol); pilots stay BPSK.
    pub fn with_apsk32() -> Self {
        Self::build(5, true)
    }

    fn build(bits_per_sc: usize, apsk32: bool) -> Self {
        let preamble = PreambleSpec::new(
            PreambleType::Pn63,
            PREAMBLE_SYMBOLS,
            PreambleConstellation::Bpsk,
        )
        .iq_symbols();
        Self {
            preamble,
            pilot_spacing: PILOT_SPACING,
            bits_per_sc,
            apsk32,
        }
    }

    /// Number of preamble symbols at the front of every frame.
    pub fn preamble_len(&self) -> usize {
        self.preamble.len()
    }

    /// The known preamble symbols (used by the passband layer to build the
    /// onset-correlation template).
    pub fn preamble(&self) -> &[(f32, f32)] {
        &self.preamble
    }

    /// Data-region pilot cadence (one pilot per this many symbols).
    pub fn pilot_spacing(&self) -> usize {
        self.pilot_spacing
    }

    /// Encode payload bytes into frame symbols: preamble followed by the data
    /// symbols with a known pilot inserted every `pilot_spacing` positions.
    pub fn encode(&self, payload: &[u8]) -> Vec<(f32, f32)> {
        let data = bytes_to_symbols(payload, self.bits_per_sc, self.apsk32);
        let mut frame = self.preamble.clone();
        let mut di = 0usize;
        let mut pos = 0usize;
        while di < data.len() {
            if pos.is_multiple_of(self.pilot_spacing) {
                frame.push(PILOT);
            } else {
                frame.push(data[di]);
                di += 1;
            }
            pos += 1;
        }
        frame
    }

    /// Decode frame symbols back to payload bytes, recovering the carrier with a
    /// preamble-seeded, pilot-tracked [`PilotTracker`].
    ///
    /// Assumes symbol synchronisation (the passband layer provides timing and
    /// onset); `frame` must start at the first preamble symbol.
    pub fn decode(&self, frame: &[(f32, f32)]) -> Vec<u8> {
        let (data_syms, _) = self.recover_data_syms(frame);
        symbols_to_bytes(&data_syms, self.bits_per_sc, self.apsk32)
    }

    /// Soft-decision decode: recover the carrier exactly as [`decode`](Self::decode),
    /// then emit per-bit max-log-MAP LLRs (positive = bit more likely 0) instead of
    /// hard bytes. Hard-slicing the result (`bit = llr <= 0`, LSB-first) reproduces
    /// [`decode`](Self::decode)'s bytes, matching the cross-plugin LLR convention.
    pub fn decode_soft(&self, frame: &[(f32, f32)]) -> Vec<f32> {
        let (data_syms, pilot_noise_var) = self.recover_data_syms(frame);
        symbols_to_llrs(&data_syms, self.bits_per_sc, self.apsk32, pilot_noise_var)
    }

    /// Acquire on the fully-known preamble, track the residual through the data
    /// region with the sparse pilots, and return the pilot-amplitude-normalised
    /// data symbols (pilots removed). Shared by the hard and soft decoders so both
    /// see identical symbols.
    ///
    /// Each data symbol is normalised by the pilot-referenced amplitude so the
    /// demapper sees native constellation scale — required for amplitude-bearing
    /// 16QAM/32APSK, harmless for the constant-modulus PSK orders.
    /// Also returns the additive 2-D noise variance `E|n|²` measured from the known symbols (settled
    /// preamble + data-region pilots) — the deviation of the amplitude-normalised known symbol from
    /// its reference. Unlike a decision-directed estimate over the data, this uses no decisions, so it
    /// does not saturate on the dense 16QAM/32APSK grid; `None` when too few known symbols are
    /// available (a very short frame), leaving the LLR builder its decision-directed fallback.
    fn recover_data_syms(&self, frame: &[(f32, f32)]) -> (Vec<(f32, f32)>, Option<f32>) {
        let mut tracker = PilotTracker::new(LOOP_BW);
        let plen = self.preamble.len();
        let mut noise_sum = 0.0f32;
        let mut noise_count = 0usize;
        // Skip the first half of the preamble while the loop is still converging — its residual is
        // acquisition transient, not noise.
        let settle = plen / 2;

        // Acquire on the fully-known preamble (every symbol is a pilot).
        for (k, &sym) in frame.iter().take(plen).enumerate() {
            let corrected = tracker.process(sym, Some(self.preamble[k]));
            if k >= settle {
                let amp = tracker.amplitude().max(1e-6);
                let dr = corrected.0 / amp - self.preamble[k].0;
                let di = corrected.1 / amp - self.preamble[k].1;
                noise_sum += dr * dr + di * di;
                noise_count += 1;
            }
        }

        // Track through the data region; pilots sit at every pilot_spacing-th
        // position, mirroring `encode`.
        let mut data_syms: Vec<(f32, f32)> = Vec::new();
        for (pos, &sym) in frame.iter().skip(plen).enumerate() {
            let is_pilot = pos.is_multiple_of(self.pilot_spacing);
            let corrected = tracker.process(sym, if is_pilot { Some(PILOT) } else { None });
            let amp = tracker.amplitude().max(1e-6);
            if is_pilot {
                let dr = corrected.0 / amp - PILOT.0;
                let di = corrected.1 / amp - PILOT.1;
                noise_sum += dr * dr + di * di;
                noise_count += 1;
            } else {
                data_syms.push((corrected.0 / amp, corrected.1 / amp));
            }
        }

        let noise_var = (noise_count >= 8).then(|| (noise_sum / noise_count as f32).max(1e-6));
        (data_syms, noise_var)
    }
}

/// Map a `bits_per_sc`-bit label to a point, dispatching 32APSK vs Gray QAM/PSK.
fn map_data(bits: u8, bits_per_sc: usize, apsk32: bool) -> (f32, f32) {
    let c = if apsk32 {
        map_apsk32(bits)
    } else {
        map_symbol(bits, bits_per_sc)
    };
    (c.re, c.im)
}

/// Demap a point to its `bits_per_sc`-bit label (32APSK vs Gray QAM/PSK).
fn demap_data(re: f32, im: f32, bits_per_sc: usize, apsk32: bool) -> u8 {
    if apsk32 {
        demap_apsk32(Complex32::new(re, im))
    } else {
        demap_symbol(Complex32::new(re, im), bits_per_sc)
    }
}

/// Pack payload bytes (LSB-first bit stream) into `bits_per_sc`-bit data symbols.
/// A final partial symbol is zero-padded.
fn bytes_to_symbols(payload: &[u8], bits_per_sc: usize, apsk32: bool) -> Vec<(f32, f32)> {
    let total_bits = payload.len() * 8;
    let nsyms = total_bits.div_ceil(bits_per_sc);
    let mut syms = Vec::with_capacity(nsyms);
    for s in 0..nsyms {
        let mut bits = 0u8;
        for b in 0..bits_per_sc {
            let gb = s * bits_per_sc + b;
            let bit = if gb < total_bits {
                (payload[gb / 8] >> (gb % 8)) & 1
            } else {
                0
            };
            bits |= bit << b;
        }
        syms.push(map_data(bits, bits_per_sc, apsk32));
    }
    syms
}

/// Inverse of [`bytes_to_symbols`]: demap symbols into the LSB-first bit stream
/// and pack whole bytes (a trailing partial byte from symbol padding is dropped).
fn symbols_to_bytes(syms: &[(f32, f32)], bits_per_sc: usize, apsk32: bool) -> Vec<u8> {
    let mut bits: Vec<u8> = Vec::with_capacity(syms.len() * bits_per_sc);
    for &(re, im) in syms {
        let v = demap_data(re, im, bits_per_sc, apsk32);
        for b in 0..bits_per_sc {
            bits.push((v >> b) & 1);
        }
    }
    let nbytes = bits.len() / 8;
    let mut out = vec![0u8; nbytes];
    for (i, &bit) in bits.iter().take(nbytes * 8).enumerate() {
        if bit != 0 {
            out[i / 8] |= 1 << (i % 8);
        }
    }
    out
}

/// Soft counterpart of [`symbols_to_bytes`]: per-bit max-log-MAP LLRs (positive =
/// bit more likely 0), in the same symbol-major, LSB-first-within-symbol order, so
/// hard-slicing the LLRs (`bit = llr <= 0`) reproduces `symbols_to_bytes`'s bytes.
/// Calibrated so `|LLR|` scales with `1/σ²`: `symbol_llrs` is divided by the 2-D noise variance.
///
/// The preferred `noise_var` is the pilot/preamble-residual estimate from [`recover_data_syms`]
/// (data-aided, so it does not saturate). When it is unavailable the fallback is the decision-directed
/// estimate (mean squared distance to the nearest constellation point) — correct at high SNR but it
/// under-reads σ² on the dense 16QAM/32APSK grids at moderate SNR, leaving the LLRs over-confident.
fn symbols_to_llrs(
    syms: &[(f32, f32)],
    bits_per_sc: usize,
    apsk32: bool,
    pilot_noise_var: Option<f32>,
) -> Vec<f32> {
    let points = if apsk32 {
        apsk32_points()
    } else {
        constellation_points(bits_per_sc)
    };
    let noise_var = match pilot_noise_var {
        // The pilot residual is the *additive* noise σ². The data symbols are additionally de-rotated
        // and amplitude-normalised by a phase/amplitude reference estimated from those same noisy
        // pilots, so each picks up a signal-power-dependent estimation-error term ≈ `P_c·σ²` on top —
        // the single-carrier analogue of the OFDM channel-estimate-error term. With the conservative
        // `σ²_est ≈ σ²` this is the `(1+P_c)` factor (`P_c` = average constellation power).
        Some(sigma2) => {
            let p_c = points.iter().map(|(_, p)| p.norm_sqr()).sum::<f32>() / points.len() as f32;
            (sigma2 * (1.0 + p_c)).max(1e-6)
        }
        // Fallback (a frame too short for the pilot estimate): the decision-directed noise. Correct at
        // high SNR but it under-reads σ² on the dense grids at moderate SNR (over-confident LLRs).
        None => {
            if syms.is_empty() {
                1.0
            } else {
                let sum: f32 = syms
                    .iter()
                    .map(|&(re, im)| {
                        let z = Complex32::new(re, im);
                        points
                            .iter()
                            .map(|(_, p)| (z - *p).norm_sqr())
                            .fold(f32::INFINITY, f32::min)
                    })
                    .sum();
                (sum / syms.len() as f32).max(1e-6)
            }
        }
    };
    let mut llrs: Vec<f32> = Vec::with_capacity(syms.len() * bits_per_sc);
    for &(re, im) in syms {
        llrs.extend(symbol_llrs(
            Complex32::new(re, im),
            bits_per_sc,
            noise_var,
            &points,
        ));
    }
    // Match symbols_to_bytes, which packs only whole bytes (dropping a trailing
    // partial byte from symbol zero-padding): trim to the same bit count.
    let whole = (llrs.len() / 8) * 8;
    llrs.truncate(whole);
    llrs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rot(x: (f32, f32), ang: f32) -> (f32, f32) {
        let (c, s) = (ang.cos(), ang.sin());
        (x.0 * c - x.1 * s, x.0 * s + x.1 * c)
    }

    fn payload() -> Vec<u8> {
        b"pilot-framed QPSK round-trip 0123456789 abcdefghij the quick brown fox".to_vec()
    }

    #[test]
    fn clean_round_trip() {
        let f = PilotFrame::new();
        let frame = f.encode(&payload());
        assert_eq!(f.decode(&frame), payload());
    }

    #[test]
    fn frame_geometry_is_consistent() {
        let f = PilotFrame::new();
        let p = payload();
        let frame = f.encode(&p);
        let data_syms = p.len() * 4;
        // Pilots: one at every pilot_spacing-th data-region position.
        let pilots = data_syms.div_ceil(f.pilot_spacing() - 1);
        assert!(frame.len() >= f.preamble_len() + data_syms);
        assert!(frame.len() <= f.preamble_len() + data_syms + pilots + 1);
    }

    #[test]
    fn round_trip_through_carrier_frequency_offset() {
        // A continuous carrier offset rotates symbol k by phi0 + dphi*k across the
        // WHOLE frame; the preamble acquires it and the sparse pilots track it.
        let f = PilotFrame::new();
        let frame = f.encode(&payload());
        let dphi = 0.01f32; // rad/symbol
        let phi0 = 0.6f32;
        let rxd: Vec<(f32, f32)> = frame
            .iter()
            .enumerate()
            .map(|(k, &x)| rot(x, phi0 + dphi * k as f32))
            .collect();
        assert_eq!(
            f.decode(&rxd),
            payload(),
            "pilot tracking must recover data through a carrier offset"
        );
    }

    #[test]
    fn round_trip_through_static_phase() {
        let f = PilotFrame::new();
        let frame = f.encode(&payload());
        let rxd: Vec<(f32, f32)> = frame.iter().map(|&x| rot(x, 2.0)).collect();
        assert_eq!(f.decode(&rxd), payload());
    }

    /// Decode recovers `payload` as a prefix (dense orders may append a trailing
    /// partial byte from symbol zero-padding).
    fn assert_prefix(out: &[u8], p: &[u8]) {
        assert!(
            out.len() >= p.len() && &out[..p.len()] == p,
            "decoded prefix mismatch (got {} bytes)",
            out.len()
        );
    }

    #[test]
    fn clean_round_trip_8psk() {
        let f = PilotFrame::with_bits(3);
        let frame = f.encode(&payload());
        assert_prefix(&f.decode(&frame), &payload());
    }

    #[test]
    fn clean_round_trip_16qam() {
        let f = PilotFrame::with_bits(4);
        let frame = f.encode(&payload());
        assert_prefix(&f.decode(&frame), &payload());
    }

    #[test]
    fn round_trip_16qam_through_carrier_frequency_offset() {
        // 16QAM is the dense canary: amplitude-bearing, so it exercises the
        // pilot-referenced amplitude normalisation as well as phase/freq tracking.
        let f = PilotFrame::with_bits(4);
        let frame = f.encode(&payload());
        let dphi = 0.01f32;
        let phi0 = 0.6f32;
        let rxd: Vec<(f32, f32)> = frame
            .iter()
            .enumerate()
            .map(|(k, &x)| rot(x, phi0 + dphi * k as f32))
            .collect();
        assert_prefix(&f.decode(&rxd), &payload());
    }

    #[test]
    fn clean_round_trip_apsk32() {
        let f = PilotFrame::with_apsk32();
        let frame = f.encode(&payload());
        assert_prefix(&f.decode(&frame), &payload());
    }

    #[test]
    fn round_trip_apsk32_through_carrier_frequency_offset() {
        // 32APSK is amplitude-bearing across three rings: the hardest case for
        // the pilot-referenced amplitude normalisation + carrier tracking.
        let f = PilotFrame::with_apsk32();
        let frame = f.encode(&payload());
        let dphi = 0.01f32;
        let phi0 = 0.6f32;
        let rxd: Vec<(f32, f32)> = frame
            .iter()
            .enumerate()
            .map(|(k, &x)| rot(x, phi0 + dphi * k as f32))
            .collect();
        assert_prefix(&f.decode(&rxd), &payload());
    }

    /// Hard-slice an LLR stream into bytes (`bit = llr <= 0`, LSB-first).
    fn hard_slice(llrs: &[f32]) -> Vec<u8> {
        llrs.chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0u8, |a, (i, &l)| a | (u8::from(l <= 0.0) << i))
            })
            .collect()
    }

    #[test]
    fn decode_soft_hard_slice_matches_decode() {
        // For every constellation, hard-slicing decode_soft's LLRs must reproduce
        // decode()'s bytes exactly — pins the sign + bit order the FEC layer needs.
        let frames = [
            PilotFrame::with_bits(2),
            PilotFrame::with_bits(3),
            PilotFrame::with_bits(4),
            PilotFrame::with_apsk32(),
        ];
        for f in &frames {
            let frame = f.encode(&payload());
            let hard = f.decode(&frame);
            let soft = hard_slice(&f.decode_soft(&frame));
            let n = hard.len().min(soft.len());
            assert_eq!(soft[..n], hard[..n], "bits_per_sc={}", f.bits_per_sc);
            assert!(n >= payload().len(), "decoded too few bytes ({n})");
        }
    }

    #[test]
    fn decode_soft_recovers_through_carrier_offset() {
        // Soft LLRs must also be correct through a carrier offset (pilot tracking).
        let f = PilotFrame::with_bits(3);
        let frame = f.encode(&payload());
        let (dphi, phi0) = (0.01f32, 0.6f32);
        let rxd: Vec<(f32, f32)> = frame
            .iter()
            .enumerate()
            .map(|(k, &x)| rot(x, phi0 + dphi * k as f32))
            .collect();
        let soft = hard_slice(&f.decode_soft(&rxd));
        assert!(soft.len() >= payload().len() && soft[..payload().len()] == payload()[..]);
    }
}
