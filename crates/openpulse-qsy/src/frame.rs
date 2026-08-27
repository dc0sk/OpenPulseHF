//! QSY wire frame codec and Ed25519 signing helpers.
//!
//! All frames are CR-terminated ASCII text lines. Signatures are appended as
//! `|SIG:<base64>` and cover the payload text that precedes the separator.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{SigningKey, VerifyingKey};
use openpulse_core::signing_domain::SigningDomain;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QsyFrameError {
    #[error("unknown or malformed QSY frame: {0}")]
    Malformed(String),
    /// A QSY line of a version this build does not speak.
    ///
    /// Distinct from [`Self::Malformed`] on purpose: a version mismatch means the peer is running a
    /// different build, and reporting it as "malformed" makes an attributable condition read as
    /// corruption.
    #[error("unsupported QSY version {got} (this build speaks {expected})")]
    UnsupportedVersion {
        /// The version the line carried.
        got: u8,
        /// The version this build emits and accepts.
        expected: u8,
    },
    #[error("invalid signature")]
    InvalidSignature,
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("signature bytes wrong length")]
    SignatureLength,
}

/// A QSY negotiation frame.
#[derive(Debug, Clone, PartialEq)]
pub enum QsyFrame {
    /// Initiate QSY scan: `QSY_REQ <token> <n_candidates>`.
    Req { token: String, n_candidates: u32 },
    /// Candidate list from initiator: `QSY_LIST <token> <freq_hz>,<snr_db> [...]`.
    List {
        token: String,
        candidates: Vec<(u64, f32)>,
    },
    /// Partner's SNR assessments: `QSY_VOTE <token> <freq_hz>,<snr_db> [...]`.
    Vote {
        token: String,
        votes: Vec<(u64, f32)>,
    },
    /// Agreed channel: `QSY_ACK <token> <agreed_freq_hz> <switchover_offset_s>`.
    Ack {
        token: String,
        agreed_freq_hz: u64,
        switchover_offset_s: u32,
    },
    /// Decline: `QSY_REJECT <token> <reason>`.
    Reject { token: String, reason: String },
}

/// The wire token every QSY line starts with: the signing domain's magic plus its version digit.
///
/// Derived from [`SigningDomain::QsyLine`] rather than written as a literal (#1162). A re-typed
/// `"OPQS1"` would stop agreeing with the registry the moment either half moved, and the registry's
/// source scan only sees `b"…"` byte-string literals — an ASCII string literal here would be
/// invisible to it. Sharing by reference removes the question.
fn wire_prefix() -> String {
    format!(
        "{}{}",
        std::str::from_utf8(SigningDomain::QsyLine.tag()).unwrap_or("OPQS"),
        SigningDomain::QsyLine.version()
    )
}

/// Encode a frame as an unsigned text line (no signature).
pub fn encode_unsigned(frame: &QsyFrame) -> String {
    match frame {
        QsyFrame::Req {
            token,
            n_candidates,
        } => {
            format!("{} QSY_REQ {token} {n_candidates}", wire_prefix())
        }
        QsyFrame::List { token, candidates } => {
            let pairs: Vec<String> = candidates
                .iter()
                .map(|(f, s)| format!("{f},{s:.2}"))
                .collect();
            format!("{} QSY_LIST {token} {}", wire_prefix(), pairs.join(" "))
        }
        QsyFrame::Vote { token, votes } => {
            let pairs: Vec<String> = votes.iter().map(|(f, s)| format!("{f},{s:.2}")).collect();
            format!("{} QSY_VOTE {token} {}", wire_prefix(), pairs.join(" "))
        }
        QsyFrame::Ack {
            token,
            agreed_freq_hz,
            switchover_offset_s,
        } => {
            format!(
                "{} QSY_ACK {token} {agreed_freq_hz} {switchover_offset_s}",
                wire_prefix()
            )
        }
        QsyFrame::Reject { token, reason } => {
            format!("{} QSY_REJECT {token} {reason}", wire_prefix())
        }
    }
}

