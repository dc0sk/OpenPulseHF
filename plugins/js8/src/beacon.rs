//! JS8 beacon assembly (TX): build the frame sequences a discovery station transmits and synthesise
//! their GFSK audio.
//!
//! Two beacons: a self-identifying `@HB HEARTBEAT` (one frame), and the `@OPULSE` capability hint (a
//! compound-directed *over* — a `Compound` frame carrying the sender + grid, a `CompoundDirected`
//! frame carrying the `@OPULSE` target, then Huffman `Data` frames carrying the free text). Each frame
//! is one 79-symbol transmission; the scheduler sends them one per slot. Frames round-trip through the
//! RX decoder + [`crate::grammar`] and the `@OPULSE` sequence is recognised by the discovery
//! HintAssembler (loopback-tested).

use crate::costas::CostasKind;
use crate::encode::{pack_compound_frame, pack_heartbeat_frame, pack_huff_frame};
use crate::frame::pack_grid;
use crate::grammar::FrameType;
use crate::message::js8_info_bits;
use crate::modulate::{modulate_tones, GfskParams};
use crate::submode::{params, Submode};
use crate::tones::message_to_tones;

/// One transmitted JS8 frame: its 72-bit payload and its `i3bit` transmission flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeaconFrame {
    /// 72-bit message payload (9 bytes).
    pub payload: [u8; 9],
    /// Transmission flag: `1` = First, `2` = Last, `0` = middle/standalone.
    pub i3bit: u8,
}

/// A self-identifying `@HB HEARTBEAT <grid>` beacon — a single frame (§97.119: the callsign is on air).
/// Returns empty if the callsign does not pack.
pub fn heartbeat(callsign: &str, grid: &str) -> Vec<BeaconFrame> {
    match pack_heartbeat_frame(callsign, grid) {
        Some(payload) => vec![BeaconFrame { payload, i3bit: 0 }],
        None => Vec::new(),
    }
}

/// An `@OPULSE` capability-hint over: `SENDER: @OPULSE <free_text>`. `free_text` is the OPHF hint the
/// discovery layer produced. Returns empty if the sender does not pack.
pub fn opulse_hint(sender: &str, grid: &str, free_text: &str) -> Vec<BeaconFrame> {
    build_over(sender, grid, "@OPULSE", free_text)
}

/// A directed free-text over: `SENDER: TO <free_text>` as a `Compound` (sender+grid) +
/// `CompoundDirected` (`to`) + Huffman `Data` frame sequence, `First`/`Last`-bracketed. Used for
/// rendezvous — `to` is the peer callsign and `free_text` the OPHF rendezvous message. Returns empty
/// if either callsign does not pack.
pub fn directed(sender: &str, grid: &str, to: &str, free_text: &str) -> Vec<BeaconFrame> {
    build_over(sender, grid, to, free_text)
}

/// Build a `Compound(sender+grid) + CompoundDirected(target) + Huffman(free_text)` over, bracketed with
/// the `First`/`Last` transmission flags. Empty if either callsign fails to pack.
fn build_over(sender: &str, grid: &str, target: &str, free_text: &str) -> Vec<BeaconFrame> {
    let Some(sender_frame) = pack_compound_frame(sender, FrameType::Compound, pack_grid(grid), 0)
    else {
        return Vec::new();
    };
    let Some(target_frame) = pack_compound_frame(target, FrameType::CompoundDirected, 0, 0) else {
        return Vec::new();
    };

    let mut frames = vec![
        BeaconFrame {
            payload: sender_frame,
            i3bit: 0,
        },
        BeaconFrame {
            payload: target_frame,
            i3bit: 0,
        },
    ];

    // Huffman-pack the free text across as many data frames as it needs.
    let mut rest = free_text;
    while !rest.is_empty() {
        let (payload, n) = pack_huff_frame(rest);
        if n == 0 {
            break; // an un-encodable character — stop rather than loop forever
        }
        frames.push(BeaconFrame { payload, i3bit: 0 });
        rest = &rest[n..];
    }

    // Bracket the over: First on the leading frame, Last on the final one.
    if let Some(first) = frames.first_mut() {
        first.i3bit = 1;
    }
    if let Some(last) = frames.last_mut() {
        last.i3bit = 2;
    }
    frames
}

