//! Phase-A state-machine + wire tests: every protocol edge (offer/accept/reject/timeout/cancel/
//! per-block NACK/verify/tamper) exercised deterministically with an injected clock.

use ed25519_dalek::SigningKey;
use openpulse_core::manifest::TransferManifest;
use openpulse_filexfer::{
    block_count, decide, sanitize_filename, CompleteStatus, FileOffer, FxAction, FxFrame,
    OfferDecision, OfferPolicy, Outcome, Reason, ReceiverSession, SenderSession, Timeouts,
    TransferResult,
};

// ── helpers ─────────────────────────────────────────────────────────────────

fn seed(b: u8) -> [u8; 32] {
    let mut s = [0u8; 32];
    s[0] = b;
    s
}

fn pubkey(s: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(s).verifying_key().to_bytes()
}

fn signed_offer(transfer_id: u32, payload: &[u8], block_size: u32, s: &[u8; 32]) -> FileOffer {
    let manifest = TransferManifest::sign(payload, "W1AW", s).expect("sign");
    FileOffer::from_manifest(
        transfer_id,
        &manifest,
        "report.txt",
        "text/plain",
        block_size,
    )
    .expect("build offer")
}

fn accept(id: u32) -> FxFrame {
    FxFrame::FileAccept {
        transfer_id: id,
        have_bitmap: Vec::new(),
    }
}

fn block_ack(id: u32, block: u16, complete: bool, missing: Vec<u8>) -> FxFrame {
    FxFrame::BlockAck {
        transfer_id: id,
        block_index: block,
        complete,
        missing_frag_bitmap: missing,
    }
}

fn finished(actions: &[FxAction]) -> Option<&Outcome> {
    actions.iter().find_map(|a| match a {
        FxAction::Finished(o) => Some(o),
        _ => None,
    })
}

// ── wire codec ──────────────────────────────────────────────────────────────

#[test]
fn wire_roundtrips_every_frame() {
    let s = seed(1);
    let offer = signed_offer(42, b"hello world payload", 1024, &s);
    let frames = vec![
        FxFrame::FileOffer(offer),
        FxFrame::FileAccept {
            transfer_id: 42,
            have_bitmap: vec![0xF0, 0x0F],
        },
        FxFrame::FileReject {
            transfer_id: 42,
            reason: Reason::TooLarge,
        },
        FxFrame::FileData {
            transfer_id: 42,
            block_index: 7,
            packed: vec![1, 2, 3, 4, 5],
        },
        FxFrame::BlockAck {
            transfer_id: 42,
            block_index: 7,
            complete: false,
            missing_frag_bitmap: vec![0b1010_0101],
        },
        FxFrame::FileComplete {
            transfer_id: 42,
            status: CompleteStatus::VerifiedOk,
            countersignature: [9u8; 64],
        },
        FxFrame::FileCancel {
            transfer_id: 42,
            reason: Reason::OperatorCancel,
        },
    ];
    for f in frames {
        let encoded = f.encode();
        let decoded = FxFrame::decode(&encoded).expect("decode");
        assert_eq!(decoded, f, "round-trip mismatch for {f:?}");
    }
}

#[test]
fn decode_rejects_malformed_frames() {
    let good = FxFrame::FileReject {
        transfer_id: 1,
        reason: Reason::Busy,
    }
    .encode();
    // Bad magic.
    let mut bad_magic = good.clone();
    bad_magic[0] = b'X';
    assert!(FxFrame::decode(&bad_magic).is_err());
    // Unknown version.
    let mut bad_ver = good.clone();
    bad_ver[4] = 0x99;
    assert!(FxFrame::decode(&bad_ver).is_err());
    // Unknown type.
    let mut bad_type = good.clone();
    bad_type[5] = 0x7E;
    assert!(FxFrame::decode(&bad_type).is_err());
    // Truncated.
    assert!(FxFrame::decode(&good[..6]).is_err());
    assert!(FxFrame::decode(b"OP").is_err());
}

// ── offer signature + tamper ────────────────────────────────────────────────