/// Decode a frame from an unsigned text line.
pub fn decode_unsigned(line: &str) -> Result<QsyFrame, QsyFrameError> {
    let line = line.trim_end_matches('\r').trim_end_matches('\n');

    // The wire token is consumed FIRST and its version is authoritative (#1162). Before this the
    // format had neither a magic nor a version, so a receiver had nothing to branch on and an
    // unknown line was indistinguishable from corruption. An unsupported version is reported as
    // itself, never as `Malformed`: an epoch mismatch that reads as garbage is the unattributable
    // symptom every other format in this tree now avoids (frame.rs, handshake_wire.rs, and
    // WireEnvelope since #1164).
    let (prefix, rest) = line
        .split_once(' ')
        .ok_or_else(|| QsyFrameError::Malformed("empty line".into()))?;
    let magic = std::str::from_utf8(SigningDomain::QsyLine.tag()).unwrap_or("OPQS");
    let digits = prefix
        .strip_prefix(magic)
        .ok_or_else(|| QsyFrameError::Malformed("missing QSY wire token".into()))?;
    let got: u8 = digits
        .parse()
        .map_err(|_| QsyFrameError::Malformed(format!("unparsable version {digits:?}")))?;
    if got != SigningDomain::QsyLine.version() {
        return Err(QsyFrameError::UnsupportedVersion {
            got,
            expected: SigningDomain::QsyLine.version(),
        });
    }

    let mut parts = rest.splitn(3, ' ');
    let verb = parts
        .next()
        .ok_or_else(|| QsyFrameError::Malformed("empty line".into()))?;
    let token = parts
        .next()
        .ok_or_else(|| QsyFrameError::Malformed("missing token".into()))?
        .to_string();
    if token.len() > 64 {
        return Err(QsyFrameError::Malformed(format!(
            "token too long: {} bytes (max 64)",
            token.len()
        )));
    }
    let rest = parts.next().unwrap_or("").trim();

    match verb {
        "QSY_REQ" => {
            let n: u32 = rest
                .parse()
                .map_err(|_| QsyFrameError::Malformed(format!("bad n_candidates: {rest}")))?;
            Ok(QsyFrame::Req {
                token,
                n_candidates: n,
            })
        }
        "QSY_LIST" => Ok(QsyFrame::List {
            token,
            candidates: parse_pairs(rest)?,
        }),
        "QSY_VOTE" => Ok(QsyFrame::Vote {
            token,
            votes: parse_pairs(rest)?,
        }),
        "QSY_ACK" => {
            let mut it = rest.splitn(2, ' ');
            let freq: u64 = it
                .next()
                .unwrap_or("")
                .parse()
                .map_err(|_| QsyFrameError::Malformed(format!("bad freq: {rest}")))?;
            let offset: u32 = it
                .next()
                .ok_or_else(|| QsyFrameError::Malformed("missing switchover offset".into()))?
                .parse()
                .map_err(|_| QsyFrameError::Malformed(format!("bad offset: {rest}")))?;
            Ok(QsyFrame::Ack {
                token,
                agreed_freq_hz: freq,
                switchover_offset_s: offset,
            })
        }
        "QSY_REJECT" => Ok(QsyFrame::Reject {
            token,
            reason: rest.to_string(),
        }),
        other => Err(QsyFrameError::Malformed(format!("unknown verb: {other}"))),
    }
}

fn parse_pairs(s: &str) -> Result<Vec<(u64, f32)>, QsyFrameError> {
    let mut out = Vec::new();
    for token in s.split_whitespace() {
        let mut it = token.splitn(2, ',');
        let freq: u64 = it
            .next()
            .unwrap_or("")
            .parse()
            .map_err(|_| QsyFrameError::Malformed(format!("bad freq in pair: {token}")))?;
        let snr_str = it
            .next()
            .ok_or_else(|| QsyFrameError::Malformed(format!("missing snr in pair: {token}")))?;
        let snr: f32 = snr_str
            .parse()
            .map_err(|_| QsyFrameError::Malformed(format!("bad snr in pair: {token}")))?;
        out.push((freq, snr));
    }
    Ok(out)
}

