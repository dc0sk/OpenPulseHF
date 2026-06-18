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
use openpulse_dsp::constellation::{demap_symbol, map_symbol};
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

/// Pilot-framed symbol-level codec (QPSK / 8PSK / 16QAM data; BPSK pilots).
pub struct PilotFrame {
    preamble: Vec<(f32, f32)>,
    pilot_spacing: usize,
    bits_per_sc: usize,
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
        let data = bytes_to_symbols(payload, self.bits_per_sc);
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
        let mut tracker = PilotTracker::new(LOOP_BW);
        let plen = self.preamble.len();

        // Acquire on the fully-known preamble (every symbol is a pilot).
        for (k, &sym) in frame.iter().take(plen).enumerate() {
            tracker.process(sym, Some(self.preamble[k]));
        }

        // Track through the data region; pilots sit at every pilot_spacing-th
        // position, mirroring `encode`. Each data symbol is normalised by the
        // pilot-referenced amplitude so the demapper sees native constellation
        // scale — required for amplitude-bearing 16QAM, harmless for the
        // constant-modulus PSK orders.
        let mut data_syms: Vec<(f32, f32)> = Vec::new();
        for (pos, &sym) in frame.iter().skip(plen).enumerate() {
            let is_pilot = pos.is_multiple_of(self.pilot_spacing);
            let corrected = tracker.process(sym, if is_pilot { Some(PILOT) } else { None });
            if !is_pilot {
                let amp = tracker.amplitude().max(1e-6);
                data_syms.push((corrected.0 / amp, corrected.1 / amp));
            }
        }

        symbols_to_bytes(&data_syms, self.bits_per_sc)
    }
}

/// Pack payload bytes (LSB-first bit stream) into `bits_per_sc`-bit data symbols.
/// A final partial symbol is zero-padded.
fn bytes_to_symbols(payload: &[u8], bits_per_sc: usize) -> Vec<(f32, f32)> {
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
        let c = map_symbol(bits, bits_per_sc);
        syms.push((c.re, c.im));
    }
    syms
}

/// Inverse of [`bytes_to_symbols`]: demap symbols into the LSB-first bit stream
/// and pack whole bytes (a trailing partial byte from symbol padding is dropped).
fn symbols_to_bytes(syms: &[(f32, f32)], bits_per_sc: usize) -> Vec<u8> {
    let mut bits: Vec<u8> = Vec::with_capacity(syms.len() * bits_per_sc);
    for &(re, im) in syms {
        let v = demap_symbol(Complex32::new(re, im), bits_per_sc);
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
}
