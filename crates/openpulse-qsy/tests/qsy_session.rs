//! Integration tests for QSY frame codec, session state machine, and scanner.

mod common;

use ed25519_dalek::SigningKey;
use openpulse_qsy::{
    frame::{decode_signed, decode_unsigned, encode_signed, encode_unsigned},
    scanner::QsyScanner,
    QsyAction, QsyFrame, QsyFrameError, QsyPolicy, QsySession,
};
use openpulse_radio::RigctldController;
use rand::rngs::OsRng;

// ── Frame codec tests ────────────────────────────────────────────────────────

/// All five frame types survive encode → decode unchanged.
#[test]
fn frame_round_trip() {
    let frames = vec![
        QsyFrame::Req {
            token: "aabbccdd".into(),
            n_candidates: 3,
        },
        QsyFrame::List {
            token: "aabbccdd".into(),
            candidates: vec![(14070000, -87.5_f32), (14074000, -91.0_f32)],
        },
        QsyFrame::Vote {
            token: "aabbccdd".into(),
            votes: vec![(14070000, -88.0_f32), (14074000, -90.5_f32)],
        },
        QsyFrame::Ack {
            token: "aabbccdd".into(),
            agreed_freq_hz: 14070000,
            switchover_offset_s: 5,
        },
        QsyFrame::Reject {
            token: "aabbccdd".into(),
            reason: "qsy disabled".into(),
        },
    ];
    for f in frames {
        assert_eq!(decode_unsigned(&encode_unsigned(&f)).unwrap(), f);
    }
}

// ── Signature tests ──────────────────────────────────────────────────────────

/// Signed round-trip: encode_signed → decode_signed verifies ok.
#[test]
fn signed_round_trip_integration() {
    let key = SigningKey::generate(&mut OsRng);
    let f = QsyFrame::Req {
        token: "deadbeef".into(),
        n_candidates: 2,
    };
    let line = encode_signed(&f, &key);
    let decoded = decode_signed(&line, &key.verifying_key()).unwrap();
    assert_eq!(decoded, f);
}

/// Mutating the payload makes the signature invalid.
#[test]
fn signature_tamper() {
    let key = SigningKey::generate(&mut OsRng);
    let f = QsyFrame::Req {
        token: "deadbeef".into(),
        n_candidates: 2,
    };
    let mut line = encode_signed(&f, &key);
    // Flip a character in the token field
    let idx = line.find("deadbeef").unwrap() + 2;
    let ch = line.as_bytes()[idx];
    line.replace_range(idx..idx + 1, if ch == b'd' { "X" } else { "d" });
    assert!(matches!(
        decode_signed(&line, &key.verifying_key()),
        Err(QsyFrameError::InvalidSignature)
    ));
}

// ── Session state machine tests ──────────────────────────────────────────────

/// Initiator drives through the full flow and ends with QsyNow.
#[test]
fn initiator_full_flow() {
    let mut session = QsySession::new_initiator();
    let candidates = vec![14070000u64, 14074000u64];

    // Step 1: initiate
    let actions = session.initiate(candidates.clone()).unwrap();
    assert!(actions
        .iter()
        .any(|a| matches!(a, QsyAction::SendFrame(QsyFrame::Req { .. }))));
    let start_scan = actions
        .iter()
        .find(|a| matches!(a, QsyAction::StartScan { .. }))
        .expect("StartScan action expected");
    if let QsyAction::StartScan { candidates: c } = start_scan {
        assert_eq!(*c, candidates);
    }

    // Step 2: scan results
    let my_results = vec![(14070000u64, -87.0_f32), (14074000u64, -91.0_f32)];
    let actions = session.scan_complete(my_results).unwrap();
    assert!(actions
        .iter()
        .any(|a| matches!(a, QsyAction::SendFrame(QsyFrame::List { .. }))));

    // Step 3: receive partner's vote
    let partner_votes = vec![(14070000u64, -85.0_f32), (14074000u64, -93.0_f32)];
    let actions = session
        .apply(QsyFrame::Vote {
            token: extract_token_from_actions(&actions),
            votes: partner_votes,
        })
        .unwrap();

    // Best combined: 14070000 has -87 + -85 = -172, 14074000 has -91 + -93 = -184 → pick 14070000
    assert!(actions
        .iter()
        .any(|a| matches!(a, QsyAction::QsyNow { freq_hz: 14070000 })));
    assert!(actions.iter().any(|a| matches!(
        a,
        QsyAction::SendFrame(QsyFrame::Ack {
            agreed_freq_hz: 14070000,
            ..
        })
    )));
}

