//! QSY wire frame codec and Ed25519 signing helpers.
//!
//! All frames are CR-terminated ASCII text lines. Signatures are appended as
//! `|SIG:<base64>` and cover the payload text that precedes the separator.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QsyFrameError {
    #[error("unknown or malformed QSY frame: {0}")]
    Malformed(String),
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

/// Encode a frame as an unsigned text line (no signature).
pub fn encode_unsigned(frame: &QsyFrame) -> String {
    match frame {
        QsyFrame::Req {
            token,
            n_candidates,
        } => {
            format!("QSY_REQ {token} {n_candidates}")
        }
        QsyFrame::List { token, candidates } => {
            let pairs: Vec<String> = candidates
                .iter()
                .map(|(f, s)| format!("{f},{s:.2}"))
                .collect();
            format!("QSY_LIST {token} {}", pairs.join(" "))
        }
        QsyFrame::Vote { token, votes } => {
            let pairs: Vec<String> = votes.iter().map(|(f, s)| format!("{f},{s:.2}")).collect();
            format!("QSY_VOTE {token} {}", pairs.join(" "))
        }
        QsyFrame::Ack {
            token,
            agreed_freq_hz,
            switchover_offset_s,
        } => {
            format!("QSY_ACK {token} {agreed_freq_hz} {switchover_offset_s}")
        }
        QsyFrame::Reject { token, reason } => {
            format!("QSY_REJECT {token} {reason}")
        }
    }
}

/// Decode a frame from an unsigned text line.
pub fn decode_unsigned(line: &str) -> Result<QsyFrame, QsyFrameError> {
    let line = line.trim_end_matches('\r').trim_end_matches('\n');
    let mut parts = line.splitn(3, ' ');
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
    let sig: Signature = key.sign(line.as_bytes());
    format!("{line}|SIG:{}", STANDARD.encode(sig.to_bytes()))
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
    let sig = Signature::from_bytes(&sig_array);
    key.verify(payload.as_bytes(), &sig)
        .map_err(|_| QsyFrameError::InvalidSignature)?;
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
        let long_token = "a".repeat(65);
        let line = format!("QSY_REQ {long_token} 2");
        assert!(matches!(
            decode_unsigned(&line),
            Err(QsyFrameError::Malformed(_))
        ));
    }

    #[test]
    fn token_max_length_accepted() {
        let token = "a".repeat(64);
        let line = format!("QSY_REQ {token} 2");
        assert!(decode_unsigned(&line).is_ok());
    }
}
