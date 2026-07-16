//! ACK frame taxonomy for HPX sessions.
//!
//! ACK frames carry control information between IRS and ISS.  They are
//! transported over a separate 4FSK waveform (see `fsk4-plugin`) that is
//! decodable at lower SNR than the data modulation, giving ≈ 6 dB headroom.
//!
//! ## Wire layout (5 bytes)
//!
//! ```text
//! byte 0: ACK type [2:0], has_reverse_ack [3], has_recommended_level [4], reserved [7:5]
//! bytes 1–2: session_hash u16 big-endian  (anti-collision)
//! byte 3: recommended_level [7:3] (SpeedLevel 1–20 when has_recommended_level=1, else 0),
//!         reverse_ack [2:0] (when has_reverse_ack=1, else 0)
//!         — backward-compatible: old receivers ignore this byte; CRC still validates
//! byte 4: CRC-8/SMBUS over bytes 0–3
//! ```
//!
//! `recommended_level` is the receiver-led, absolute target the data receiver wants
//! the sender to transmit at (OTA rate lockstep).  Absolute (not a relative step) so a
//! lost ACK can't accumulate drift: the next ACK simply re-states the target.

use serde::{Deserialize, Serialize};

use crate::error::AckError;
use crate::rate::SpeedLevel;

// ── AckType ───────────────────────────────────────────────────────────────────

/// HPX ACK frame type (3-bit wire code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum AckType {
    /// Data received correctly; maintain current speed level.
    AckOk = 0b000,
    /// Data received correctly; request one speed-level step up.
    AckUp = 0b001,
    /// Data received correctly; request one speed-level step down.
    AckDown = 0b010,
    /// Data received with uncorrectable errors; retransmit at current rate.
    Nack = 0b011,
    /// Changeover: IRS requests to become ISS.
    Break = 0b100,
    /// ACK lost; repeat last data frame.
    Req = 0b101,
    /// Graceful session end.
    Qrt = 0b110,
    /// Abnormal teardown.
    Abort = 0b111,
}

impl AckType {
    fn from_u8(v: u8) -> Result<Self, AckError> {
        match v {
            0 => Ok(Self::AckOk),
            1 => Ok(Self::AckUp),
            2 => Ok(Self::AckDown),
            3 => Ok(Self::Nack),
            4 => Ok(Self::Break),
            5 => Ok(Self::Req),
            6 => Ok(Self::Qrt),
            7 => Ok(Self::Abort),
            _ => Err(AckError::InvalidAckType(v)),
        }
    }
}

// ── AckFrame ──────────────────────────────────────────────────────────────────

/// Five-byte ACK frame payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckFrame {
    /// Frame type.
    pub ack_type: AckType,
    /// 16-bit FNV-1a hash of the session ID for anti-collision filtering.
    pub session_hash: u16,
    /// Sender's assessment of the *incoming* path quality (reverse direction).
    ///
    /// When set, the sender piggybacks its own RX quality report so the peer
    /// can update its own [`crate::rate::RateAdapter`] for the reverse direction without a separate frame.
    /// `None` encodes as byte 3 low bits = 0 (backward compatible with old receivers).
    pub reverse_ack: Option<AckType>,
    /// Receiver-led absolute rate target: the speed level the data receiver wants
    /// the sender to transmit at next (OTA rate lockstep).  `None` for legacy frames.
    pub recommended_level: Option<SpeedLevel>,
}

impl AckFrame {
    /// Create a frame with the session hash computed from `session_id`.
    pub fn new(ack_type: AckType, session_id: &str) -> Self {
        Self {
            ack_type,
            session_hash: Self::hash_session_id(session_id),
            reverse_ack: None,
            recommended_level: None,
        }
    }

    /// Create a frame that also carries a reverse-direction quality report.
    pub fn new_with_reverse(ack_type: AckType, session_id: &str, reverse_ack: AckType) -> Self {
        Self {
            ack_type,
            session_hash: Self::hash_session_id(session_id),
            reverse_ack: Some(reverse_ack),
            recommended_level: None,
        }
    }

    /// Builder: attach a receiver-led absolute rate recommendation.
    pub fn with_recommended_level(mut self, level: SpeedLevel) -> Self {
        self.recommended_level = Some(level);
        self
    }

