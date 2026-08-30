//! #1201 — the station identity must survive the wire intact, or be refused.
//!
//! The predecessor silently truncated an over-length `sender_id` on a char boundary and then signed
//! the truncated form, so a station whose callsign exceeded the cap shipped a validly-signed
//! identity that was not its own. Two properties are pinned here: an identity at the cap round-trips
//! byte-for-byte, and one above it is REFUSED rather than shortened.

use openpulse_core::handshake_wire::caps;
use openpulse_core::manifest::TransferManifest;
use openpulse_filexfer::{FileOffer, FxFrame, SenderId, MAX_BLOCK_SIZE, MIN_BLOCK_SIZE};

fn seed() -> [u8; 32] {
    let mut s = [0u8; 32];
    s[0] = 7;
    s
}

/// The cap this crate enforces must be able to hold anything the handshake can verify. If these
/// drift apart again, a callsign that handshakes successfully cannot be named in an offer — which
/// is exactly how #1201 presented, as an UntrustedPeer rejection between two verified stations.
#[test]
fn the_identity_cap_covers_every_callsign_the_handshake_accepts() {
    let at_handshake_cap = "A".repeat(caps::STATION_ID);
    assert!(
        SenderId::new(&at_handshake_cap).is_ok(),
        "an id the handshake would accept ({} bytes) must be nameable in an offer",
        caps::STATION_ID
    );
}

/// The defect's own codec case: an id LONGER than the old 16-byte cap but legal at the handshake
/// must come back byte-for-byte, not as a prefix. Asserting against the INPUT rather than
/// round-trip stability is the point — encode/decode agreed with each other while both truncated.
#[test]
fn an_identity_over_the_old_cap_survives_encode_and_decode_intact() {
    // 17 is the reachable window: over the old 16-byte cap that truncated, at or under the
    // handshake's cap so the peer can actually be verified. The assertion below can fail — if
    // STATION_ID ever drops, this fixture stops exercising the defect and should say so.
    let id = "A".repeat(17);
    assert!(
        id.len() <= caps::STATION_ID,
        "fixture id is {} bytes, over the handshake cap of {} — no longer a reachable case",
        id.len(),
        caps::STATION_ID
    );
    let manifest = TransferManifest::sign(b"payload", &id, &seed()).unwrap();
    let offer = FileOffer::from_manifest(
        1,
        &manifest,
        "f.bin",
        "application/octet-stream",
        MIN_BLOCK_SIZE,
        &seed(),
    )
    .expect("a 17-byte id is within the shared cap");

    let bytes = FxFrame::FileOffer(offer).encode();
    let decoded = match FxFrame::decode(&bytes).expect("round-trip") {
        FxFrame::FileOffer(o) => o,
        other => panic!("expected FileOffer, got {other:?}"),
    };
    assert_eq!(
        decoded.sender_id.as_str(),
        id,
        "the decoded identity must equal the INPUT, not a prefix of it"
    );
}

/// Above the cap the answer is refusal, at both doors that can construct one.
#[test]
fn an_over_cap_identity_is_refused_not_truncated() {
    let too_long = "A".repeat(caps::STATION_ID + 1);

    let err = SenderId::new(&too_long).expect_err("over-cap must be refused at the constructor");
    assert!(
        format!("{err}").contains("sender_id"),
        "the error must name the field; got {err}"
    );

    let manifest = TransferManifest::sign(b"payload", &too_long, &seed()).unwrap();
    assert!(
        FileOffer::from_manifest(
            1,
            &manifest,
            "f.bin",
            "application/octet-stream",
            MIN_BLOCK_SIZE,
            &seed()
        )
        .is_none(),
        "from_manifest must refuse rather than ship a truncated identity"
    );
}

/// A maximal legal offer must still fit ONE SAR fragment. Without this, the next cap bump crosses
/// 251 bytes silently and every offer becomes a multi-fragment transmission on a fading channel.
/// The handshake got the same assert in #1147; this is the filexfer twin.
#[test]
fn a_maximal_offer_fits_one_sar_fragment() {
    const SAR_FRAGMENT_PAYLOAD: usize = 251;
    let id = "A".repeat(caps::STATION_ID);
    let manifest = TransferManifest::sign(b"payload", &id, &seed()).unwrap();
    let offer = FileOffer::from_manifest(
        u32::MAX,
        &manifest,
        // longest name/mime the encoder will emit, and the largest legal block size
        &"n".repeat(64),
        &"m".repeat(32),
        MAX_BLOCK_SIZE,
        &seed(),
    )
    .expect("maximal legal offer");

    let encoded = FxFrame::FileOffer(offer).encode();
    assert!(
        encoded.len() <= SAR_FRAGMENT_PAYLOAD,
        "a maximal offer is {} bytes, over the {SAR_FRAGMENT_PAYLOAD}-byte single-fragment budget",
        encoded.len()
    );
}
