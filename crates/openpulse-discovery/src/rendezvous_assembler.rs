//! Cross-slot assembly of an inbound rendezvous over (plan §5.3).
//!
//! A rendezvous message travels as a JS8 directed free-text over addressed to *us*: a `Compound` frame
//! (sender + grid), a `CompoundDirected` frame carrying our callsign as the target, then Huffman `Data`
//! frames carrying the OPHF rendezvous text (`OPHF QSY? …` / `OPHF QSY …` / `OPHF NO …`). This mirrors
//! [`crate::hint_assembler`] but keys the target on our own callsign and parses the reassembled text as
//! a [`RendezvousMsg`] rather than a capability hint.
//!
//! Only overs whose `CompoundDirected` target matches our callsign are surfaced, so ordinary traffic
//! and other stations' rendezvous exchanges are ignored.

use js8_plugin::{unpack_compound_frame, unpack_data_message, unpack_grid, FrameType};

use crate::rendezvous::RendezvousMsg;

/// `i3bit` transmission flag marking the first frame of an over (JS8Call `JS8CallFirst`).
const I3_FIRST: u8 = 1;
/// Upper bound on accumulated free text per over (a stuck bucket can't grow unbounded).
const MAX_TEXT_LEN: usize = 256;

/// A recognised inbound rendezvous message and who sent it.
#[derive(Debug, Clone, PartialEq)]
pub struct RecognizedRendezvous {
    /// Sender callsign (the peer we would rendezvous with).
    pub from: String,
    /// Sender grid if the compound frame advertised one.
    pub grid: Option<String>,
    /// The decoded rendezvous message.
    pub msg: RendezvousMsg,
    /// Audio offset the over was heard at (Hz).
    pub base_freq_hz: f32,
}

/// An in-progress over buffered at one audio offset.
#[derive(Debug)]
struct PartialOver {
    base_freq_hz: f32,
    last_slot: u64,
    sender: Option<(String, Option<String>)>,
    target: Option<String>,
    text: String,
}

impl PartialOver {
    fn new(base_freq_hz: f32, now_slot: u64) -> Self {
        Self {
            base_freq_hz,
            last_slot: now_slot,
            sender: None,
            target: None,
            text: String::new(),
        }
    }
}

/// Two audio offsets belong to the same station's over if within `tol` Hz.
fn freq_match(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

/// Buffers JS8 decodes across slots and recognises rendezvous overs directed at our callsign.
pub struct RendezvousAssembler {
    my_callsign: String,
    freq_tol_hz: f32,
    max_over_slots: u64,
    partials: Vec<PartialOver>,
}

impl RendezvousAssembler {
    /// Recognise rendezvous overs addressed to `my_callsign`. `freq_tol_hz` buckets decodes to a
    /// station's stable audio offset; `max_over_slots` evicts an over whose `Last` frame never decoded.
    pub fn new(my_callsign: &str, freq_tol_hz: f32, max_over_slots: u64) -> Self {
        Self {
            my_callsign: my_callsign.trim().to_ascii_uppercase(),
            freq_tol_hz,
            max_over_slots,
            partials: Vec::new(),
        }
    }

    /// Ingest one decoded frame. Returns a [`RecognizedRendezvous`] once a complete over directed at us
    /// has assembled at this offset and its text parses as a rendezvous message.
    pub fn ingest(
        &mut self,
        payload9: &[u8; 9],
        i3bit: u8,
        base_freq_hz: f32,
        now_slot: u64,
    ) -> Option<RecognizedRendezvous> {
        self.sweep(now_slot);
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

        if let Some(r) = self.try_recognize(&self.partials[idx]) {
            self.partials.remove(idx);
            return Some(r);
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
                FrameType::CompoundDirected => p.target = Some(cf.callsign),
                _ => {}
            }
        } else if let Some(t) = unpack_data_message(payload9) {
            if p.text.len() + t.len() <= MAX_TEXT_LEN {
                p.text.push_str(&t);
            }
        }
    }

    /// Recognise a complete over addressed to us whose text is a rendezvous message.
    fn try_recognize(&self, p: &PartialOver) -> Option<RecognizedRendezvous> {
        let (from, grid) = p.sender.clone()?;
        let target = p.target.as_ref()?;
        if !target.eq_ignore_ascii_case(&self.my_callsign) {
            return None;
        }
        let msg = RendezvousMsg::decode(p.text.trim())?;
        Some(RecognizedRendezvous {
            from,
            grid,
            msg,
            base_freq_hz: p.base_freq_hz,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use js8_plugin::{directed, BeaconFrame};

    /// Build the frames for `sender: to <text>` and feed them through the assembler in order, returning
    /// the recognition (if any) produced by the final frame.
    fn run(
        asm: &mut RendezvousAssembler,
        sender: &str,
        to: &str,
        text: &str,
        freq: f32,
        start_slot: u64,
    ) -> Option<RecognizedRendezvous> {
        let frames: Vec<BeaconFrame> = directed(sender, "JN58", to, text);
        assert!(frames.len() >= 3, "sender + target + ≥1 data frame");
        let mut out = None;
        for (i, f) in frames.iter().enumerate() {
            out = asm.ingest(&f.payload, f.i3bit, freq, start_slot + i as u64);
        }
        out
    }

    #[test]
    fn assembles_a_propose_directed_at_us() {
        let mut asm = RendezvousAssembler::new("DC0SK", 3.0, 8);
        let r = run(&mut asm, "KN4CRD", "DC0SK", "OPHF QSY? R7 C3 C9", 1500.0, 0)
            .expect("recognized a rendezvous over addressed to us");
        assert_eq!(r.from, "KN4CRD");
        assert_eq!(r.grid.as_deref(), Some("JN58"));
        assert_eq!(
            r.msg,
            RendezvousMsg::Propose {
                token: "R7".into(),
                channels: vec![3, 9],
            }
        );
    }

    #[test]
    fn an_over_addressed_to_another_station_is_ignored() {
        let mut asm = RendezvousAssembler::new("DC0SK", 3.0, 8);
        // KN4CRD is talking to W1AW, not us.
        assert_eq!(
            run(&mut asm, "KN4CRD", "W1AW", "OPHF QSY? R7 C3", 1500.0, 0),
            None
        );
    }

    #[test]
    fn an_accept_and_a_reject_both_decode() {
        let mut asm = RendezvousAssembler::new("DC0SK", 3.0, 8);
        let acc = run(&mut asm, "KN4CRD", "DC0SK", "OPHF QSY R7 C9 S4", 1000.0, 0)
            .expect("accept recognized");
        assert_eq!(
            acc.msg,
            RendezvousMsg::Accept {
                token: "R7".into(),
                channel: 9,
                switch_in_slots: 4,
            }
        );
        let rej = run(&mut asm, "KN4CRD", "DC0SK", "OPHF NO R7 F", 1000.0, 10)
            .expect("reject recognized");
        assert_eq!(
            rej.msg,
            RendezvousMsg::Reject {
                token: "R7".into(),
                reason: crate::rendezvous::RejectReason::NoCommonFreq,
            }
        );
    }

    #[test]
    fn non_rendezvous_free_text_to_us_is_not_recognized() {
        let mut asm = RendezvousAssembler::new("DC0SK", 3.0, 8);
        assert_eq!(
            run(&mut asm, "KN4CRD", "DC0SK", "HELLO OM", 1500.0, 0),
            None
        );
    }
}
