//! FF-16 Phase B acceptance: a file's blocks survive the real modem (framing + RS FEC) as SAR
//! fragments and reassemble + verify; a tampered fragment on the wire fails verification.

use ed25519_dalek::SigningKey;
use openpulse_core::fec::FecMode;
use openpulse_core::manifest::{verify_manifest_with_payload, TransferManifest};
use openpulse_filexfer::{block_count, encode_block, split_blocks, BlockAssembler, FileOffer};
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

const MODE: &str = "QPSK500";

fn seed(b: u8) -> [u8; 32] {
    let mut s = [0u8; 32];
    s[0] = b;
    s
}
fn pubkey(s: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(s).verifying_key().to_bytes()
}

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    }
    h
}

/// Send one SAR fragment through the clean modem loopback and return the decoded bytes.
fn wire(h: &mut ChannelSimHarness, fragment: &[u8]) -> Vec<u8> {
    h.tx_engine
        .transmit_with_fec_mode(fragment, MODE, FecMode::Rs, None)
        .unwrap();
    h.route_clean();
    h.rx_engine
        .receive_with_fec_mode(MODE, FecMode::Rs, None)
        .expect("decode fragment")
}

#[test]
fn file_blocks_survive_the_modem_and_verify() {
    let s = seed(7);
    // A multi-block, compressible payload (Winlink-ish), well over one block.
    let file = "DE W1AW status ok, traffic pending. "
        .repeat(200)
        .into_bytes();
    let block_size = 1024;
    let count = block_count(file.len() as u64, block_size).unwrap();
    assert!(count >= 3, "want several blocks, got {count}");

    let manifest = TransferManifest::sign(&file, "W1AW", &s).unwrap();
    let transfer_id = 0xABCD_1234;
    let _offer = FileOffer::from_manifest(
        transfer_id,
        &manifest,
        "traffic.txt",
        "text/plain",
        block_size,
    )
    .unwrap();

    let mut h = harness();
    let mut asm = BlockAssembler::new(transfer_id, count, block_size, file.len() as u64);
    for (k, block) in split_blocks(&file, block_size).iter().enumerate() {
        for fragment in encode_block(transfer_id, k as u16, block, None).unwrap() {
            let decoded = wire(&mut h, &fragment);
            assert_eq!(
                decoded, fragment,
                "SAR fragment must survive the wire unchanged"
            );
            asm.ingest_fragment(&decoded);
        }
    }

    let got = asm.reassemble().expect("file reassembled");
    assert_eq!(got, file);
    verify_manifest_with_payload(&manifest, &pubkey(&s), &got).expect("signed-manifest verify");
}

#[test]
fn a_tampered_block_on_the_wire_fails_verification() {
    let s = seed(7);
    let file = "payload block ".repeat(300).into_bytes();
    let block_size = 1024;
    let count = block_count(file.len() as u64, block_size).unwrap();
    let manifest = TransferManifest::sign(&file, "W1AW", &s).unwrap();
    let transfer_id = 1;

    let mut h = harness();
    let mut asm = BlockAssembler::new(transfer_id, count, block_size, file.len() as u64);
    let mut tampered_once = false;
    for (k, block) in split_blocks(&file, block_size).iter().enumerate() {
        for fragment in encode_block(transfer_id, k as u16, block, None).unwrap() {
            let mut decoded = wire(&mut h, &fragment);
            // Corrupt one data byte of the first block after it comes off the wire.
            if k == 0 && !tampered_once && decoded.len() > 8 {
                decoded[7] ^= 0xFF;
                tampered_once = true;
            }
            asm.ingest_fragment(&decoded);
        }
    }

    match asm.reassemble() {
        Some(got) => {
            assert_ne!(got, file);
            assert!(verify_manifest_with_payload(&manifest, &pubkey(&s), &got).is_err());
        }
        None => { /* the corrupted block never completed — also a non-delivery, acceptable */ }
    }
    assert!(tampered_once, "the test must have tampered a fragment");
}