/// Append an Ed25519 signature to a text line: `<line>|SIG:<base64>`.
pub fn sign_line(line: &str, key: &SigningKey) -> String {
    let sig = openpulse_core::signing::sign_in_band(
        SigningDomain::QsyLine,
        &key.to_bytes(),
        line.as_bytes(),
    )
    .unwrap_or([0u8; 64]);
    format!("{line}|SIG:{}", STANDARD.encode(sig))
}

/// Verify the `|SIG:` suffix and return the payload (before the separator).
pub fn verify_line<'a>(line: &'a str, key: &VerifyingKey) -> Result<&'a str, QsyFrameError> {
    let line = line.trim_end_matches(['\r', '\n']);
    let (payload, sig_b64) = line
        .rsplit_once("|SIG:")
        .ok_or_else(|| QsyFrameError::Malformed("missing |SIG: field".into()))?;
    let sig_bytes = STANDARD.decode(sig_b64)?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| QsyFrameError::SignatureLength)?;
    if !openpulse_core::signing::verify_in_band(
        SigningDomain::QsyLine,
        &key.to_bytes(),
        payload.as_bytes(),
        &sig_array,
    ) {
        return Err(QsyFrameError::InvalidSignature);
    }
    Ok(payload)
}

/// Encode a frame as a signed text line.
pub fn encode_signed(frame: &QsyFrame, key: &SigningKey) -> String {
    sign_line(&encode_unsigned(frame), key)
}