#[test]
fn offer_signature_verifies_and_tamper_is_caught() {
    let s = seed(3);
    let offer = signed_offer(1, b"the quick brown fox", 1024, &s);
    assert!(offer.verify_signature(&pubkey(&s)).is_ok());

    // Tampered hash → signature no longer matches the (now different) canonical body.
    let mut tampered = offer.clone();
    tampered.sha256[0] ^= 0xFF;
    assert!(tampered.verify_signature(&pubkey(&s)).is_err());

    // Wrong peer key → rejected.
    assert!(offer.verify_signature(&pubkey(&seed(4))).is_err());
}

// ── acceptance policy ───────────────────────────────────────────────────────

#[test]
fn policy_decides_correctly() {
    let s = seed(3);
    let small = signed_offer(1, &vec![0u8; 500], 1024, &s);
    let big = signed_offer(2, &vec![0u8; 40_000], 1024, &s);

    let disabled = OfferPolicy {
        enabled: false,
        ..OfferPolicy::default()
    };
    assert_eq!(
        decide(&small, &disabled, true),
        OfferDecision::Reject(Reason::FeatureDisabled)
    );

    let cap = OfferPolicy {
        enabled: true,
        max_file_bytes: 1000,
        auto_accept_max_bytes: 0,
        require_verified_peer: true,
    };
    assert_eq!(
        decide(&big, &cap, true),
        OfferDecision::Reject(Reason::TooLarge)
    );
    // Under cap but unverified peer with require=true → untrusted.
    assert_eq!(
        decide(&small, &cap, false),
        OfferDecision::Reject(Reason::UntrustedPeer)
    );
    // Under cap, verified, auto-accept 0 → prompt.
    assert_eq!(decide(&small, &cap, true), OfferDecision::Prompt);

    let permissive = OfferPolicy {
        enabled: true,
        max_file_bytes: 1024 * 1024,
        auto_accept_max_bytes: 4096,
        require_verified_peer: false,
    };
    assert_eq!(
        decide(&small, &permissive, false),
        OfferDecision::AutoAccept
    );
}

// ── sender state machine ────────────────────────────────────────────────────

#[test]
fn sender_happy_path_three_blocks() {
    let s = seed(5);
    let payload = vec![7u8; 2500]; // 3 blocks at 1024
    let offer = signed_offer(100, &payload, 1024, &s);
    assert_eq!(offer.block_count, 3);

    let (mut tx, init) = SenderSession::new(offer.clone(), Timeouts::default(), 0);
    assert_eq!(
        init,
        vec![FxAction::Transmit(FxFrame::FileOffer(offer).encode())]
    );

    // Accept → send block 0.
    let a = tx.apply(&accept(100), 10);
    assert_eq!(
        a,
        vec![
            FxAction::SendBlock {
                block_index: 0,
                missing: None
            },
            FxAction::Progress {
                transfer_id: 100,
                blocks_done: 0,
                blocks_total: 3
            },
        ]
    );

    // Ack block 0 → send block 1; ack 1 → send block 2.
    assert!(matches!(
        tx.apply(&block_ack(100, 0, true, vec![]), 20)[0],
        FxAction::SendBlock { block_index: 1, .. }
    ));
    assert!(matches!(
        tx.apply(&block_ack(100, 1, true, vec![]), 30)[0],
        FxAction::SendBlock { block_index: 2, .. }
    ));

    // Ack final block → only a progress update, awaiting verify.
    let a = tx.apply(&block_ack(100, 2, true, vec![]), 40);
    assert_eq!(
        a,
        vec![FxAction::Progress {
            transfer_id: 100,
            blocks_done: 3,
            blocks_total: 3
        }]
    );
    assert!(!tx.is_terminal());

    // Receiver confirms verified → done.
    let a = tx.apply(
        &FxFrame::FileComplete {
            transfer_id: 100,
            status: CompleteStatus::VerifiedOk,
            countersignature: [0u8; 64],
        },
        50,
    );
    assert_eq!(
        finished(&a).unwrap().result,
        TransferResult::Sent {
            peer_verified: true
        }
    );
    assert!(tx.is_terminal());
}

#[test]
fn sender_reject_ends_transfer() {
    let offer = signed_offer(1, b"data", 1024, &seed(5));
    let (mut tx, _) = SenderSession::new(offer, Timeouts::default(), 0);
    let a = tx.apply(
        &FxFrame::FileReject {
            transfer_id: 1,
            reason: Reason::OperatorDeclined,
        },
        10,
    );
    assert_eq!(
        finished(&a).unwrap().result,
        TransferResult::Rejected {
            reason: Reason::OperatorDeclined
        }
    );
}

