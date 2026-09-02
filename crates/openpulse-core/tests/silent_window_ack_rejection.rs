//! The property that lets `decode_fsk4_ack_in_stream` scan without an energy gate (#1177).
//!
//! A silent capture window demodulates to all-zero bytes on every FSK4 tone decision (the Goertzel
//! energies are all exactly 0.0 and the strict comparison picks tone 0). #894 guarded against that
//! with `rms >= 0.3 * peak`, because the zero word was then a *valid* ShortFec+CRC ACK. Wire
//! whitening (#1027) made it invalid, which is what allows the gate — whose threshold refuses real
//! ACKs in band noise — to be removed instead of retuned.
//!
//! That makes the whitening load-bearing for a safety property it was not introduced for, and the
//! keystream has already been changed once (#1148). These tests fail if it changes again.

use openpulse_core::ack::AckFrame;
use openpulse_core::fec::ShortFecCodec;
use openpulse_core::scramble::scrambled;

/// Width of a ShortFec-encoded 5-byte ACK on the wire.
const ACK_WIRE_LEN: usize = 13;

/// Positive control: WITHOUT whitening the all-zero window really does decode to a valid ACK, so the
/// #894 comment described a real defect and this test can distinguish the two regimes. An all-zero
/// word is a codeword of any linear code, and the CRC of zero content is itself zero.
#[test]
fn the_unwhitened_silent_window_really_did_decode_as_a_valid_ack() {
    let silent = vec![0u8; ACK_WIRE_LEN];
    let decoded = ShortFecCodec::new()
        .decode(&silent)
        .expect("the all-zero word is an RS codeword");
    let arr: [u8; 5] = decoded.as_slice().try_into().expect("5-byte ACK payload");
    assert!(
        AckFrame::decode(&arr).is_ok(),
        "pre-#1027 the silent window decoded as a valid ACK — if this stops holding, the control is \
         gone and the sibling test below no longer proves whitening is what refuses it"
    );
}

/// The live path: `stage_demodulate_payload` un-whitens before RS, so the silent window's zero bytes
/// descramble to a non-codeword and RS refuses them. This is the guarantee that replaces the gate.
#[test]
fn silent_window_is_not_a_valid_short_fec_ack() {
    let descrambled = scrambled(&[0u8; ACK_WIRE_LEN]);
    assert!(
        ShortFecCodec::new().decode(&descrambled).is_err(),
        "a silent capture window must not decode as an ACK: {descrambled:02x?}"
    );
}

/// The same for a steady tone, which is the other degenerate window a gate-free scan will now try —
/// every symbol decides the same tone, so the wire is a constant byte.
#[test]
fn a_steady_tone_window_is_not_a_valid_short_fec_ack() {
    for constant in [0x00u8, 0x55, 0xAA, 0xFF] {
        let descrambled = scrambled(&[constant; ACK_WIRE_LEN]);
        assert!(
            ShortFecCodec::new().decode(&descrambled).is_err(),
            "a constant-{constant:#04x} window must not decode as an ACK"
        );
    }
}