/// Synthesise the GFSK audio for one beacon frame at `base_freq_hz` in the given submode (one 79-symbol
/// transmission).
pub fn frame_audio(frame: &BeaconFrame, base_freq_hz: f32, submode: Submode) -> Vec<f32> {
    let sm = params(submode);
    let info = js8_info_bits(&frame.payload, frame.i3bit);
    let tones = message_to_tones(&info, CostasKind::Original);
    modulate_tones(&tones, base_freq_hz, &GfskParams::from_submode(&sm))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{decode_window, DecodeCfg};
    use crate::grammar::{parse_heartbeat, unpack_compound_frame};

    #[test]
    fn heartbeat_beacon_transmits_and_decodes_off_air() {
        let frames = heartbeat("DC0SK", "JN58");
        assert_eq!(frames.len(), 1);
        let audio = frame_audio(&frames[0], 1500.0, Submode::Normal);

        let cfg = DecodeCfg {
            base_min: 1490.0,
            base_max: 1510.0,
            ..DecodeCfg::default()
        };
        let decodes = decode_window(&audio, &params(Submode::Normal), &cfg);
        let hb = decodes
            .iter()
            .find_map(|d| {
                unpack_compound_frame(&d.payload)
                    .as_ref()
                    .and_then(parse_heartbeat)
            })
            .expect("heartbeat decoded off air");
        assert_eq!(hb.callsign, "DC0SK");
        assert_eq!(hb.grid, "JN58");
    }

    #[test]
    fn opulse_hint_builds_a_first_last_bracketed_over() {
        let frames = opulse_hint("DC0SK", "JN58", "OPHF1 1FAX3AIT");
        assert!(frames.len() >= 3, "sender + @OPULSE + ≥1 data frame");
        assert_eq!(frames.first().unwrap().i3bit, 1, "First");
        assert_eq!(frames.last().unwrap().i3bit, 2, "Last");
        // Frame 0 is the sender, frame 1 the @OPULSE target.
        let sender = unpack_compound_frame(&frames[0].payload).unwrap();
        assert_eq!(sender.callsign, "DC0SK");
        assert_eq!(sender.frame_type, FrameType::Compound);
        let target = unpack_compound_frame(&frames[1].payload).unwrap();
        assert_eq!(target.callsign, "@OPULSE");
        assert_eq!(target.frame_type, FrameType::CompoundDirected);
    }

    #[test]
    fn directed_over_carries_sender_target_and_free_text() {
        use crate::varicode::unpack_data_message;
        let text = "OPHF QSY? R7 C3 C9";
        let frames = directed("DC0SK", "JN58", "KN4CRD", text);
        assert!(frames.len() >= 3, "sender + target + ≥1 data frame");
        assert_eq!(frames.first().unwrap().i3bit, 1, "First");
        assert_eq!(frames.last().unwrap().i3bit, 2, "Last");

        // Frame 0 = sender, frame 1 = the directed target callsign (not a group).
        let sender = unpack_compound_frame(&frames[0].payload).unwrap();
        assert_eq!(sender.callsign, "DC0SK");
        assert_eq!(sender.frame_type, FrameType::Compound);
        let target = unpack_compound_frame(&frames[1].payload).unwrap();
        assert_eq!(target.callsign, "KN4CRD");
        assert_eq!(target.frame_type, FrameType::CompoundDirected);

        // The Huffman data frames reassemble to the original free text.
        let recovered: String = frames[2..]
            .iter()
            .filter_map(|f| unpack_data_message(&f.payload))
            .collect();
        assert_eq!(recovered, text);
    }
}