#[test]
fn sender_offer_timeout_fails() {
    let offer = signed_offer(1, b"data", 1024, &seed(5));
    let t = Timeouts {
        offer_ms: 1000,
        ..Timeouts::default()
    };
    let (mut tx, _) = SenderSession::new(offer, t, 0);
    assert!(tx.poll_timeout(500).is_empty()); // before deadline
    let a = tx.poll_timeout(1001);
    assert_eq!(
        finished(&a).unwrap().result,
        TransferResult::Failed {
            reason: Reason::Timeout
        }
    );
}

#[test]
fn sender_retransmits_missing_then_stalls() {
    let payload = vec![0u8; 1500]; // 2 blocks
    let offer = signed_offer(1, &payload, 1024, &seed(5));
    let (mut tx, _) = SenderSession::new(offer, Timeouts::default(), 0);
    tx.apply(&accept(1), 0);

    // Four NACKs (max retries) each re-send only the missing fragments.
    for i in 0..4 {
        let a = tx.apply(&block_ack(1, 0, false, vec![0b0000_0010]), 10 + i);
        assert_eq!(
            a,
            vec![FxAction::SendBlock {
                block_index: 0,
                missing: Some(vec![0b0000_0010])
            }]
        );
    }
    // Fifth NACK exhausts retries → stall failure.
    let a = tx.apply(&block_ack(1, 0, false, vec![0b0000_0010]), 20);
    assert_eq!(
        finished(&a).unwrap().result,
        TransferResult::Failed {
            reason: Reason::Stall
        }
    );
}

#[test]
fn sender_cancel_announces_and_finishes() {
    let offer = signed_offer(1, b"data", 1024, &seed(5));
    let (mut tx, _) = SenderSession::new(offer, Timeouts::default(), 0);
    let a = tx.cancel();
    assert_eq!(
        a[0],
        FxAction::Transmit(
            FxFrame::FileCancel {
                transfer_id: 1,
                reason: Reason::OperatorCancel
            }
            .encode()
        )
    );
    assert_eq!(
        finished(&a).unwrap().result,
        TransferResult::Cancelled {
            reason: Reason::OperatorCancel
        }
    );
}

// ── receiver state machine ──────────────────────────────────────────────────

#[test]
fn receiver_auto_accept_receive_verify() {
    let payload = vec![3u8; 2000]; // 2 blocks
    let offer = signed_offer(200, &payload, 1024, &seed(5));
    assert_eq!(offer.block_count, 2);

    let (mut rx, init) =
        ReceiverSession::new(&offer, OfferDecision::AutoAccept, Timeouts::default(), 0);
    assert_eq!(
        init,
        vec![
            FxAction::Transmit(
                FxFrame::FileAccept {
                    transfer_id: 200,
                    have_bitmap: vec![]
                }
                .encode()
            ),
            FxAction::Progress {
                transfer_id: 200,
                blocks_done: 0,
                blocks_total: 2
            },
        ]
    );

    // Block 0 done → progress only; block 1 done → progress + Verify.
    assert_eq!(
        rx.note_block_complete(0, 10),
        vec![FxAction::Progress {
            transfer_id: 200,
            blocks_done: 1,
            blocks_total: 2
        }]
    );
    let a = rx.note_block_complete(1, 20);
    assert!(a.contains(&FxAction::Verify { transfer_id: 200 }));

    // Verified → FileComplete(ok) + received-verified outcome.
    let a = rx.set_verify_result(CompleteStatus::VerifiedOk, [1u8; 64]);
    assert_eq!(
        a[0],
        FxAction::Transmit(
            FxFrame::FileComplete {
                transfer_id: 200,
                status: CompleteStatus::VerifiedOk,
                countersignature: [1u8; 64]
            }
            .encode()
        )
    );
    assert_eq!(
        finished(&a).unwrap().result,
        TransferResult::Received { verified: true }
    );
}

