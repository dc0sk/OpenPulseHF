//! JS8 compound-frame grammar (RX side): a decoded 72-bit payload → callsign, frame type, and the
//! grid/number field (JS8Call `unpackCompoundFrame` / `unpackAlphaNumeric50`).
//!
//! The 72-bit payload a heartbeat/compound frame carries lays out as `[flag:3][callsign50:50]
//! [num_hi:11][num_lo:5][bits3:3]`. Since `pack72bits` serializes exactly `value64 ‖ rem8`, the
//! decoder's 9-byte payload is `value64` (big-endian, bytes 0–7) followed by `rem8` (byte 8) — so the
//! fields read straight off the bits, no `alphabet72` char round-trip needed. This is what the
//! discovery MVP uses to learn who it heard and their grid.

use crate::frame::{unpack_callsign, unpack_grid, ALPHANUMERIC};

/// Directed-command values → command string (JS8Call `directed_cmds`, reversed as `QMap::key` does:
/// the lexicographically-smallest key for each value). Index is `packed_cmd % 32`.
const DIRECTED_CMD_BY_VALUE: [&str; 32] = [
    " SNR?",          // 0
    " DIT DIT",       // 1
    " NACK",          // 2
    " HEARING?",      // 3
    " GRID?",         // 4
    ">",              // 5
    " STATUS?",       // 6
    " STATUS",        // 7
    " HEARING",       // 8
    " MSG",           // 9
    " MSG TO:",       // 10
    " QUERY",         // 11
    " QUERY MSGS",    // 12
    " QUERY CALL",    // 13
    " ACK",           // 14
    " GRID",          // 15
    " INFO?",         // 16
    " INFO",          // 17
    " FB",            // 18
    " HW CPY?",       // 19
    " SK",            // 20
    " RR",            // 21
    " QSL?",          // 22
    " QSL",           // 23
    " CMD",           // 24
    " SNR",           // 25
    " NO",            // 26
    " YES",           // 27
    " 73",            // 28
    " HEARTBEAT SNR", // 29
    " AGN?",          // 30
    " ",              // 31
];

/// `<....>` incomplete-callsign sentinel (JS8Call `basecalls`; `nbasecall + 1`). A compound `to` is
/// sent as this placeholder in the directed frame and carried for real in a separate compound frame.
const NBASECALL: u32 = 37 * 36 * 10 * 27 * 27 * 27;

/// Command values whose trailing number is a signed SNR (JS8Call `snr_cmds`).
fn is_snr_cmd_value(v: u8) -> bool {
    v == 25 || v == 29
}

/// Unpack a 28-bit directed from/to callsign, applying the portable `/P` suffix and the `<....>`
/// placeholder (JS8Call `unpackCallsign(value, portable)`; predefined-group basecalls are a follow-on).
fn unpack_callsign_ext(value: u32, portable: bool) -> String {
    if value == NBASECALL + 1 {
        return "<....>".to_string();
    }
    let s = unpack_callsign(value);
    if portable && !s.is_empty() {
        format!("{s}/P")
    } else {
        s
    }
}

/// Format a signed SNR the way JS8Call `formatSNR` does (`+NN` / `-NNN`), or `None` out of ±60.
fn format_snr(snr: i32) -> Option<String> {
    if !(-60..=60).contains(&snr) {
        return None;
    }
    Some(if snr >= 0 {
        format!("+{snr:02}")
    } else {
        format!("{snr:03}")
    })
}

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

/// A decoded directed message (JS8Call `FrameDirected`): sender, target, command, optional number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectedMessage {
    /// Sender callsign (`<....>` if a compound call sent separately).
    pub from: String,
    /// Target callsign (`<....>` if the target is compound — see the paired compound frame).
    pub to: String,
    /// Command, including its leading space (e.g. `" SNR?"`, `" ACK"`).
    pub cmd: String,
    /// Trailing number: a formatted SNR for SNR commands, else a plain signed integer. `None` if absent.
    pub num: Option<String>,
}

