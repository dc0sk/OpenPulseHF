//! JS8 compound-frame grammar (RX side): a decoded 72-bit payload → callsign, frame type, and the
//! grid/number field (JS8Call `unpackCompoundFrame` / `unpackAlphaNumeric50`).
//!
//! The 72-bit payload a heartbeat/compound frame carries lays out as `[flag:3][callsign50:50]
//! [num_hi:11][num_lo:5][bits3:3]`. Since `pack72bits` serializes exactly `value64 ‖ rem8`, the
//! decoder's 9-byte payload is `value64` (big-endian, bytes 0–7) followed by `rem8` (byte 8) — so the
//! fields read straight off the bits, no `alphabet72` char round-trip needed. This is what the
//! discovery MVP uses to learn who it heard and their grid.

use crate::frame::{unpack_grid, ALPHANUMERIC};

/// JS8 frame type (leading 3 payload bits; JS8Call `Varicode::FrameType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// `@HB` heartbeat (or `@CQ` when the alt bit is set).
    Heartbeat,
    /// Compound-callsign partial.
    Compound,
    /// Compound directed.
    CompoundDirected,
    /// Directed (not a compound frame — carries a command).
    Directed,
    /// Free-text / data (`10X`/`11X`).
    Data,
}

impl FrameType {
    fn from_flag(flag: u8) -> Self {
        match flag {
            0 => FrameType::Heartbeat,
            1 => FrameType::Compound,
            2 => FrameType::CompoundDirected,
            3 => FrameType::Directed,
            _ => FrameType::Data,
        }
    }
}

/// A decoded compound frame: the sender callsign, its frame type, and the raw 16-bit num/grid field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundFrame {
    /// Frame type.
    pub frame_type: FrameType,
    /// Sender callsign (spaces removed; may carry a `/` compound separator).
    pub callsign: String,
    /// The 16-bit num field (a packed grid for a heartbeat).
    pub num: u16,
}

/// A decoded heartbeat: sender, 4-char grid (empty if none), and whether it is the `@CQ` variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heartbeat {
    /// Sender callsign.
    pub callsign: String,
    /// Maidenhead grid, or empty if the sender advertised none.
    pub grid: String,
    /// `true` for the `@CQ` alt-heartbeat, `false` for `@HB`.
    pub is_cq: bool,
}

/// Unpack a 50-bit alphanumeric callsign (JS8Call `unpackAlphaNumeric50`). Positions 3 and 7 are `/`
/// compound separators; padding spaces are stripped.
pub fn unpack_alphanumeric50(mut packed: u64) -> String {
    let idx = |t: u64| ALPHANUMERIC[t as usize] as char;
    let mut word = [' '; 11];
    word[10] = idx(packed % 38);
    packed /= 38;
    word[9] = idx(packed % 38);
    packed /= 38;
    word[8] = idx(packed % 38);
    packed /= 38;
    word[7] = if packed % 2 == 1 { '/' } else { ' ' };
    packed /= 2;
    word[6] = idx(packed % 38);
    packed /= 38;
    word[5] = idx(packed % 38);
    packed /= 38;
    word[4] = idx(packed % 38);
    packed /= 38;
    word[3] = if packed % 2 == 1 { '/' } else { ' ' };
    packed /= 2;
    word[2] = idx(packed % 38);
    packed /= 38;
    word[1] = idx(packed % 38);
    packed /= 38;
    word[0] = idx(packed % 39); // upstream quirk: position 0 uses modulo 39, not 38
    word.iter().filter(|&&c| c != ' ').collect()
}

