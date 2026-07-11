//! JS8 frame encoders (TX side) — the pack counterparts of [`crate::grammar`]'s decoders.
//!
//! `pack_alphanumeric50` (50-bit compound callsign, JS8Call `packAlphaNumeric50`) and
//! `pack_compound_frame` (`Heartbeat`/`Compound`/`CompoundDirected` 72-bit payloads,
//! `packCompoundFrame`) build the frames the discovery beacon transmits. Validated against the
//! verbatim upstream compiled with real Qt5 (ground-truth vectors in the `tests` module) and
//! round-tripped through the RX decoders.

use crate::frame::{pack_grid, ALPHANUMERIC};
use crate::grammar::FrameType;

fn alnum_index(c: u8) -> u64 {
    ALPHANUMERIC.iter().position(|&x| x == c).unwrap_or(0) as u64
}

/// Pack a callsign/group into its 50-bit compound value (JS8Call `packAlphaNumeric50`). Positions 3
/// and 7 are `/` compound separators; the string is filtered to `[A-Z0-9 /@]` and space-padded to 11.
pub fn pack_alphanumeric50(value: &str) -> u64 {
    let mut word: Vec<u8> = value
        .bytes()
        .filter(|&b| {
            b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' ' || b == b'/' || b == b'@'
        })
        .collect();
    if word.len() > 3 && word[3] != b'/' {
        word.insert(3, b' ');
    }
    if word.len() > 7 && word[7] != b'/' {
        word.insert(7, b' ');
    }
    while word.len() < 11 {
        word.push(b' ');
    }

    let is_slash = |c: u8| (c == b'/') as u64;
    // Mixed-radix weights, verbatim from upstream (positions 3 and 7 are radix-2 `/` flags).
    (38 * 38 * 38 * 2 * 38 * 38 * 38 * 2 * 38 * 38) * alnum_index(word[0])
        + (38 * 38 * 38 * 2 * 38 * 38 * 38 * 2 * 38) * alnum_index(word[1])
        + (38 * 38 * 38 * 2 * 38 * 38 * 38 * 2) * alnum_index(word[2])
        + (38 * 38 * 38 * 2 * 38 * 38 * 38) * is_slash(word[3])
        + (38 * 38 * 38 * 2 * 38 * 38) * alnum_index(word[4])
        + (38 * 38 * 38 * 2 * 38) * alnum_index(word[5])
        + (38 * 38 * 38 * 2) * alnum_index(word[6])
        + (38 * 38 * 38) * is_slash(word[7])
        + (38 * 38) * alnum_index(word[8])
        + 38 * alnum_index(word[9])
        + alnum_index(word[10])
}

/// Pack a compound frame (JS8Call `packCompoundFrame`): `[flag:3][callsign50:50][num_hi:11]` in the
/// 64-bit value and `[num_lo:5][bits3:3]` in the trailing byte. Returns `None` for a non-compound
/// flag (`Directed`/`Data`) or an unpackable callsign.
pub fn pack_compound_frame(
    callsign: &str,
    frame_type: FrameType,
    num: u16,
    bits3: u8,
) -> Option<[u8; 9]> {
    let flag = match frame_type {
        FrameType::Heartbeat => 0u64,
        FrameType::Compound => 1,
        FrameType::CompoundDirected => 2,
        FrameType::Directed | FrameType::Data => return None,
    };
    let callsign50 = pack_alphanumeric50(callsign);
    if callsign50 == 0 {
        return None;
    }
    let packed_11 = ((num >> 5) & 0x7ff) as u64;
    let packed_5 = (num & 0x1f) as u8;
    let packed_8 = (packed_5 << 3) | (bits3 & 0x7);

    let value64 = (flag << 61) | (callsign50 << 11) | packed_11;
    let mut payload = [0u8; 9];
    payload[..8].copy_from_slice(&value64.to_be_bytes());
    payload[8] = packed_8;
    Some(payload)
}

/// Build a plain `@HB HEARTBEAT` frame for `callsign` at `grid` (JS8Call `packHeartbeatMessage`, the
/// non-alt heartbeat: grid packed into the extra field, status bits 0). A grid shorter than 4 chars
/// packs the "no grid" sentinel.
pub fn pack_heartbeat_frame(callsign: &str, grid: &str) -> Option<[u8; 9]> {
    let packed_grid = if grid.trim().chars().count() == 4 {
        pack_grid(grid)
    } else {
        (1u16 << 15) - 1 // nmaxgrid → empty grid on decode
    };
    pack_compound_frame(callsign, FrameType::Heartbeat, packed_grid, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::{parse_heartbeat, unpack_alphanumeric50, unpack_compound_frame};

    fn hex9(s: &str) -> [u8; 9] {
        let mut p = [0u8; 9];
        for (i, b) in p.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        p
    }

    #[test]
    fn pack_alphanumeric50_matches_upstream_and_round_trips() {
        // Values from verbatim `Varicode::packAlphaNumeric50` on real Qt.
        for (call, packed) in [
            ("KN4CRD", 358_399_795_381_724u64),
            ("DC0SK", 231_581_663_745_228),
            ("W1AW", 557_100_718_697_932),
        ] {
            assert_eq!(pack_alphanumeric50(call), packed, "pack {call}");
            assert_eq!(unpack_alphanumeric50(packed), call, "round-trip {call}");
        }
    }

    #[test]
    fn heartbeat_frame_matches_upstream_ground_truth() {
        // The C-2 upstream heartbeat vector: KN4CRD @HB HEARTBEAT EM73.
        let frame = pack_heartbeat_frame("KN4CRD", "EM73").expect("heartbeat");
        assert_eq!(frame, hex9("0a2fb3a3ee2ee2ea58"));
    }

    #[test]
    fn packed_heartbeat_decodes_back_to_sender_and_grid() {
        for (call, grid) in [("DC0SK", "JN58"), ("W1AW", "FN20"), ("KN4CRD", "EM73")] {
            let frame = pack_heartbeat_frame(call, grid).unwrap();
            let cf = unpack_compound_frame(&frame).expect("compound");
            assert_eq!(cf.frame_type, FrameType::Heartbeat);
            assert_eq!(cf.callsign, call);
            let hb = parse_heartbeat(&cf).expect("heartbeat");
            assert_eq!(hb.grid, grid);
            assert!(!hb.is_cq);
        }
    }

    #[test]
    fn opulse_group_packs_and_round_trips() {
        // @OPULSE is the compound-directed target for the capability hint.
        let cf = pack_compound_frame("@OPULSE", FrameType::CompoundDirected, 0, 0).unwrap();
        let back = unpack_compound_frame(&cf).unwrap();
        assert_eq!(back.frame_type, FrameType::CompoundDirected);
        assert_eq!(back.callsign, "@OPULSE");
    }

    #[test]
    fn directed_and_data_flags_are_rejected() {
        assert!(pack_compound_frame("KN4CRD", FrameType::Directed, 0, 0).is_none());
        assert!(pack_compound_frame("KN4CRD", FrameType::Data, 0, 0).is_none());
    }
}