    /// Encode to the 5-byte wire representation.
    pub fn encode(&self) -> [u8; 5] {
        let has_rev = self.reverse_ack.is_some() as u8;
        let has_rec = self.recommended_level.is_some() as u8;
        let b0 = (self.ack_type as u8) | (has_rev << 3) | (has_rec << 4);
        let sh = self.session_hash.to_be_bytes();
        let rev = self.reverse_ack.map_or(0, |a| a as u8) & 0x07;
        let rec = self.recommended_level.map_or(0, |l| l.as_u8()) & 0x1F;
        let b3 = (rec << 3) | rev;
        let payload = [b0, sh[0], sh[1], b3];
        let crc = crc8(&payload);
        [b0, sh[0], sh[1], b3, crc]
    }

    /// Decode from the 5-byte wire representation.
    pub fn decode(b: &[u8; 5]) -> Result<Self, AckError> {
        let expected = crc8(&b[..4]);
        if b[4] != expected {
            return Err(AckError::CrcMismatch {
                expected,
                got: b[4],
            });
        }
        let ack_type = AckType::from_u8(b[0] & 0x07)?;
        let has_rev = (b[0] >> 3) & 1 != 0;
        let has_rec = (b[0] >> 4) & 1 != 0;
        let session_hash = ((b[1] as u16) << 8) | b[2] as u16;
        let reverse_ack = if has_rev {
            Some(AckType::from_u8(b[3] & 0x07)?)
        } else {
            None
        };
        let recommended_level = if has_rec {
            let code = (b[3] >> 3) & 0x1F;
            Some(SpeedLevel::from_u8(code).ok_or(AckError::InvalidSpeedLevel(code))?)
        } else {
            None
        };
        Ok(Self {
            ack_type,
            session_hash,
            reverse_ack,
            recommended_level,
        })
    }

    /// Compute a 16-bit FNV-1a hash of `session_id` for anti-collision use.
    pub fn hash_session_id(session_id: &str) -> u16 {
        let mut hash: u32 = 2_166_136_261;
        for byte in session_id.bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(16_777_619);
        }
        (hash ^ (hash >> 16)) as u16
    }

    /// Encode to the 5-byte wire representation with a keyed 24-bit authentication tag instead of the
    /// public FNV `session_hash` + CRC (E7). The frame stays exactly 5 bytes — bytes 1,2 and byte 4 hold
    /// a truncated HMAC-SHA256 over the content bytes (0 and 3), keyed by the per-session key derived at
    /// the handshake ([`crate::session_key`]). A listener without the key cannot forge a valid ACK, and
    /// the tag doubles as the anti-collision filter (a co-channel session has a different key). The tag
    /// also subsumes the CRC (a MAC detects corruption). `session_hash` is not carried in this mode.
    pub fn encode_authenticated(&self, key: &[u8; 32]) -> [u8; 5] {
        let has_rev = self.reverse_ack.is_some() as u8;
        let has_rec = self.recommended_level.is_some() as u8;
        let b0 = (self.ack_type as u8) | (has_rev << 3) | (has_rec << 4);
        let rev = self.reverse_ack.map_or(0, |a| a as u8) & 0x07;
        let rec = self.recommended_level.map_or(0, |l| l.as_u8()) & 0x1F;
        let b3 = (rec << 3) | rev;
        let tag = mac24(key, b0, b3);
        [b0, tag[0], tag[1], b3, tag[2]]
    }

    /// Decode a 5-byte ACK carrying a keyed authentication tag (see [`encode_authenticated`]). Verifies
    /// the 24-bit MAC against `key` before parsing; a wrong key (foreign session) or forged frame fails
    /// with [`AckError::MacMismatch`]. The returned frame's `session_hash` is 0 (not carried).
    ///
    /// [`encode_authenticated`]: AckFrame::encode_authenticated
    pub fn decode_authenticated(b: &[u8; 5], key: &[u8; 32]) -> Result<Self, AckError> {
        let expected = mac24(key, b[0], b[3]);
        if [b[1], b[2], b[4]] != expected {
            return Err(AckError::MacMismatch);
        }
        let ack_type = AckType::from_u8(b[0] & 0x07)?;
        let has_rev = (b[0] >> 3) & 1 != 0;
        let has_rec = (b[0] >> 4) & 1 != 0;
        let reverse_ack = if has_rev {
            Some(AckType::from_u8(b[3] & 0x07)?)
        } else {
            None
        };
        let recommended_level = if has_rec {
            let code = (b[3] >> 3) & 0x1F;
            Some(SpeedLevel::from_u8(code).ok_or(AckError::InvalidSpeedLevel(code))?)
        } else {
            None
        };
        Ok(Self {
            ack_type,
            session_hash: 0,
            reverse_ack,
            recommended_level,
        })
    }

    /// Encode with a keyed tag when `key` is `Some`, else the legacy CRC form.
    pub fn encode_maybe_authenticated(&self, key: Option<&[u8; 32]>) -> [u8; 5] {
        match key {
            Some(k) => self.encode_authenticated(k),
            None => self.encode(),
        }
    }

    /// Decode with keyed-tag verification when `key` is `Some`, else the legacy CRC form.
    pub fn decode_maybe_authenticated(
        b: &[u8; 5],
        key: Option<&[u8; 32]>,
    ) -> Result<Self, AckError> {
        match key {
            Some(k) => Self::decode_authenticated(b, k),
            None => Self::decode(b),
        }
    }
}