/// Decode a signed text line, verifying the signature.
pub fn decode_signed(line: &str, key: &VerifyingKey) -> Result<QsyFrame, QsyFrameError> {
    let payload = verify_line(line, key)?;
    decode_unsigned(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1162: every encoded line starts with the wire token, and the token is DERIVED from the
    /// signing registry rather than typed here — so a change to either half fails this test rather
    /// than silently splitting the two representations.
    #[test]
    fn every_encoded_line_carries_the_wire_token() {
        let expected = format!(
            "{}{}",
            std::str::from_utf8(SigningDomain::QsyLine.tag()).unwrap(),
            SigningDomain::QsyLine.version()
        );
        let frames = [
            QsyFrame::Req {
                token: "t".into(),
                n_candidates: 1,
            },
            QsyFrame::List {
                token: "t".into(),
                candidates: vec![(14_100_000, 9.5)],
            },
            QsyFrame::Vote {
                token: "t".into(),
                votes: vec![(14_100_000, 9.5)],
            },
            QsyFrame::Ack {
                token: "t".into(),
                agreed_freq_hz: 14_100_000,
                switchover_offset_s: 5,
            },
            QsyFrame::Reject {
                token: "t".into(),
                reason: "busy".into(),
            },
        ];
        for f in &frames {
            let line = encode_unsigned(f);
            assert!(
                line.starts_with(&expected),
                "{line:?} does not start with the wire token {expected:?}"
            );
            // and it must still round-trip through its own decoder
            assert_eq!(&decode_unsigned(&line).expect("round trip"), f);
        }
    }

    /// The version is AUTHORITATIVE and reports itself — a mismatch is not `Malformed`.
    ///
    /// The pre-#1162 format had neither a magic nor a version, so a receiver had nothing to branch
    /// on and every unknown line was indistinguishable from corruption. Reporting an epoch mismatch
    /// as "malformed" would preserve exactly that, which is the unattributable symptom every other
    /// format in this tree now avoids.
    #[test]
    fn a_line_of_another_version_is_refused_by_version() {
        let magic = std::str::from_utf8(SigningDomain::QsyLine.tag()).unwrap();
        let current = SigningDomain::QsyLine.version();

        // Positive control: the current version decodes, or the assertions below pass vacuously.
        let good = encode_unsigned(&QsyFrame::Req {
            token: "t".into(),
            n_candidates: 1,
        });
        assert!(
            decode_unsigned(&good).is_ok(),
            "control: current version must decode"
        );

        for other in [current.wrapping_add(1), current.wrapping_add(9), 0] {
            let line = format!("{magic}{other} QSY_REQ t 1");
            match decode_unsigned(&line) {
                Err(QsyFrameError::UnsupportedVersion { got, .. }) => assert_eq!(got, other),
                res => panic!("version {other} must be refused by VERSION, got {res:?}"),
            }
        }
    }

    /// A line with no wire token at all — the pre-#1162 format — is refused, not silently accepted.
    #[test]
    fn a_line_without_the_wire_token_is_refused() {
        assert!(matches!(
            decode_unsigned("QSY_REQ t 1"),
            Err(QsyFrameError::Malformed(_))
        ));
    }
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn test_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    #[test]
    fn req_round_trip() {
        let f = QsyFrame::Req {
            token: "abc123ef".into(),
            n_candidates: 4,
        };
        assert_eq!(decode_unsigned(&encode_unsigned(&f)).unwrap(), f);
    }

    #[test]
    fn list_round_trip() {
        let f = QsyFrame::List {
            token: "tok00001".into(),
            candidates: vec![(14074000, -87.5), (14070000, -91.0)],
        };
        assert_eq!(decode_unsigned(&encode_unsigned(&f)).unwrap(), f);
    }

    #[test]
    fn ack_round_trip() {
        let f = QsyFrame::Ack {
            token: "tok00002".into(),
            agreed_freq_hz: 14074000,
            switchover_offset_s: 5,
        };
        assert_eq!(decode_unsigned(&encode_unsigned(&f)).unwrap(), f);
    }

    #[test]
    fn reject_round_trip() {
        let f = QsyFrame::Reject {
            token: "tok00003".into(),
            reason: "qsy disabled".into(),
        };
        assert_eq!(decode_unsigned(&encode_unsigned(&f)).unwrap(), f);
    }

    #[test]
    fn signed_round_trip() {
        let key = test_key();
        let f = QsyFrame::Req {
            token: "deadbeef".into(),
            n_candidates: 2,
        };
        let line = encode_signed(&f, &key);
        let decoded = decode_signed(&line, &key.verifying_key()).unwrap();
        assert_eq!(decoded, f);
    }

    #[test]
    fn tampered_signature_rejected() {
        let key = test_key();
        let f = QsyFrame::Req {
            token: "deadbeef".into(),
            n_candidates: 2,
        };
        let mut line = encode_signed(&f, &key);
        // Flip one character in the payload
        let idx = line.find("QSY_REQ").unwrap() + 4;
        let ch = line.as_bytes()[idx];
        line.replace_range(idx..idx + 1, if ch == b'_' { "X" } else { "_" });
        assert!(matches!(
            decode_signed(&line, &key.verifying_key()),
            Err(QsyFrameError::InvalidSignature)
        ));
    }

    #[test]
    fn token_too_long_rejected() {
        // The line must carry the wire token, or this rejects for the WRONG REASON. Hand-built as
        // `"QSY_REQ …"` it still returned `Malformed` after #1162 — but from the missing-token
        // check, never reaching the length cap this test is named for. `Malformed(_)` matches both,
        // so the test passed while testing nothing. Assert on the MESSAGE to pin the mechanism.
        let long_token = "a".repeat(65);
        let line = encode_unsigned(&QsyFrame::Req {
            token: long_token,
            n_candidates: 2,
        });
        match decode_unsigned(&line) {
            Err(QsyFrameError::Malformed(m)) => {
                assert!(
                    m.contains("token too long"),
                    "rejected for the wrong reason: {m}"
                )
            }
            other => panic!("an over-long token must be refused, got {other:?}"),
        }
    }

    #[test]
    fn token_max_length_accepted() {
        // Built through the ENCODER: hand-typing the line pinned the pre-#1162 format and broke
        // when the wire token was added.
        let token = "a".repeat(64);
        let line = encode_unsigned(&QsyFrame::Req {
            token,
            n_candidates: 2,
        });
        assert!(decode_unsigned(&line).is_ok());
    }
}
