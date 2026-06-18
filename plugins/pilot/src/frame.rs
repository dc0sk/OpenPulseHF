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

/// QPSK carries 2 bits per symbol.
const QPSK_BITS: usize = 2;
/// Preamble length in symbols (PN-63 truncated/cycled to this many).
const PREAMBLE_SYMBOLS: usize = 48;
/// Data-region pilot cadence: one known pilot every `PILOT_SPACING` symbols.
const PILOT_SPACING: usize = 16;
/// Known pilot symbol (BPSK +1, unit power).
const PILOT: (f32, f32) = (1.0, 0.0);
/// Pilot-tracker loop bandwidth.
const LOOP_BW: f32 = 0.1;

/// Pilot-framed QPSK symbol-level codec.
pub struct PilotFrame {
    preamble: Vec<(f32, f32)>,
    pilot_spacing: usize,
}

impl Default for PilotFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl PilotFrame {
    /// Construct with the default (PN-63 / spacing-16) frame parameters.
    pub fn new() -> Self {
        let preamble = PreambleSpec::new(
            PreambleType::Pn63,
            PREAMBLE_SYMBOLS,
            PreambleConstellation::Bpsk,
        )
        .iq_symbols();
        Self {
            preamble,
            pilot_spacing: PILOT_SPACING,
        }
    }

    /// Number of preamble symbols at the front of every frame.
    pub fn preamble_len(&self) -> usize {
        self.preamble.len()
    }

    /// Data-region pilot cadence (one pilot per this many symbols).
    pub fn pilot_spacing(&self) -> usize {
        self.pilot_spacing
    }

    /// Encode payload bytes into frame symbols: preamble followed by the QPSK
    /// data symbols with a known pilot inserted every `pilot_spacing` positions.
    pub fn encode(&self, payload: &[u8]) -> Vec<(f32, f32)> {
        let data = bytes_to_qpsk(payload);
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
        // position, mirroring `encode`.
        let mut data_syms: Vec<(f32, f32)> = Vec::new();
        for (pos, &sym) in frame.iter().skip(plen).enumerate() {
            let is_pilot = pos.is_multiple_of(self.pilot_spacing);
            let corrected = tracker.process(sym, if is_pilot { Some(PILOT) } else { None });
            if !is_pilot {
                data_syms.push(corrected);
            }
        }

        qpsk_to_bytes(&data_syms)
    }
}

/// Pack payload bytes into QPSK symbols (4 symbols/byte, LSB-first 2-bit groups).
fn bytes_to_qpsk(payload: &[u8]) -> Vec<(f32, f32)> {
    let mut syms = Vec::with_capacity(payload.len() * 4);
    for &b in payload {
        for j in 0..4 {
            let bits = (b >> (QPSK_BITS * j)) & 0b11;
            let c = map_symbol(bits, QPSK_BITS);
            syms.push((c.re, c.im));
        }
    }
    syms
}

/// Inverse of [`bytes_to_qpsk`]: demap QPSK symbols back into bytes.
fn qpsk_to_bytes(syms: &[(f32, f32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(syms.len() / 4);
    for chunk in syms.chunks(4) {
        let mut b = 0u8;
        for (j, &(re, im)) in chunk.iter().enumerate() {
            let bits = demap_symbol(Complex32::new(re, im), QPSK_BITS) & 0b11;
            b |= bits << (QPSK_BITS * j);
        }
        out.push(b);
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
}
