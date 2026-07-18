use openpulse_b2f::{
    banner,
    compress::{
        compress_gzip, compress_lzhuf, compress_lzhuf_winlink, decompress_gzip, decompress_lzhuf,
        decompress_lzhuf_compat, decompress_lzhuf_winlink,
    },
    frame::{self, B2fFrame, FsAnswer, ProposalType},
    header::{self, AttachmentInfo, WlHeader},
    B2fSession, SessionRole,
};

// ── Banner ────────────────────────────────────────────────────────────────────

#[test]
fn banner_roundtrip() {
    let encoded = banner::encode("W1AW");
    let decoded = banner::decode(&encoded).unwrap();
    assert_eq!(decoded.version, "3.0-B2FWINMOR-4.0");
    assert!(!decoded.session_key.is_empty());
}

// ── Frame codec ───────────────────────────────────────────────────────────────

#[test]
fn fc_frame_roundtrip() {
    let f = B2fFrame::Fc {
        proposal_type: ProposalType::D,
        mid: "ABC1234567890".into(),
        size: 1024,
        date: "20260504120000".into(),
    };
    let line = frame::encode(&f);
    let decoded = frame::decode(&line).unwrap();
    assert_eq!(decoded, f);
}

#[test]
fn fs_frame_roundtrip() {
    let f = B2fFrame::Fs {
        answers: vec![FsAnswer::Accept, FsAnswer::Reject, FsAnswer::Defer],
    };
    let line = frame::encode(&f);
    let decoded = frame::decode(&line).unwrap();
    assert_eq!(decoded, f);
}

#[test]
fn ff_fq_roundtrip() {
    assert_eq!(
        frame::decode(&frame::encode(&B2fFrame::Ff)).unwrap(),
        B2fFrame::Ff
    );
    assert_eq!(
        frame::decode(&frame::encode(&B2fFrame::Fq)).unwrap(),
        B2fFrame::Fq
    );
}

// ── Header ────────────────────────────────────────────────────────────────────

#[test]
fn header_roundtrip() {
    let h = WlHeader {
        mid: "OPNPLS001".into(),
        date: "2026/05/04 12:00".into(),
        from: "W1AW@winlink.org".into(),
        to: vec!["W2AW@winlink.org".into()],
        subject: "Test message".into(),
        size: 256,
        body: 64,
        attachments: vec![AttachmentInfo {
            name: "file.txt".into(),
            size: 128,
        }],
    };
    let encoded = header::encode(&h);
    let decoded = header::decode(&encoded).unwrap();
    assert_eq!(decoded.mid, h.mid);
    assert_eq!(decoded.from, h.from);
    assert_eq!(decoded.to, h.to);
    assert_eq!(decoded.subject, h.subject);
    assert_eq!(decoded.body, h.body);
    assert_eq!(decoded.attachments.len(), 1);
    assert_eq!(decoded.attachments[0].name, "file.txt");
}

// ── Compression ───────────────────────────────────────────────────────────────

