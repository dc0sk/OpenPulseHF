//! Phase-B block layer: split/pack/SAR round-trips, the >64 005 B multi-object case, integrity
//! failure on a tampered fragment, and selective retransmission via the missing-fragment bitmap.

use ed25519_dalek::SigningKey;
use openpulse_core::manifest::{verify_manifest_with_payload, TransferManifest};
use openpulse_filexfer::{
    block_count, encode_block, split_blocks, BlockAssembler, BlockEvent, FileOffer,
};

fn seed(b: u8) -> [u8; 32] {
    let mut s = [0u8; 32];
    s[0] = b;
    s
}
fn pubkey(s: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(s).verifying_key().to_bytes()
}

/// High-entropy, incompressible bytes (so `pack()` keeps the block large enough to span fragments).
fn incompressible(n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    let mut state: u32 = 0x1234_5678;
    for _ in 0..n {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        v.push((state >> 24) as u8);
    }
    v
}

/// Encode every block of `file` and return the flat list of transmittable SAR fragments (in order).
fn all_fragments(transfer_id: u32, file: &[u8], block_size: u32) -> Vec<Vec<u8>> {
    split_blocks(file, block_size)
        .iter()
        .enumerate()
        .flat_map(|(k, block)| encode_block(transfer_id, k as u16, block, None).expect("encode"))
        .collect()
}

#[test]
fn split_blocks_math() {
    assert_eq!(split_blocks(b"", 1024), vec![&[] as &[u8]]);
    let f = vec![0u8; 2500];
    let blocks = split_blocks(&f, 1024);
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].len(), 1024);
    assert_eq!(blocks[2].len(), 452);
}

#[test]
fn an_oversized_block_is_rejected() {
    // Audit F-1: the offer geometry declares one 1024-byte block, but a peer sends a block that
    // unpacks to 4000 bytes. It must be dropped (not stored), so a small, quota-approved offer can't
    // be inflated into an arbitrarily large on-disk file.
    let oversized = incompressible(4000);
    let mut asm = BlockAssembler::new(1, 1, 1024, 1024);
    let mut last = BlockEvent::Progress { block_index: 0 };
    for frag in encode_block(1, 0, &oversized, None).expect("encode") {
        last = asm.ingest_fragment(&frag);
    }
    assert_eq!(
        last,
        BlockEvent::Ignored,
        "an over-length block must be dropped, not completed"
    );
    assert!(
        !asm.is_complete(),
        "the transfer must not count as complete"
    );
    assert!(
        asm.reassemble().is_none(),
        "no oversized payload may be reassembled"
    );
}

#[test]
fn single_block_roundtrips_and_verifies() {
    let s = seed(9);
    let file = b"a short compressible file, repeated. ".repeat(3);
    let manifest = TransferManifest::sign(&file, "W1AW", &s).unwrap();
    let offer = FileOffer::from_manifest(1, &manifest, "f.txt", "text/plain", 1024).unwrap();
    assert_eq!(
        offer.block_count,
        block_count(file.len() as u64, 1024).unwrap()
    );

    let mut asm = BlockAssembler::new(1, offer.block_count, offer.block_size, offer.file_size);
    for frag in all_fragments(1, &file, 1024) {
        asm.ingest_fragment(&frag);
    }
    let got = asm.reassemble().expect("reassembled");
    assert_eq!(got, file);
    verify_manifest_with_payload(&manifest, &pubkey(&s), &got).expect("verify");
}

#[test]
fn multi_object_over_64kb_roundtrips_out_of_order() {
    let s = seed(9);
    let file = incompressible(100_000); // > 64 005 → cannot be one SAR object
    let block_size = 16_384;
    let count = block_count(file.len() as u64, block_size).unwrap();
    assert_eq!(count, 7);
    let manifest = TransferManifest::sign(&file, "W1AW", &s).unwrap();

    let mut frags = all_fragments(2, &file, block_size);
    frags.reverse(); // deliver everything in reverse — SAR + block map must not care

    let mut asm = BlockAssembler::new(2, count, block_size, file.len() as u64);
    let mut completes = 0;
    for frag in &frags {
        if let BlockEvent::Complete { .. } = asm.ingest_fragment(frag) {
            completes += 1;
        }
    }
    assert_eq!(completes, count);
    assert!(asm.is_complete());
    let got = asm.reassemble().expect("reassembled");
    assert_eq!(got, file);
    verify_manifest_with_payload(&manifest, &pubkey(&s), &got).expect("verify");
}