#[test]
fn receiver_verify_failure_reports_unverified() {
    let offer = signed_offer(1, &vec![0u8; 1000], 1024, &seed(5));
    let (mut rx, _) =
        ReceiverSession::new(&offer, OfferDecision::AutoAccept, Timeouts::default(), 0);
    rx.note_block_complete(0, 10); // 1 block
    let a = rx.set_verify_result(CompleteStatus::HashMismatch, [0u8; 64]);
    // FileComplete carries the mismatch status; the outcome is not verified.
    assert!(matches!(
        FxFrame::decode(match &a[0] {
            FxAction::Transmit(b) => b,
            _ => panic!("expected transmit"),
        })
        .unwrap(),
        FxFrame::FileComplete {
            status: CompleteStatus::HashMismatch,
            ..
        }
    ));
    assert_eq!(
        finished(&a).unwrap().result,
        TransferResult::Received { verified: false }
    );
}

#[test]
fn receiver_prompt_then_operator_accepts() {
    let offer = signed_offer(1, &vec![0u8; 1000], 1024, &seed(5));
    let (mut rx, init) =
        ReceiverSession::new(&offer, OfferDecision::Prompt, Timeouts::default(), 0);
    assert_eq!(init, vec![FxAction::Prompt { transfer_id: 1 }]);
    let a = rx.accept(5);
    assert_eq!(
        a[0],
        FxAction::Transmit(
            FxFrame::FileAccept {
                transfer_id: 1,
                have_bitmap: vec![]
            }
            .encode()
        )
    );
}

#[test]
fn receiver_prompt_times_out_to_rejection() {
    let offer = signed_offer(1, &vec![0u8; 1000], 1024, &seed(5));
    let t = Timeouts {
        offer_ms: 1000,
        ..Timeouts::default()
    };
    let (mut rx, _) = ReceiverSession::new(&offer, OfferDecision::Prompt, t, 0);
    assert!(rx.poll_timeout(500).is_empty());
    let a = rx.poll_timeout(1001);
    assert_eq!(
        finished(&a).unwrap().result,
        TransferResult::Rejected {
            reason: Reason::Timeout
        }
    );
}

#[test]
fn receiver_reject_decision_declines_on_air() {
    let offer = signed_offer(1, &vec![0u8; 1000], 1024, &seed(5));
    let (_rx, init) = ReceiverSession::new(
        &offer,
        OfferDecision::Reject(Reason::TooLarge),
        Timeouts::default(),
        0,
    );
    assert_eq!(
        init[0],
        FxAction::Transmit(
            FxFrame::FileReject {
                transfer_id: 1,
                reason: Reason::TooLarge
            }
            .encode()
        )
    );
}

#[test]
fn receiver_cancel_ends_transfer() {
    let offer = signed_offer(1, &vec![0u8; 1000], 1024, &seed(5));
    let (mut rx, _) =
        ReceiverSession::new(&offer, OfferDecision::AutoAccept, Timeouts::default(), 0);
    let a = rx.apply(
        &FxFrame::FileCancel {
            transfer_id: 1,
            reason: Reason::Stall,
        },
        10,
    );
    assert_eq!(
        finished(&a).unwrap().result,
        TransferResult::Cancelled {
            reason: Reason::Stall
        }
    );
}

// ── helpers & sanitization ──────────────────────────────────────────────────

#[test]
fn block_count_math() {
    assert_eq!(block_count(0, 1024), Some(1));
    assert_eq!(block_count(1, 1024), Some(1));
    assert_eq!(block_count(1024, 1024), Some(1));
    assert_eq!(block_count(1025, 1024), Some(2));
    assert_eq!(block_count(2500, 1024), Some(3));
    assert_eq!(block_count(100, 0), None);
    // Audit F-6: a count of 0xFFFF would map the last block's SAR segment id onto the reserved
    // control segment id; the count is capped one below it.
    assert_eq!(
        block_count(0xFFFE * 1024, 1024),
        Some(openpulse_filexfer::MAX_BLOCK_COUNT)
    );
    assert_eq!(block_count(0xFFFF * 1024, 1024), None);
}

#[test]
fn sanitize_defeats_traversal_and_control_chars() {
    assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
    assert_eq!(sanitize_filename("/abs/path/file.txt"), "file.txt");
    assert_eq!(sanitize_filename("C:\\win\\evil.exe"), "evil.exe");
    assert_eq!(sanitize_filename(".."), "received.bin");
    assert_eq!(sanitize_filename(""), "received.bin");
    assert_eq!(sanitize_filename("na\u{0000}me.bin"), "na_me.bin");
    assert!(!sanitize_filename("a/b\\c:d*e?f").contains(['/', '\\', ':', '*', '?']));
}