/// Unpack a 72-bit directed-message payload (JS8Call `Varicode::unpackDirectedMessage`). Returns
/// `None` if the frame is not a `FrameDirected` (flag 3). Layout: `[flag:3][from:28][to:28][cmd:5]`
/// with the trailing byte `[portable_from:1][portable_to:1][num:6]`.
pub fn unpack_directed_message(payload9: &[u8; 9]) -> Option<DirectedMessage> {
    let value64 = u64::from_be_bytes(payload9[..8].try_into().ok()?);
    let extra = payload9[8];
    let flag = ((value64 >> 61) & 0x7) as u8;
    if flag != 3 {
        return None; // not FrameDirected
    }
    let packed_from = ((value64 >> 33) & 0x0fff_ffff) as u32;
    let packed_to = ((value64 >> 5) & 0x0fff_ffff) as u32;
    let packed_cmd = (value64 & 0x1f) as u8;

    let portable_from = (extra >> 7) & 1 == 1;
    let portable_to = (extra >> 6) & 1 == 1;
    let num_field = extra % 64;

    let cmd = DIRECTED_CMD_BY_VALUE[(packed_cmd % 32) as usize].to_string();
    let num = (num_field != 0).then(|| {
        let n = num_field as i32 - 31;
        if is_snr_cmd_value(packed_cmd % 32) {
            format_snr(n).unwrap_or_default()
        } else {
            n.to_string()
        }
    });

    Some(DirectedMessage {
        from: unpack_callsign_ext(packed_from, portable_from),
        to: unpack_callsign_ext(packed_to, portable_to),
        cmd,
        num,
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

    #[test]
    fn directed_messages_match_upstream() {
        // (payload9, from, to, cmd) from verbatim upstream packDirectedMessage/unpackDirectedMessage.
        for (hex, from, to, cmd) in [
            ("71717ebdf299dde000", "KN4CRD", "W1AW", " SNR?"),
            ("71717ebdf299ddee00", "KN4CRD", "W1AW", " ACK"),
            ("6b43a9551717ebc400", "DC0SK", "KN4CRD", " GRID?"),
            ("6b43a955f299ddfc00", "DC0SK", "W1AW", " 73"),
            ("71717ebcb43a955600", "KN4CRD", "DC0SK", " QSL?"),
            ("7f299ddf1717ebd300", "W1AW", "KN4CRD", " HW CPY?"),
            ("71717ebdef34ac3000", "KN4CRD", "N0P", " INFO?"),
            ("6b43a955ec8e3bb400", "DC0SK", "G0ABC", " SK"),
            ("71717ebdf299ddfb00", "KN4CRD", "W1AW", " YES"),
            ("71717ebdf299ddfa00", "KN4CRD", "W1AW", " NO"),
        ] {
            let dm = unpack_directed_message(&hex9(hex)).expect("directed frame");
            assert_eq!(dm.from, from, "from {hex}");
            assert_eq!(dm.to, to, "to {hex}");
            assert_eq!(dm.cmd, cmd, "cmd {hex}");
            assert_eq!(dm.num, None, "num {hex}");
        }
    }

    #[test]
    fn directed_snr_numbers_match_upstream() {
        for (hex, cmd, num) in [
            ("71717ebdf299ddf915", " SNR", "-10"),
            ("71717ebdf299ddf924", " SNR", "+05"),
            ("6b43a955f299ddf91a", " SNR", "-05"),
            ("71717ebdf299ddfd21", " HEARTBEAT SNR", "+02"),
        ] {
            let dm = unpack_directed_message(&hex9(hex)).expect("directed frame");
            assert_eq!(dm.cmd, cmd, "cmd {hex}");
            assert_eq!(dm.num.as_deref(), Some(num), "num {hex}");
        }
    }

    #[test]
    fn non_directed_frame_returns_none() {
        // A heartbeat (flag 0) is not a directed frame.
        assert_eq!(unpack_directed_message(&hex9("0a2fb3a3ee2ee2ea58")), None);
    }
}
