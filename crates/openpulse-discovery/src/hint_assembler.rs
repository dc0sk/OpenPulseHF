//! Cross-slot assembly of the `@OPULSE` capability beacon (plan §3.2).
//!
//! On air the beacon `DC0SK: @OPULSE OPHF1 XXXXXXXX` is **not** one frame — the upstream JS8 packer
//! emits it as an *over* of four NORMAL frames, one per 15 s slot (verified against the Qt5-compiled
//! `Varicode::buildMessageFrames`): a `Compound` frame carrying the sender + grid, a
//! `CompoundDirected` frame carrying the group `@OPULSE` (a custom group cannot ride the 28-bit
//! directed `to` field), then `Data` frames carrying the free text. A station transmits its whole
//! over at one stable audio offset and brackets it with the `First`/`Last` transmission flags, so
//! this assembler buffers decoded frames per frequency bucket and, once the sender + `@OPULSE` target
//! + free text are present, runs [`decode_hint`] to recognise the peer.
//!
//! The OpenPulse beacon is standardised on Huffman-framed data (its alphabet is entirely in the JS8
//! Huffman table and stock JS8Call decodes it), so only the Huffman data path is consumed here; a
//! JSC-compressed data frame is ignored (general free-text decode is a separate follow-on).

use js8_plugin::{unpack_compound_frame, unpack_data_message, unpack_grid, FrameType};

use crate::hint::{decode_hint, HintPayload, OPULSE_GROUP};

/// `i3bit` transmission flag marking the first frame of an over (JS8Call `JS8CallFirst`).
const I3_FIRST: u8 = 1;
/// Upper bound on accumulated free text per over (a stuck bucket can't grow unbounded).
const MAX_TEXT_LEN: usize = 256;

/// A recognised OpenPulse peer: the sender of a valid `@OPULSE` hint and its decoded capabilities.
#[derive(Debug, Clone, PartialEq)]
pub struct RecognizedHint {
    /// Sender callsign (the hint CRC salt).
    pub callsign: String,
    /// Sender grid if the compound frame advertised one.
    pub grid: Option<String>,
    /// Decoded hint capabilities.
    pub hint: HintPayload,
    /// Audio offset the over was heard at (Hz).
    pub base_freq_hz: f32,
}

/// An in-progress over buffered at one audio offset.
#[derive(Debug)]
struct PartialOver {
    base_freq_hz: f32,
    last_slot: u64,
    sender: Option<(String, Option<String>)>,
    target_group: Option<String>,
    text: String,
}

impl PartialOver {
    fn new(base_freq_hz: f32, now_slot: u64) -> Self {
        Self {
            base_freq_hz,
            last_slot: now_slot,
            sender: None,
            target_group: None,
            text: String::new(),
        }
    }
}