#[test]
fn gzip_compress_decompress() {
    let data = b"The quick brown fox jumps over the lazy dog".repeat(10);
    let compressed = compress_gzip(&data).unwrap();
    assert!(
        compressed.len() < data.len(),
        "gzip should compress repetitive data"
    );
    let decompressed = decompress_gzip(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn lzhuf_round_trip() {
    let data: Vec<u8> = b"The quick brown fox jumps over the lazy dog. ".repeat(10);
    let compressed = compress_lzhuf(&data).unwrap();
    assert!(
        compressed.len() < data.len() + 4,
        "LZHUF should compress repetitive data"
    );
    let decompressed = decompress_lzhuf(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn lzhuf_bad_input_error() {
    assert!(
        decompress_lzhuf(b"bad").is_err(),
        "truncated input must error"
    );
}

#[test]
fn lzhuf_winlink_round_trip() {
    let data: Vec<u8> = b"The quick brown fox jumps over the lazy dog. ".repeat(10);
    let compressed = compress_lzhuf_winlink(&data).unwrap();
    assert!(
        compressed.len() >= 4,
        "compressed output should be non-trivial"
    );
    let decompressed = decompress_lzhuf_winlink(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn lzhuf_winlink_format_differs_from_internal() {
    // Use a payload large enough that the length prefix bytes differ between BE and LE.
    let data = b"Hello from Winlink! ".repeat(15); // 300 bytes > 255
    let internal = compress_lzhuf(&data).unwrap();
    let winlink = compress_lzhuf_winlink(&data).unwrap();
    // Both have 4-byte length prefixes, but BE vs LE differ for lengths > 255.
    assert_ne!(
        &internal[..4],
        &winlink[..4],
        "BE and LE prefixes should differ for payload length > 255"
    );
    // Both must decompress to the original payload.
    assert_eq!(decompress_lzhuf(&internal).unwrap(), data.to_vec());
    assert_eq!(decompress_lzhuf_winlink(&winlink).unwrap(), data.to_vec());
}

#[test]
fn lzhuf_compat_accepts_both_length_prefixes() {
    let data = b"Winlink Type C compatibility payload".repeat(12);
    let internal = compress_lzhuf(&data).unwrap();
    let winlink = compress_lzhuf_winlink(&data).unwrap();

    assert_eq!(decompress_lzhuf_compat(&internal).unwrap(), data.to_vec());
    assert_eq!(decompress_lzhuf_compat(&winlink).unwrap(), data.to_vec());
}

// ── Session state machine ─────────────────────────────────────────────────────

#[test]
fn session_iss_irs_exchange() {
    let body = b"Hello Winlink!".to_vec();

    // Build ISS session with one queued message.
    let mut iss = B2fSession::new(SessionRole::Iss);
    iss.queue_message(
        WlHeader {
            mid: "MSG001".into(),
            date: "2026/05/04 12:00".into(),
            from: "W1AW@winlink.org".into(),
            to: vec!["W2AW@winlink.org".into()],
            subject: "Hello".into(),
            size: body.len() as u32,
            body: body.len() as u32,
            attachments: vec![],
        },
        body.clone(),
    )
    .unwrap();

    // Build IRS session.
    let mut irs = B2fSession::new(SessionRole::Irs);

    // Handshake: IRS sends banner → ISS responds with FCs + FF.
    let irs_banner = banner::encode("W2AW");
    let iss_out = iss.handle_line(&irs_banner).unwrap();
    // ISS should emit 1 FC + 1 FF.
    assert_eq!(iss_out.len(), 2);
    assert!(iss_out[0].starts_with("FC"));
    assert!(iss_out[1].trim_end_matches('\r') == "FF");

    // IRS receives FC.
    let irs_fc_out = irs.handle_line(&iss_out[0]).unwrap();
    assert!(irs_fc_out.is_empty());

    // IRS receives FF → sends FS + accept.
    let irs_ff_out = irs.handle_line(&iss_out[1]).unwrap();
    assert_eq!(irs_ff_out.len(), 1);
    assert!(irs_ff_out[0].starts_with("FS"));

    // ISS receives FS → stages data.
    iss.handle_line(&irs_ff_out[0]).unwrap();
    let data_chunks = iss.drain_pending_data();
    assert_eq!(data_chunks.len(), 1);
    assert!(iss.is_done(), "ISS should be Done after draining all data");

    // IRS decodes the compressed chunk; result includes header block + body.
    let decompressed = irs.receive_data(data_chunks[0].clone()).unwrap();
    let sep = decompressed
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap();
    assert_eq!(&decompressed[sep + 4..], body.as_slice());
    assert!(irs.is_done(), "IRS should be Done after receiving all data");
}

#[test]
fn session_irs_rejects() {
    let mut iss = B2fSession::new(SessionRole::Iss);
    iss.queue_message(
        WlHeader {
            mid: "MSG002".into(),
            date: "2026/05/04 12:00".into(),
            from: "W1AW@winlink.org".into(),
            to: vec!["W2AW@winlink.org".into()],
            subject: "Rejected".into(),
            size: 10,
            body: 10,
            attachments: vec![],
        },
        b"reject me!".to_vec(),
    )
    .unwrap();

    let mut irs = B2fSession::new(SessionRole::Irs);

    let iss_out = iss.handle_line(&banner::encode("W2AW")).unwrap();
    irs.handle_line(&iss_out[0]).unwrap(); // FC
    let irs_out = irs.handle_line(&iss_out[1]).unwrap(); // FF → FS

    // Manually override IRS FS to reject.
    let reject_fs = frame::encode(&B2fFrame::Fs {
        answers: vec![FsAnswer::Reject],
    });
    iss.handle_line(&reject_fs).unwrap();

    // No data should be staged; ISS reaches Done immediately (nothing to transfer).
    assert!(iss.drain_pending_data().is_empty());
    assert!(
        iss.is_done(),
        "ISS should be Done after all proposals rejected"
    );
    assert!(irs_out[0].starts_with("FS"));
}

#[test]
fn session_receive_data_type_c_accepts_internal_and_winlink_prefixes() {
    let mut irs = B2fSession::new(SessionRole::Irs);

    // Drive IRS into Transfer state with one accepted Type C proposal.
    let fc = frame::encode(&B2fFrame::Fc {
        proposal_type: ProposalType::C,
        mid: "MSG003".into(),
        size: 64,
        date: "20260504120000".into(),
    });
    irs.handle_line(&fc).unwrap();
    let fs = irs.handle_line(&frame::encode(&B2fFrame::Ff)).unwrap();
    assert_eq!(fs.len(), 1);
    assert!(fs[0].starts_with("FS "));

    let payload = b"compat body".repeat(8);
    let winlink = compress_lzhuf_winlink(&payload).unwrap();
    let internal = compress_lzhuf(&payload).unwrap();

    // First accepted payload uses Winlink LE format.
    let out1 = irs.receive_data(winlink).unwrap();
    assert_eq!(out1, payload);

    // Rebuild IRS for BE-format fallback path.
    let mut irs2 = B2fSession::new(SessionRole::Irs);
    irs2.handle_line(&fc).unwrap();
    irs2.handle_line(&frame::encode(&B2fFrame::Ff)).unwrap();
    let out2 = irs2.receive_data(internal).unwrap();
    assert_eq!(out2, payload);
}

#[test]
fn session_iss_type_c_irs_roundtrip() {
    let body = b"Winlink Type C round-trip body".to_vec();

    // ISS proposes with Type C (LZHUF Winlink-compatible).
    let mut iss = B2fSession::new(SessionRole::Iss);
    iss.queue_message_type_c(
        WlHeader {
            mid: "MSG_TC01".into(),
            date: "2026/05/19 10:00".into(),
            from: "W1AW@winlink.org".into(),
            to: vec!["W2AW@winlink.org".into()],
            subject: "Type C test".into(),
            size: body.len() as u32,
            body: body.len() as u32,
            attachments: vec![],
        },
        body.clone(),
    )
    .unwrap();

    let mut irs = B2fSession::new(SessionRole::Irs);

    // Handshake: IRS banner → ISS emits FC (type=C) + FF.
    let iss_out = iss.handle_line(&banner::encode("W2AW")).unwrap();
    assert_eq!(iss_out.len(), 2);
    assert!(iss_out[0].starts_with("FC C"), "FC proposal must be Type C");
    assert!(iss_out[1].trim_end_matches('\r') == "FF");

    // IRS receives FC + FF → sends FS accept.
    irs.handle_line(&iss_out[0]).unwrap();
    let irs_out = irs.handle_line(&iss_out[1]).unwrap();
    assert_eq!(irs_out.len(), 1);
    assert!(irs_out[0].starts_with("FS"));

    // ISS receives FS → stages LZHUF-compressed data.
    iss.handle_line(&irs_out[0]).unwrap();
    let chunks = iss.drain_pending_data();
    assert_eq!(chunks.len(), 1);
    assert!(iss.is_done());

    // IRS decompresses via decompress_lzhuf_compat (accepts Winlink LE prefix).
    let decompressed = irs.receive_data(chunks[0].clone()).unwrap();
    let sep = decompressed
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap();
    assert_eq!(&decompressed[sep + 4..], body.as_slice());
    assert!(irs.is_done());
}

#[test]
fn gzip_decompression_bomb_over_the_cap_is_rejected() {
    // Audit B-1: a tiny gzip stream that expands past the 16 MiB cap must be rejected, not allocated.
    let bomb = compress_gzip(&vec![0u8; 16 * 1024 * 1024 + 1]).unwrap();
    assert!(
        bomb.len() < 65_535,
        "a run of zeros compresses to a tiny blob"
    );
    assert!(
        decompress_gzip(&bomb).is_err(),
        "a gzip stream expanding past the cap must be rejected"
    );
    // A modest payload still round-trips.
    let ok = b"hello".repeat(100);
    assert_eq!(decompress_gzip(&compress_gzip(&ok).unwrap()).unwrap(), ok);
}

#[test]
fn fc_flood_bounds_retained_proposals_and_still_answers_all() {
    // Audit follow-up: the B-2 cap gated the Accept/Reject *answer* but pushed every proposal
    // unconditionally, so the `proposals` Vec (holding attacker-sized mid/date strings) grew without
    // bound. Retention must be bounded at MAX_PROPOSALS while the Ff answer still replies once per
    // proposal the peer sent (so a legit >32 batch is not desynced).
    let mut irs = B2fSession::new(SessionRole::Irs);
    const SENT: usize = 100;
    for i in 0..SENT as u32 {
        let fc = frame::encode(&B2fFrame::Fc {
            proposal_type: ProposalType::D,
            mid: format!("MSG{i:05}"),
            size: 64,
            date: "20260504120000".into(),
        });
        irs.handle_line(&fc).unwrap();
    }
    assert!(
        irs.retained_proposals() <= 32,
        "retained proposals must stay bounded (got {}); an unbounded Vec is the OOM DoS",
        irs.retained_proposals()
    );
    let fs = irs.handle_line(&frame::encode(&B2fFrame::Ff)).unwrap();
    assert_eq!(fs.len(), 1);
    // One answer per proposal the peer sent: 32 Accept + 68 Reject.
    let answers = fs[0].trim_start_matches("FS ").trim();
    assert_eq!(
        answers.chars().count(),
        SENT,
        "the FS answer must have one char per proposal sent, not just the retained ones"
    );
    assert_eq!(irs.accepted_count(), 32);
}

#[test]
fn an_unterminated_frame_stream_aborts_instead_of_hanging() {
    // A hostile/buggy peer that streams valid FC frames and never sends FF keeps the driver/gateway
    // receive loops (which break only on a non-empty response or is_done()) spinning forever. The
    // session must abort past a generous frame ceiling so the loop's `?` terminates it.
    let mut irs = B2fSession::new(SessionRole::Irs);
    let fc = frame::encode(&B2fFrame::Fc {
        proposal_type: ProposalType::D,
        mid: "M".into(),
        size: 1,
        date: "20260504120000".into(),
    });
    let mut aborted = false;
    for _ in 0..20_000 {
        if irs.handle_line(&fc).is_err() {
            aborted = true;
            break;
        }
    }
    assert!(
        aborted,
        "an endless FC stream must eventually abort the session, not loop forever"
    );
}

#[test]
fn irs_caps_the_number_of_accepted_proposals() {
    // Audit B-2: a hostile peer offering many proposals must not make us accept (and later receive,
    // decompress, and retain) an unbounded number in one session — accepts are capped at 32.
    let mut irs = B2fSession::new(SessionRole::Irs);
    for i in 0..40u32 {
        let fc = frame::encode(&B2fFrame::Fc {
            proposal_type: ProposalType::D,
            mid: format!("MSG{i:05}"),
            size: 64,
            date: "20260504120000".into(),
        });
        irs.handle_line(&fc).unwrap();
    }
    let fs = irs.handle_line(&frame::encode(&B2fFrame::Ff)).unwrap();
    assert_eq!(fs.len(), 1);
    assert!(fs[0].starts_with("FS "));
    assert_eq!(
        irs.accepted_count(),
        32,
        "no more than MAX_PROPOSALS may be accepted regardless of how many were offered"
    );
}

// ── Session-aggregate decompression cap ───────────────────────────────────────

/// Drive an IRS session into Transfer with `n` accepted Type D proposals.
fn irs_in_transfer(n: usize) -> B2fSession {
    let mut irs = B2fSession::new(SessionRole::Irs);
    for i in 0..n {
        let fc = frame::encode(&B2fFrame::Fc {
            proposal_type: ProposalType::D,
            mid: format!("MSG{i:04}"),
            size: 64,
            date: "20260718120000".into(),
        });
        irs.handle_line(&fc).unwrap();
    }
    let fs = irs.handle_line(&frame::encode(&B2fFrame::Ff)).unwrap();
    assert_eq!(fs.len(), 1, "IRS should answer FF with one FS");
    irs
}

/// A hostile peer can stay under the 16 MiB per-message cap on every message and still make us
/// decompress hundreds of MB across one session. The session must bound the AGGREGATE, not just
/// each message (audit 2026-07-17, medium tier).
#[test]
fn session_bounds_aggregate_decompressed_bytes() {
    let mut irs = irs_in_transfer(16);
    // ~4 MiB of zeros gzips to a few KB — tiny on the wire, large in RAM.
    let blob = compress_gzip(&vec![0u8; 4 * 1024 * 1024]).unwrap();
    let mut delivered = 0usize;
    let mut hit_cap = false;
    for _ in 0..16 {
        match irs.receive_data(blob.clone()) {
            Ok(_) => delivered += 1,
            Err(e) => {
                assert!(
                    matches!(e, openpulse_b2f::B2fError::SessionTooLarge { .. }),
                    "expected the aggregate cap to fire, got {e:?}"
                );
                hit_cap = true;
                break;
            }
        }
    }
    assert!(
        hit_cap,
        "session accepted {delivered} × 4 MiB decompressed with no aggregate cap"
    );
}

/// The cap must not touch a realistic mailbox batch: many small messages stay well under it.
#[test]
fn session_aggregate_cap_does_not_trip_on_a_normal_batch() {
    let mut irs = irs_in_transfer(24);
    let blob = compress_gzip(&vec![b'x'; 64 * 1024]).unwrap();
    for i in 0..24 {
        irs.receive_data(blob.clone())
            .unwrap_or_else(|e| panic!("normal 64 KiB message {i} rejected: {e:?}"));
    }
}