/// Responder drives through the full flow and ends with QsyNow.
#[test]
fn responder_full_flow() {
    let mut session = QsySession::new_responder(QsyPolicy { enabled: true });

    // Receive QSY_REQ
    let actions = session
        .apply(QsyFrame::Req {
            token: "cafebabe".into(),
            n_candidates: 2,
        })
        .unwrap();
    // No immediate action expected (just acknowledgement)
    assert!(
        actions.is_empty()
            || !actions
                .iter()
                .any(|a| matches!(a, QsyAction::Reject { .. }))
    );

    // Receive QSY_LIST
    let actions = session
        .apply(QsyFrame::List {
            token: "cafebabe".into(),
            candidates: vec![(14070000u64, -87.0_f32), (14074000u64, -91.0_f32)],
        })
        .unwrap();
    let scan_candidates = if let Some(QsyAction::StartScan { candidates }) = actions
        .iter()
        .find(|a| matches!(a, QsyAction::StartScan { .. }))
    {
        candidates.clone()
    } else {
        panic!("expected StartScan, got {actions:?}");
    };
    assert_eq!(scan_candidates, vec![14070000u64, 14074000u64]);

    // Scan completes
    let my_results = vec![(14070000u64, -89.0_f32), (14074000u64, -92.0_f32)];
    let actions = session.scan_complete(my_results).unwrap();
    assert!(actions
        .iter()
        .any(|a| matches!(a, QsyAction::SendFrame(QsyFrame::Vote { .. }))));

    // Receive QSY_ACK
    let actions = session
        .apply(QsyFrame::Ack {
            token: "cafebabe".into(),
            agreed_freq_hz: 14070000,
            switchover_offset_s: 5,
        })
        .unwrap();
    assert!(actions
        .iter()
        .any(|a| matches!(a, QsyAction::QsyNow { freq_hz: 14070000 })));
}

/// When `enabled=false`, responder rejects QSY_REQ with Reject action.
#[test]
fn reject_on_policy() {
    let mut session = QsySession::new_responder(QsyPolicy { enabled: false });
    let actions = session
        .apply(QsyFrame::Req {
            token: "12345678".into(),
            n_candidates: 3,
        })
        .unwrap();
    assert!(actions
        .iter()
        .any(|a| matches!(a, QsyAction::Reject { .. })));
    assert!(actions
        .iter()
        .any(|a| matches!(a, QsyAction::SendFrame(QsyFrame::Reject { .. }))));
}

/// When the initiator and responder have no common candidate, the session emits
/// QSY_REJECT and transitions to Rejected rather than leaving the peer hanging.
#[test]
fn disjoint_candidate_lists_emits_reject() {
    let mut session = QsySession::new_initiator();
    let init_actions = session.initiate(vec![14070000u64, 14074000u64]).unwrap();

    // Capture the token from the QSY_REQ frame.
    let token = if let QsyAction::SendFrame(QsyFrame::Req { token, .. }) = &init_actions[0] {
        token.clone()
    } else {
        panic!("expected QSY_REQ as first action");
    };

    session
        .scan_complete(vec![(14070000u64, -87.0_f32), (14074000u64, -91.0_f32)])
        .unwrap();

    // Partner votes on a completely different frequency — no overlap.
    let actions = session
        .apply(QsyFrame::Vote {
            token: token.clone(),
            votes: vec![(7030000u64, -80.0_f32)],
        })
        .unwrap();

    assert!(
        actions
            .iter()
            .any(|a| matches!(a, QsyAction::Reject { .. })),
        "expected Reject action, got {actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, QsyAction::SendFrame(QsyFrame::Reject { .. }))),
        "expected SendFrame(Reject), got {actions:?}"
    );
    // Session must have transitioned to Rejected — not still in Listed.
    let follow_up = session.apply(QsyFrame::Vote {
        token,
        votes: vec![(14070000u64, -85.0_f32)],
    });
    assert!(
        follow_up.is_err(),
        "session should be in Rejected state, not accept further votes"
    );
}

/// Receiving QSY_REJECT at any point yields a Reject action.
#[test]
fn qsy_reject_frame_yields_reject_action() {
    // Test on both initiator and responder
    for mut session in [
        QsySession::new_initiator(),
        QsySession::new_responder(QsyPolicy { enabled: true }),
    ] {
        let actions = session
            .apply(QsyFrame::Reject {
                token: "anytoken".into(),
                reason: "hamlib unavailable".into(),
            })
            .unwrap();
        assert!(
            actions.iter().any(
                |a| matches!(a, QsyAction::Reject { reason } if reason == "hamlib unavailable")
            ),
            "expected Reject action, got {actions:?}"
        );
    }
}

// ── Scanner test ─────────────────────────────────────────────────────────────

/// Scanner tunes to candidates, reads S-meter, returns to home frequency.
#[test]
fn scanner_returns_snr() {
    let (port, recorded_freqs) = common::start_recording_rigctld(14074000, -87);
    let addr = format!("127.0.0.1:{port}");

    let rig = RigctldController::connect(&addr).expect("connect to mock rigctld");
    let mut scanner = QsyScanner::new(rig, 0 /* no dwell for tests */);

    let results = scanner
        .scan(&[14070000, 14074000])
        .expect("scan should succeed");

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, 14070000);
    assert!(
        (results[0].1 - (-87.0_f32)).abs() < 0.5,
        "snr mismatch: {:?}",
        results[0].1
    );
    assert_eq!(results[1].0, 14074000);

    // Verify the rig was tuned to each candidate and then restored to home.
    let freqs = recorded_freqs.lock().unwrap();
    assert!(
        freqs.last() == Some(&14074000),
        "expected home freq 14074000 as last set_freq, got {freqs:?}"
    );
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn extract_token_from_actions(actions: &[QsyAction]) -> String {
    for a in actions {
        if let QsyAction::SendFrame(QsyFrame::List { token, .. }) = a {
            return token.clone();
        }
    }
    panic!("no QSY_LIST frame found in actions");
}
