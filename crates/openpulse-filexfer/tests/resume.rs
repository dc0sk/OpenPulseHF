//! Phase-E resume mechanic: the sender skips blocks the receiver already holds (via
//! `FileAccept.have_bitmap`), and the receiver announces its held blocks + counts them done.

use openpulse_core::manifest::TransferManifest;
use openpulse_filexfer::{
    CompleteStatus, FileOffer, FxAction, FxFrame, OfferDecision, ReceiverSession, SenderSession,
    Timeouts,
};

fn offer(block_count_hint_bytes: usize, block_size: u32) -> FileOffer {
    let mut seed = [0u8; 32];
    seed[0] = 7;
    let payload = vec![0u8; block_count_hint_bytes];
    let manifest = TransferManifest::sign(&payload, "W1AW", &seed).unwrap();
    FileOffer::from_manifest(
        1,
        &manifest,
        "f.bin",
        "application/octet-stream",
        block_size,
        &seed,
    )
    .unwrap()
}

fn accept_with(transfer_id: u32, have_bitmap: Vec<u8>) -> FxFrame {
    FxFrame::FileAccept {
        transfer_id,
        have_bitmap,
    }
}

fn block_ack(transfer_id: u32, block: u16) -> FxFrame {
    FxFrame::BlockAck {
        transfer_id,
        block_index: block,
        complete: true,
        missing_frag_bitmap: vec![],
    }
}

fn first_sendblock(actions: &[FxAction]) -> Option<u16> {
    actions.iter().find_map(|a| match a {
        FxAction::SendBlock { block_index, .. } => Some(*block_index),
        _ => None,
    })
}

#[test]
fn sender_skips_held_blocks_and_advances_over_them() {
    let o = offer(2500, 1024); // 3 blocks
    assert_eq!(o.block_count, 3);
    let id = o.transfer_id;
    let (mut tx, _) = SenderSession::new(o, Timeouts::default(), 0);

    // Receiver already holds block 0 → the sender starts at block 1.
    let a = tx.apply(&accept_with(id, vec![0b0000_0001]), 10);
    assert_eq!(first_sendblock(&a), Some(1));

    // Ack block 1 → send block 2 (block 0 stays skipped).
    let a = tx.apply(&block_ack(id, 1), 20);
    assert_eq!(first_sendblock(&a), Some(2));

    // Ack block 2 → nothing left to send; await verify.
    let a = tx.apply(&block_ack(id, 2), 30);
    assert!(first_sendblock(&a).is_none());
    assert!(!tx.is_terminal());
}

#[test]
fn sender_with_all_blocks_held_sends_nothing() {
    let o = offer(2500, 1024); // 3 blocks
    let id = o.transfer_id;
    let (mut tx, _) = SenderSession::new(o, Timeouts::default(), 0);
    // All three blocks held (bits 0,1,2).
    let a = tx.apply(&accept_with(id, vec![0b0000_0111]), 10);
    assert!(
        first_sendblock(&a).is_none(),
        "nothing to send when all held"
    );
    // FileComplete then completes the transfer.
    let a = tx.apply(
        &FxFrame::FileComplete {
            transfer_id: id,
            status: CompleteStatus::VerifiedOk,
            countersignature: [0u8; 64],
        },
        20,
    );
    assert!(tx.is_terminal(), "await-verify → done on FileComplete");
    assert!(a.iter().any(|x| matches!(x, FxAction::Finished(_))));
}

#[test]
fn receiver_announces_held_bitmap_and_counts_them_done() {
    let o = offer(2500, 1024); // 3 blocks
    let id = o.transfer_id;
    let held = [true, false, false]; // block 0 already on disk
    let (mut rx, actions) =
        ReceiverSession::resume(&o, OfferDecision::AutoAccept, &held, Timeouts::default(), 0);

    // The emitted FileAccept must carry bit 0.
    let accept = actions
        .iter()
        .find_map(|a| match a {
            FxAction::Transmit(b) => FxFrame::decode(b).ok(),
            _ => None,
        })
        .expect("FileAccept emitted");
    match accept {
        FxFrame::FileAccept { have_bitmap, .. } => {
            assert_eq!(have_bitmap, vec![0b0000_0001], "block 0 announced held")
        }
        other => panic!("expected FileAccept, got {other:?}"),
    }

    // Only blocks 1 and 2 remain; completing them triggers Verify (block 0 was pre-counted).
    assert!(!rx
        .note_block_complete(1, 10)
        .iter()
        .any(|a| matches!(a, FxAction::Verify { .. })));
    let a = rx.note_block_complete(2, 20);
    assert!(a.iter().any(|a| matches!(a, FxAction::Verify { .. })));

    // Verify → FileComplete + terminal.
    let a = rx.set_verify_result(CompleteStatus::VerifiedOk, [0u8; 64]);
    assert!(rx.is_terminal());
    assert!(a.iter().any(|x| matches!(x, FxAction::Finished(_))));
    let _ = id;
}

#[test]
fn receiver_with_all_blocks_held_goes_straight_to_verify() {
    let o = offer(2500, 1024); // 3 blocks
    let held = [true, true, true];
    let (_rx, actions) =
        ReceiverSession::resume(&o, OfferDecision::AutoAccept, &held, Timeouts::default(), 0);
    assert!(
        actions.iter().any(|a| matches!(a, FxAction::Verify { .. })),
        "an all-held resume verifies immediately without receiving anything"
    );
}