/// Parse a decoded 72-bit payload (`payload9`) as a compound frame. Returns `None` for a `Directed`
/// or `Data` flag (those are not compound frames — matches `unpackCompoundFrame`).
pub fn unpack_compound_frame(payload9: &[u8; 9]) -> Option<CompoundFrame> {
    let value64 = u64::from_be_bytes(payload9[..8].try_into().ok()?);
    let packed_8 = payload9[8];
    let flag = ((value64 >> 61) & 0x7) as u8;
    if flag == 3 || flag == 4 {
        return None;
    }
    let callsign50 = (value64 >> 11) & ((1u64 << 50) - 1);
    let packed_11 = (value64 & 0x7FF) as u16;
    let packed_5 = (packed_8 >> 3) as u16;
    let num = (packed_11 << 5) | packed_5;
    Some(CompoundFrame {
        frame_type: FrameType::from_flag(flag),
        callsign: unpack_alphanumeric50(callsign50),
        num,
    })
}

/// Interpret a compound frame as a heartbeat (grid in the low 15 bits, `@CQ` alt bit at 0x8000).
/// Returns `None` if the frame is not a heartbeat.
pub fn parse_heartbeat(frame: &CompoundFrame) -> Option<Heartbeat> {
    if frame.frame_type != FrameType::Heartbeat {
        return None;
    }
    Some(Heartbeat {
        callsign: frame.callsign.clone(),
        grid: unpack_grid(frame.num & 0x7FFF),
        is_cq: frame.num & 0x8000 != 0,
    })
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

    #[test]
    fn unpack_alphanumeric50_matches_upstream() {
        // Values from verbatim `Varicode::packAlphaNumeric50` on Qt.
        assert_eq!(unpack_alphanumeric50(358_399_795_381_724), "KN4CRD");
        assert_eq!(unpack_alphanumeric50(231_581_663_745_228), "DC0SK");
        assert_eq!(unpack_alphanumeric50(557_100_718_697_932), "W1AW");
    }

    #[test]
    fn decodes_heartbeat_payloads_from_upstream() {
        // 72-bit heartbeat payloads assembled by the verbatim upstream packer on Qt.
        for (hex, call, grid) in [
            ("0a2fb3a3ee2ee2ea58", "KN4CRD", "EM73"),
            ("0694fa766ea661dcd0", "DC0SK", "JN58"),
            ("0fd570f3896e62ce70", "W1AW", "FN20"),
        ] {
            let frame = unpack_compound_frame(&hex9(hex)).expect("compound frame");
            assert_eq!(frame.frame_type, FrameType::Heartbeat);
            assert_eq!(frame.callsign, call);
            let hb = parse_heartbeat(&frame).expect("heartbeat");
            assert_eq!(hb.grid, grid, "grid for {call}");
            assert!(!hb.is_cq);
        }
    }

    #[test]
    fn full_rx_pipeline_decodes_who_and_where() {
        // Build a heartbeat payload → info bits → LDPC → tones → GFSK audio → decode_window → grammar,
        // proving the receiver reads the sender callsign + grid off the air end to end.
        use crate::costas::CostasKind;
        use crate::decoder::{decode_window, DecodeCfg};
        use crate::message::js8_info_bits;
        use crate::modulate::{modulate_tones, GfskParams};
        use crate::submode::{params, Submode};
        use crate::tones::message_to_tones;

        let sm = params(Submode::Normal);
        let payload = hex9("0a2fb3a3ee2ee2ea58"); // KN4CRD EM73
        let info = js8_info_bits(&payload, 0);
        let audio = modulate_tones(
            &message_to_tones(&info, CostasKind::Original),
            1500.0,
            &GfskParams::from_submode(&sm),
        );
        let cfg = DecodeCfg {
            base_min: 1490.0,
            base_max: 1510.0,
            base_step: 3.125,
            max_offset: 0,
            offset_step: 1,
            ..DecodeCfg::default()
        };
        let decodes = decode_window(&audio, &sm, &cfg);
        let hb = decodes
            .iter()
            .find_map(|d| {
                unpack_compound_frame(&d.payload)
                    .as_ref()
                    .and_then(parse_heartbeat)
            })
            .expect("a heartbeat decoded from the air");
        assert_eq!(hb.callsign, "KN4CRD");
        assert_eq!(hb.grid, "EM73");
    }
}