#[test]
fn tampered_fragment_fails_verification() {
    let s = seed(9);
    let file = incompressible(20_000);
    let block_size = 4096;
    let count = block_count(file.len() as u64, block_size).unwrap();
    let manifest = TransferManifest::sign(&file, "W1AW", &s).unwrap();

    let mut frags = all_fragments(3, &file, block_size);
    // Flip a data byte in one fragment (past the 4-byte SAR header).
    let victim = frags.len() / 2;
    frags[victim][SAR_HEADER_TAMPER_OFFSET] ^= 0xFF;

    let mut asm = BlockAssembler::new(3, count, block_size, file.len() as u64);
    for frag in &frags {
        asm.ingest_fragment(frag);
    }
    // Either the block reassembles to wrong bytes, or a block never completes — both must fail verify.
    match asm.reassemble() {
        Some(got) => {
            assert_ne!(got, file);
            assert!(verify_manifest_with_payload(&manifest, &pubkey(&s), &got).is_err());
        }
        None => { /* incomplete transfer also fails to deliver — acceptable */ }
    }
}

const SAR_HEADER_TAMPER_OFFSET: usize = 6; // 4-byte SAR header + 2 bytes into the data

#[test]
fn missing_bitmap_drives_selective_retransmit() {
    let block = incompressible(600); // packs to ~605 B → 3 SAR fragments
    let all = encode_block(4, 0, &block, None).unwrap();
    assert!(
        all.len() >= 2,
        "need a multi-fragment block, got {}",
        all.len()
    );

    let mut asm = BlockAssembler::new(4, 1, 1024, block.len() as u64);
    // Deliver only the first fragment.
    assert_eq!(
        asm.ingest_fragment(&all[0]),
        BlockEvent::Progress { block_index: 0 }
    );

    // The missing bitmap must flag every fragment except #0.
    let missing = asm.missing_bitmap(0);
    assert!(!bit(&missing, 0), "fragment 0 arrived");
    for i in 1..all.len() {
        assert!(bit(&missing, i), "fragment {i} should be missing");
    }

    // Retransmit only the missing fragments → block completes.
    let resend = encode_block(4, 0, &block, Some(&missing)).unwrap();
    assert_eq!(resend.len(), all.len() - 1);
    let mut last = BlockEvent::Ignored;
    for frag in &resend {
        last = asm.ingest_fragment(frag);
    }
    assert_eq!(last, BlockEvent::Complete { block_index: 0 });
    assert_eq!(asm.reassemble().unwrap(), block);
}

fn bit(bitmap: &[u8], i: usize) -> bool {
    bitmap.get(i / 8).is_some_and(|b| b & (1 << (i % 8)) != 0)
}

#[test]
fn seeded_blocks_complete_without_fragments() {
    // A 3-block transfer where block 0 and block 2 were persisted from an interrupted run.
    let file: Vec<u8> = (0..2500u32).map(|i| i as u8).collect();
    let blocks = split_blocks(&file, 1024);
    assert_eq!(blocks.len(), 3);

    let mut asm = BlockAssembler::new(7, 3, 1024, file.len() as u64);
    asm.seed_block(0, blocks[0].to_vec());
    asm.seed_block(2, blocks[2].to_vec());
    assert_eq!(asm.block(0), Some(blocks[0]));
    assert_eq!(asm.block(1), None, "block 1 was not seeded");
    assert!(!asm.is_complete(), "block 1 still missing");

    // Only the middle block needs to arrive over the air.
    let frags = encode_block(7, 1, blocks[1], None).unwrap();
    let mut last = BlockEvent::Ignored;
    for f in &frags {
        last = asm.ingest_fragment(f);
    }
    assert_eq!(last, BlockEvent::Complete { block_index: 1 });
    assert!(asm.is_complete());
    assert_eq!(asm.reassemble().unwrap(), file);

    // Seeding an out-of-range index is a no-op.
    asm.seed_block(9, vec![1, 2, 3]);
    assert_eq!(asm.block(9), None);
}
