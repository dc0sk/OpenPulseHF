//! End-to-end session compression: a `pack`ed payload must survive modem framing + FEC over the wire
//! and `unpack` back to the original, and the compressed frame must actually be smaller than the raw
//! bytes it replaces (the point of enabling it).

use openpulse_core::compression::{pack, unpack};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

const MODE: &str = "QPSK500";

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    }
    h
}

#[test]
fn packed_payload_survives_the_modem_and_unpacks() {
    // A realistic, highly compressible session payload (repeated Winlink-ish header text).
    let raw = "DE N0CALL TO N0CALL SUBJECT: status ok status ok status ok status ok status ok"
        .repeat(8)
        .into_bytes();
    let packed = pack(&raw);
    assert!(
        packed.len() < raw.len(),
        "packed {} should be smaller than raw {}",
        packed.len(),
        raw.len()
    );

    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(&packed, MODE, FecMode::Rs, None)
        .unwrap();
    h.route_clean();
    let decoded = h
        .rx_engine
        .receive_with_fec_mode(MODE, FecMode::Rs, None)
        .expect("decode the packed frame");

    // The binary packed frame arrives byte-for-byte, and unpacks to the original payload.
    assert_eq!(
        decoded, packed,
        "packed bytes must survive the wire unchanged"
    );
    assert_eq!(unpack(&decoded).expect("unpack"), raw);
}

#[test]
fn an_unpacked_payload_passes_through_the_rx_seam_untouched() {
    // Mirrors the daemon rx tick: a non-packed frame (compression disabled on the sender) has no magic,
    // so `unpack` returns None and the caller keeps the original bytes.
    let raw = b"plain uncompressed session body".to_vec();
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(&raw, MODE, FecMode::Rs, None)
        .unwrap();
    h.route_clean();
    let decoded = h
        .rx_engine
        .receive_with_fec_mode(MODE, FecMode::Rs, None)
        .expect("decode");
    let recovered = unpack(&decoded).unwrap_or(decoded);
    assert_eq!(recovered, raw);
}
