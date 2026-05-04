//! B2F control frame encode/decode (FC, FS, FF, FQ).

use crate::B2fError;

/// Compression type for a file-check proposal.
#[derive(Debug, Clone, PartialEq)]
pub enum ProposalType {
    /// LZHUF B2 compression (type C).
    C,
    /// Gzip compression (type D).
    D,
}

/// File-select answer per proposed message.
#[derive(Debug, Clone, PartialEq)]
pub enum FsAnswer {
    Accept,
    Reject,
    Defer,
}

/// A B2F control frame.
#[derive(Debug, Clone, PartialEq)]
pub enum B2fFrame {
    /// `FC <type> <mid> <size> <date>` — sender proposes a message.
    Fc {
        proposal_type: ProposalType,
        mid: String,
        size: u32,
        date: String,
    },
    /// `FS <answers>` — receiver selects which proposed messages to accept.
    Fs { answers: Vec<FsAnswer> },
    /// `FF` — all proposals sent; transfer complete.
    Ff,
    /// `FQ` — quit session.
    Fq,
}

/// Encode a frame to a CR-terminated ASCII line.
pub fn encode(frame: &B2fFrame) -> String {
    match frame {
        B2fFrame::Fc {
            proposal_type,
            mid,
            size,
            date,
        } => {
            let t = match proposal_type {
                ProposalType::C => 'C',
                ProposalType::D => 'D',
            };
            format!("FC {t} {mid} {size} {date}\r")
        }
        B2fFrame::Fs { answers } => {
            let s: String = answers
                .iter()
                .map(|a| match a {
                    FsAnswer::Accept => '+',
                    FsAnswer::Reject => '-',
                    FsAnswer::Defer => '=',
                })
                .collect();
            format!("FS {s}\r")
        }
        B2fFrame::Ff => "FF\r".to_string(),
        B2fFrame::Fq => "FQ\r".to_string(),
    }
}

/// Decode a CR-terminated (or plain) ASCII line into a frame.
pub fn decode(line: &str) -> Result<B2fFrame, B2fError> {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    let parts: Vec<&str> = trimmed.splitn(5, ' ').collect();
    match parts[0].to_uppercase().as_str() {
        "FC" if parts.len() >= 5 => {
            let proposal_type = match parts[1] {
                "C" | "c" => ProposalType::C,
                "D" | "d" => ProposalType::D,
                t => return Err(B2fError::InvalidFrame(format!("unknown type: {t}"))),
            };
            let size = parts[3]
                .parse()
                .map_err(|_| B2fError::InvalidFrame("bad size".into()))?;
            Ok(B2fFrame::Fc {
                proposal_type,
                mid: parts[2].to_string(),
                size,
                date: parts[4].to_string(),
            })
        }
        "FS" if parts.len() >= 2 => {
            let answers = parts[1]
                .chars()
                .map(|c| match c {
                    '+' => Ok(FsAnswer::Accept),
                    '-' => Ok(FsAnswer::Reject),
                    '=' => Ok(FsAnswer::Defer),
                    other => Err(B2fError::InvalidFrame(format!("bad answer char: {other}"))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(B2fFrame::Fs { answers })
        }
        "FF" => Ok(B2fFrame::Ff),
        "FQ" => Ok(B2fFrame::Fq),
        _ => Err(B2fError::InvalidFrame(trimmed.to_string())),
    }
}