/// Truncated (24-bit) HMAC-SHA256 over the two ACK content bytes, keyed by the session key.
fn mac24(key: &[u8; 32], b0: u8, b3: u8) -> [u8; 3] {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&[b0, b3]);
    let out = mac.finalize().into_bytes();
    [out[0], out[1], out[2]]
}

// ── Diversity ACK decode ────────────────────────────────────────────────────────

/// Recover a repeated [`AckFrame`] from `k` per-copy soft-LLR observations of the same ShortFec-coded frame
/// (each `copy` is that copy's bit-LLR stream, engine convention: LSB-first, negative = bit 1).
///
/// Uses the #694 **union** rule — decode each copy standalone first, and fall back to the MAP-sum only if
/// none decodes alone — so success is a strict superset of every single copy and a clean copy is never
/// diluted by a faded one (which plain LLR-summing would do). Each candidate is ShortFec-corrected then
/// CRC-validated, so a wrong-lock mis-correction is rejected rather than returned.
///
/// Measured (K=3, ~0.5 s inter-copy spacing over the constant-envelope 500 Hz `MFSK16-ACK`): clears ≥ 0.99
/// at 3 dB **below** the MFSK16 data floor on moderate_f1/poor_f1, where a single 1.28 s ACK holds only
/// ~0.6 there — see `plugins/mfsk16/src/robust_ack.rs` and `docs/dev/research/robust-narrowband-measurement.md`.
pub fn decode_ack_from_llr_copies(copies: &[&[f32]]) -> Option<AckFrame> {
    let fec = crate::fec::ShortFecCodec::new();
    let one = |llrs: &[f32]| -> Option<AckFrame> {
        let data = fec.decode(&crate::fec::hard_decide(llrs)).ok()?;
        let arr: [u8; 5] = data.as_slice().try_into().ok()?;
        AckFrame::decode(&arr).ok()
    };
    for &c in copies {
        if let Some(f) = one(c) {
            return Some(f);
        }
    }
    (copies.len() >= 2)
        .then(|| one(&crate::fec::combine_llrs_map(copies)))
        .flatten()
}

/// As [`decode_ack_from_llr_copies`], but verifies the keyed authentication tag with `key` when `Some`
/// (E7) instead of the legacy CRC. With `None` it is identical to [`decode_ack_from_llr_copies`].
pub fn decode_ack_from_llr_copies_maybe_auth(
    copies: &[&[f32]],
    key: Option<&[u8; 32]>,
) -> Option<AckFrame> {
    let fec = crate::fec::ShortFecCodec::new();
    let one = |llrs: &[f32]| -> Option<AckFrame> {
        let data = fec.decode(&crate::fec::hard_decide(llrs)).ok()?;
        let arr: [u8; 5] = data.as_slice().try_into().ok()?;
        AckFrame::decode_maybe_authenticated(&arr, key).ok()
    };
    for &c in copies {
        if let Some(f) = one(c) {
            return Some(f);
        }
    }
    (copies.len() >= 2)
        .then(|| one(&crate::fec::combine_llrs_map(copies)))
        .flatten()
}

// ── CRC-8/SMBUS ───────────────────────────────────────────────────────────────

fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticated_ack_round_trips_and_rejects_wrong_key() {
        let key = [7u8; 32];
        let frame = AckFrame {
            ack_type: AckType::AckDown,
            session_hash: 0, // not carried in authenticated mode
            reverse_ack: Some(AckType::AckOk),
            recommended_level: SpeedLevel::from_u8(6),
        };
        let wire = frame.encode_authenticated(&key);
        let decoded = AckFrame::decode_authenticated(&wire, &key).expect("valid tag");
        assert_eq!(decoded.ack_type, AckType::AckDown);
        assert_eq!(decoded.reverse_ack, Some(AckType::AckOk));
        assert_eq!(decoded.recommended_level, SpeedLevel::from_u8(6));

        // A different session key rejects it (also the anti-collision filter).
        let wrong = [9u8; 32];
        assert_eq!(
            AckFrame::decode_authenticated(&wire, &wrong),
            Err(AckError::MacMismatch)
        );
    }

    #[test]
    fn tampering_an_authenticated_ack_fails_verification() {
        let key = [3u8; 32];
        let frame = AckFrame {
            ack_type: AckType::AckOk,
            session_hash: 0,
            reverse_ack: None,
            recommended_level: SpeedLevel::from_u8(10),
        };
        let mut wire = frame.encode_authenticated(&key);
        // An attacker flips the recommendation to a much higher rate (byte 3 carries the level).
        wire[3] ^= 0xF8;
        assert_eq!(
            AckFrame::decode_authenticated(&wire, &key),
            Err(AckError::MacMismatch)
        );
        // Flipping the ack_type byte is also caught.
        let mut wire2 = frame.encode_authenticated(&key);
        wire2[0] ^= 0x03;
        assert!(AckFrame::decode_authenticated(&wire2, &key).is_err());
    }

    #[test]
    fn maybe_authenticated_falls_back_to_legacy_without_key() {
        let frame = AckFrame::new(AckType::AckUp, "sess-1");
        // No key → legacy CRC path, round-trips including session_hash.
        let wire = frame.encode_maybe_authenticated(None);
        let decoded = AckFrame::decode_maybe_authenticated(&wire, None).unwrap();
        assert_eq!(decoded.ack_type, AckType::AckUp);
        assert_eq!(decoded.session_hash, frame.session_hash);
    }

    #[test]
    fn ack_frame_round_trip_all_types() {
        for t in [
            AckType::AckOk,
            AckType::AckUp,
            AckType::AckDown,
            AckType::Nack,
            AckType::Break,
            AckType::Req,
            AckType::Qrt,
            AckType::Abort,
        ] {
            let f = AckFrame {
                ack_type: t,
                session_hash: 0xABCD,
                reverse_ack: None,
                recommended_level: None,
            };
            let b = f.encode();
            assert_eq!(AckFrame::decode(&b).unwrap(), f);
        }
    }

    #[test]
    fn ack_frame_with_reverse_ack_round_trips() {
        let f = AckFrame::new_with_reverse(AckType::AckOk, "sess", AckType::AckUp);
        let b = f.encode();
        let decoded = AckFrame::decode(&b).unwrap();
        assert_eq!(decoded.ack_type, AckType::AckOk);
        assert_eq!(decoded.reverse_ack, Some(AckType::AckUp));
    }

    #[test]
    fn ack_frame_recommended_level_round_trips() {
        use crate::rate::SpeedLevel;
        for lvl in [
            SpeedLevel::Sl1,
            SpeedLevel::Sl8,
            SpeedLevel::Sl15,
            SpeedLevel::Sl20,
        ] {
            let f = AckFrame::new(AckType::AckOk, "sess").with_recommended_level(lvl);
            let b = f.encode();
            let d = AckFrame::decode(&b).unwrap();
            assert_eq!(d.recommended_level, Some(lvl));
            assert_eq!(d.ack_type, AckType::AckOk);
        }
    }

    #[test]
    fn ack_frame_recommended_level_coexists_with_reverse_ack() {
        use crate::rate::SpeedLevel;
        let f = AckFrame::new_with_reverse(AckType::AckUp, "sess", AckType::AckDown)
            .with_recommended_level(SpeedLevel::Sl11);
        let d = AckFrame::decode(&f.encode()).unwrap();
        assert_eq!(d.ack_type, AckType::AckUp);
        assert_eq!(d.reverse_ack, Some(AckType::AckDown));
        assert_eq!(d.recommended_level, Some(SpeedLevel::Sl11));
    }

    #[test]
    fn ack_frame_without_recommended_level_keeps_high_bits_clear() {
        let f = AckFrame::new(AckType::AckOk, "sess");
        let b = f.encode();
        assert_eq!(b[0] & 0x10, 0, "has_recommended_level flag must be 0");
        assert_eq!(
            b[3] & 0xF8,
            0,
            "byte 3 high bits must be 0 without a recommendation"
        );
    }

    #[test]
    fn ack_frame_without_reverse_ack_is_backward_compatible() {
        // A frame with no reverse_ack must have byte 0 bit 3 = 0 and byte 3 = 0,
        // matching the old wire format exactly.
        let f = AckFrame::new(AckType::AckDown, "sess");
        let b = f.encode();
        assert_eq!(b[0] & 0x08, 0, "has_reverse_ack flag must be 0");
        assert_eq!(b[3], 0, "byte 3 must be 0 without reverse_ack");
    }

    #[test]
    fn ack_frame_crc_mismatch_detected() {
        let mut b = AckFrame {
            ack_type: AckType::AckOk,
            session_hash: 0,
            reverse_ack: None,
            recommended_level: None,
        }
        .encode();
        b[4] ^= 0xFF;
        assert!(matches!(
            AckFrame::decode(&b),
            Err(AckError::CrcMismatch { .. })
        ));
    }

    #[test]
    fn session_hash_is_deterministic() {
        assert_eq!(
            AckFrame::hash_session_id("sess-abc"),
            AckFrame::hash_session_id("sess-abc")
        );
    }

    #[test]
    fn different_sessions_have_different_hashes() {
        assert_ne!(
            AckFrame::hash_session_id("sess-A"),
            AckFrame::hash_session_id("sess-B")
        );
    }

    #[test]
    fn new_constructor_hashes_session_id() {
        let f = AckFrame::new(AckType::AckUp, "session-xyz");
        assert_eq!(f.session_hash, AckFrame::hash_session_id("session-xyz"));
        assert_eq!(f.reverse_ack, None);
    }

    // ── diversity ACK decode ──

    fn coded() -> Vec<u8> {
        crate::fec::ShortFecCodec::new()
            .encode(&AckFrame::new(AckType::AckOk, "sess").encode())
            .expect("encode") // 13 bytes
    }

    /// Coded bytes → clean bit-LLRs (LSB-first, negative = bit 1), magnitude `mag`.
    fn clean_llrs(mag: f32) -> Vec<f32> {
        let mut v = Vec::new();
        for &b in &coded() {
            for i in 0..8 {
                v.push(if (b >> i) & 1 == 1 { -mag } else { mag });
            }
        }
        v
    }

    /// Sign-flip every bit of the given byte range (a fully-wrong byte burst).
    fn corrupt(clean: &[f32], bytes: std::ops::Range<usize>) -> Vec<f32> {
        let mut v = clean.to_vec();
        for byte in bytes {
            for i in 0..8 {
                v[byte * 8 + i] = -v[byte * 8 + i];
            }
        }
        v
    }

    #[test]
    fn ack_union_recovers_from_one_clean_copy() {
        let clean = clean_llrs(8.0);
        let garbage = corrupt(&clean, 0..13); // every byte wrong → fails alone
        let copies = [garbage.as_slice(), clean.as_slice(), garbage.as_slice()];
        assert_eq!(
            decode_ack_from_llr_copies(&copies),
            Some(AckFrame::new(AckType::AckOk, "sess"))
        );
    }

    #[test]
    fn ack_union_recovers_via_map_sum_when_no_copy_decodes_alone() {
        let clean = clean_llrs(8.0);
        // Each copy has 5 wrong bytes (> t=4 → fails alone); the MAP-sum leaves only bytes 4 and 8 wrong
        // (each corrupted in two of three copies) → 2 ≤ t=4 → RS corrects → decodes.
        let c0 = corrupt(&clean, 0..5);
        let c1 = corrupt(&clean, 4..9);
        let c2 = corrupt(&clean, 8..13);
        assert!(decode_ack_from_llr_copies(&[c0.as_slice()]).is_none());
        assert!(decode_ack_from_llr_copies(&[c1.as_slice()]).is_none());
        assert!(decode_ack_from_llr_copies(&[c2.as_slice()]).is_none());
        assert_eq!(
            decode_ack_from_llr_copies(&[c0.as_slice(), c1.as_slice(), c2.as_slice()]),
            Some(AckFrame::new(AckType::AckOk, "sess"))
        );
    }

    #[test]
    fn ack_union_single_clean_copy_decodes() {
        let clean = clean_llrs(8.0);
        assert_eq!(
            decode_ack_from_llr_copies(&[clean.as_slice()]),
            Some(AckFrame::new(AckType::AckOk, "sess"))
        );
    }

    #[test]
    fn ack_union_all_garbage_and_empty_return_none() {
        let clean = clean_llrs(8.0);
        let g = corrupt(&clean, 0..13);
        assert_eq!(
            decode_ack_from_llr_copies(&[g.as_slice(), g.as_slice()]),
            None
        );
        assert_eq!(decode_ack_from_llr_copies(&[]), None);
    }
}