/// Two audio offsets belong to the same station's over if within `tol` Hz.
fn freq_match(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

/// Buffers JS8 decodes across slots and recognises `@OPULSE` hint beacons.
pub struct HintAssembler {
    freq_tol_hz: f32,
    max_over_slots: u64,
    partials: Vec<PartialOver>,
}

impl HintAssembler {
    /// A JS8 station holds a stable audio offset across an over; `freq_tol_hz` buckets decodes to it
    /// and `max_over_slots` evicts an over whose `Last` frame never decoded.
    pub fn new(freq_tol_hz: f32, max_over_slots: u64) -> Self {
        Self {
            freq_tol_hz,
            max_over_slots,
            partials: Vec::new(),
        }
    }

    /// Ingest one decoded frame (`payload9`, its `i3bit` transmission flag, its audio offset, and the
    /// current slot index). Returns a [`RecognizedHint`] once a complete, CRC-valid `@OPULSE` beacon
    /// has assembled at this offset.
    pub fn ingest(
        &mut self,
        payload9: &[u8; 9],
        i3bit: u8,
        base_freq_hz: f32,
        now_slot: u64,
    ) -> Option<RecognizedHint> {
        self.sweep(now_slot);
        // The first frame of an over starts fresh — drop any stale over lingering at this offset.
        if i3bit == I3_FIRST {
            self.partials
                .retain(|p| !freq_match(p.base_freq_hz, base_freq_hz, self.freq_tol_hz));
        }
        let idx = match self
            .partials
            .iter()
            .position(|p| freq_match(p.base_freq_hz, base_freq_hz, self.freq_tol_hz))
        {
            Some(i) => i,
            None => {
                self.partials.push(PartialOver::new(base_freq_hz, now_slot));
                self.partials.len() - 1
            }
        };
        self.partials[idx].last_slot = now_slot;
        Self::apply_frame(&mut self.partials[idx], payload9);

        // The CRC-salted hint check makes early recognition on a partial text simply fail, so we can
        // try after every frame rather than waiting for the `Last` flag (which a fade may drop).
        if let Some(h) = Self::try_recognize(&self.partials[idx]) {
            self.partials.remove(idx);
            return Some(h);
        }
        None
    }

    /// Drop overs whose most recent frame is older than `max_over_slots`.
    pub fn sweep(&mut self, now_slot: u64) {
        let max = self.max_over_slots;
        self.partials
            .retain(|p| now_slot.saturating_sub(p.last_slot) <= max);
    }

    /// Fold one frame into an over: sender from a `Compound` frame, target from a `CompoundDirected`
    /// frame, free text from Huffman `Data` frames; everything else is ignored.
    fn apply_frame(p: &mut PartialOver, payload9: &[u8; 9]) {
        if let Some(cf) = unpack_compound_frame(payload9) {
            match cf.frame_type {
                FrameType::Compound if !cf.callsign.starts_with('@') => {
                    let g = unpack_grid(cf.num & 0x7fff);
                    p.sender = Some((cf.callsign, (!g.is_empty()).then_some(g)));
                }
                FrameType::CompoundDirected => p.target_group = Some(cf.callsign),
                _ => {}
            }
        } else if let Some(t) = unpack_data_message(payload9) {
            if p.text.len() + t.len() <= MAX_TEXT_LEN {
                p.text.push_str(&t);
            }
        }
    }

    /// Recognise a complete over: it must carry a sender, an `@OPULSE` target, and free text whose
    /// hint verifies against the sender callsign.
    fn try_recognize(p: &PartialOver) -> Option<RecognizedHint> {
        let (callsign, grid) = p.sender.clone()?;
        let target = p.target_group.as_ref()?;
        let group = target.strip_prefix('@').unwrap_or(target);
        if !group.eq_ignore_ascii_case(OPULSE_GROUP) {
            return None;
        }
        let hint = decode_hint(&p.text, &callsign)?;
        Some(RecognizedHint {
            callsign,
            grid,
            hint,
            base_freq_hz: p.base_freq_hz,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex9(s: &str) -> [u8; 9] {
        let mut p = [0u8; 9];
        for (i, b) in p.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        p
    }

    // The four Huffman-forced frames of `DC0SK: @OPULSE OPHF1 1FAX3AIT` (a valid hint for DC0SK:
    // caps 0xB105, pref_channel 42, listen_submode 1), from the Qt5-compiled `buildMessageFrames` +
    // forced `packHuffMessage`. (payload9, i3bit).
    const SENDER: (&str, u8) = ("2694fa766ea662ea58", I3_FIRST); // Compound: DC0SK EM73
    const TARGET: (&str, u8) = ("531a90d5639ea3f5c8", 0); // CompoundDirected: @OPULSE
    const DATA0: (&str, u8) = ("bfec6491489275029b", 0); // Data: "OPHF1 1FAX3A"
    const DATA1: (&str, u8) = ("b9afffffffffffffff", 2); // Data (Last): "IT"

    fn feed(a: &mut HintAssembler, f: (&str, u8), freq: f32, slot: u64) -> Option<RecognizedHint> {
        a.ingest(&hex9(f.0), f.1, freq, slot)
    }

    #[test]
    fn assembles_a_four_frame_beacon_into_a_recognized_peer() {
        let mut a = HintAssembler::new(3.0, 6);
        assert_eq!(feed(&mut a, SENDER, 1500.0, 0), None);
        assert_eq!(feed(&mut a, TARGET, 1500.0, 1), None);
        assert_eq!(feed(&mut a, DATA0, 1500.0, 2), None); // text still partial → CRC fails
        let r = feed(&mut a, DATA1, 1500.0, 3).expect("recognized on the final data frame");
        assert_eq!(r.callsign, "DC0SK");
        assert_eq!(r.grid.as_deref(), Some("EM73"));
        assert_eq!(r.hint.caps, 0xB105);
        assert_eq!(r.hint.pref_channel, 42);
        assert_eq!(r.hint.listen_submode, 1);
    }

    #[test]
    fn frames_at_a_different_offset_do_not_cross_contaminate() {
        let mut a = HintAssembler::new(3.0, 6);
        // Sender at 1500, but the rest of the beacon lands at 800 Hz (a different station's over).
        assert_eq!(feed(&mut a, SENDER, 1500.0, 0), None);
        assert_eq!(feed(&mut a, TARGET, 800.0, 1), None);
        assert_eq!(feed(&mut a, DATA0, 800.0, 2), None);
        assert_eq!(feed(&mut a, DATA1, 800.0, 3), None); // never a full over at either offset
    }

    #[test]
    fn a_stale_over_is_evicted_before_it_completes() {
        let mut a = HintAssembler::new(3.0, 2);
        assert_eq!(feed(&mut a, SENDER, 1500.0, 0), None);
        assert_eq!(feed(&mut a, TARGET, 1500.0, 1), None);
        // The data frames arrive far too late (beyond max_over_slots) → the over was swept.
        assert_eq!(feed(&mut a, DATA0, 1500.0, 10), None);
        assert_eq!(feed(&mut a, DATA1, 1500.0, 11), None);
    }

    #[test]
    fn a_hint_minted_for_another_callsign_is_rejected() {
        // Same beacon frames, but the sender compound frame is swapped for KN4CRD — the hint text was
        // CRC-salted with DC0SK, so it must not verify.
        let mut a = HintAssembler::new(3.0, 6);
        let kn4crd = ("2a2fb3a3ee2ee2ea58", I3_FIRST); // Compound: KN4CRD EM73 (from ground truth)
        assert_eq!(feed(&mut a, kn4crd, 1500.0, 0), None);
        assert_eq!(feed(&mut a, TARGET, 1500.0, 1), None);
        assert_eq!(feed(&mut a, DATA0, 1500.0, 2), None);
        assert_eq!(feed(&mut a, DATA1, 1500.0, 3), None);
    }

    #[test]
    fn interleaved_heartbeats_do_not_disturb_assembly() {
        let mut a = HintAssembler::new(3.0, 6);
        let hb = ("0a2fb3a3ee2ee2ea58", 0u8); // a plain KN4CRD heartbeat at another offset
        assert_eq!(feed(&mut a, SENDER, 1500.0, 0), None);
        assert_eq!(feed(&mut a, hb, 1000.0, 0), None);
        assert_eq!(feed(&mut a, TARGET, 1500.0, 1), None);
        assert_eq!(feed(&mut a, hb, 1000.0, 1), None);
        assert_eq!(feed(&mut a, DATA0, 1500.0, 2), None);
        let r = feed(&mut a, DATA1, 1500.0, 3).expect("recognized despite interleaved heartbeats");
        assert_eq!(r.callsign, "DC0SK");
    }
}
