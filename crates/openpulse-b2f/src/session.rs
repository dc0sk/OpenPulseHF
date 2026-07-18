//! B2F session state machine for ISS (sending) and IRS (receiving) roles.

use crate::compress::{compress_gzip, decompress_gzip};
use crate::frame::{self, B2fFrame, FsAnswer, ProposalType};
use crate::header::WlHeader;
use crate::B2fError;

/// Whether this node is the sender or receiver for this session.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionRole {
    /// Information Sending Station — proposes outbound messages.
    Iss,
    /// Information Receiving Station — selects which proposals to accept.
    Irs,
}

#[derive(Debug, Clone, PartialEq)]
enum SessionState {
    Handshake,
    ProposalExchange,
    Transfer,
    Done,
}

struct Proposal {
    fc: B2fFrame,
    compressed_data: Vec<u8>,
    answer: Option<FsAnswer>,
}

/// Most proposals an IRS session will accept before rejecting the rest — bounds how many messages a
/// remote peer/CMS can make us receive, decompress, and retain in one session (audit B-2). A real
/// Winlink batch is a handful; 32 is generous headroom.
const MAX_PROPOSALS: usize = 32;

/// Hard ceiling on inbound lines per session. A well-behaved B2F session is tens of frames; this only
/// terminates an untrusted peer that streams valid-but-non-terminating frames (the FC-flood that
/// never sends FF), which the per-frame receive loops in the driver/gateway would otherwise spin on
/// forever. Generous so no legitimate mailbox trips it.
const MAX_SESSION_FRAMES: usize = 8192;

/// Ceiling on total decompressed bytes across ONE session. The per-message cap (`MAX_UNCOMPRESSED`,
/// 16 MiB) bounds each message but not their product: `MAX_PROPOSALS` × 16 MiB ≈ 512 MB of transient
/// allocation from ~2 MB of wire — a plausible OOM on the Pi target. Checked after each decompress,
/// so peak stays under this plus one message. Generous: a real mailbox batch is a few hundred KB.
const MAX_SESSION_DECOMPRESSED: u64 = 32 * 1024 * 1024;

/// B2F session state machine.
///
/// Feed inbound lines via `handle_line`; call `drain_pending_data` to get
/// any compressed message bytes that should be written to the data channel.
pub struct B2fSession {
    pub role: SessionRole,
    proposals: Vec<Proposal>,
    /// IRS: count of FC proposals seen beyond `MAX_PROPOSALS`. Retained only as a count (not full
    /// `Proposal`s) so a flood can't grow the heap, while `handle_proposal`'s Ff answer can still
    /// reply Reject to each — preserving the one-answer-per-proposal correspondence a legit >32 batch
    /// needs. See the audit follow-up on unbounded proposal accumulation.
    overflow_rejected: usize,
    /// Total inbound lines processed this session. Bounds an untrusted peer that streams valid but
    /// non-terminating frames (e.g. FC forever, never FF): `handle_line` errors past `MAX_SESSION_FRAMES`.
    frames_seen: usize,
    state: SessionState,
    pending_data: Vec<Vec<u8>>,
    /// IRS: index of the next proposal whose data arrives via `receive_data`.
    receive_idx: usize,
    /// IRS: running total of decompressed bytes this session, bounded by `MAX_SESSION_DECOMPRESSED`.
    decompressed_total: u64,
}

impl B2fSession {
    pub fn new(role: SessionRole) -> Self {
        Self {
            role,
            proposals: Vec::new(),
            overflow_rejected: 0,
            frames_seen: 0,
            state: SessionState::Handshake,
            pending_data: Vec::new(),
            receive_idx: 0,
            decompressed_total: 0,
        }
    }

    /// ISS: queue a message as proposal type D (Gzip) in the next `ProposalExchange`.
    ///
    /// The compressed blob includes the CRLF-terminated header block followed
    /// by the raw body bytes — matching the wire format expected by real Winlink CMS.
    pub fn queue_message(&mut self, header: WlHeader, body: Vec<u8>) -> Result<(), B2fError> {
        let mut full = crate::header::encode(&header);
        full.extend_from_slice(&body);
        let compressed = compress_gzip(&full)?;
        let size = compressed.len() as u32;
        self.proposals.push(Proposal {
            fc: B2fFrame::Fc {
                proposal_type: ProposalType::D,
                mid: header.mid.clone(),
                size,
                date: header.date.clone(),
            },
            compressed_data: compressed,
            answer: None,
        });
        Ok(())
    }

    /// Feed one inbound line from the data channel.
    ///
    /// Returns lines that should be written back to the data channel.
    pub fn handle_line(&mut self, line: &str) -> Result<Vec<String>, B2fError> {
        self.frames_seen += 1;
        if self.frames_seen > MAX_SESSION_FRAMES {
            return Err(B2fError::TooManyFrames {
                limit: MAX_SESSION_FRAMES,
            });
        }
        match self.state {
            SessionState::Handshake => self.handle_handshake(line),
            SessionState::ProposalExchange => self.handle_proposal(line),
            SessionState::Transfer => self.handle_transfer(line),
            SessionState::Done => Ok(vec![]),
        }
    }

    /// Drain compressed message bytes ready to send over the data channel.
    ///
    /// Transitions ISS to Done once all staged data has been drained.
    pub fn drain_pending_data(&mut self) -> Vec<Vec<u8>> {
        let data = std::mem::take(&mut self.pending_data);
        if self.state == SessionState::Transfer {
            self.state = SessionState::Done;
        }
        data
    }

    /// Whether the session has finished.
    pub fn is_done(&self) -> bool {
        self.state == SessionState::Done
    }

    /// IRS: number of full proposals retained in memory (bounded by `MAX_PROPOSALS`). Distinct from
    /// the number the peer *sent*, which may be larger — the overflow is answered Reject but not kept.
    pub fn retained_proposals(&self) -> usize {
        self.proposals.len()
    }

    /// IRS: total number of proposals that were accepted.
    pub fn accepted_count(&self) -> usize {
        self.proposals
            .iter()
            .filter(|p| matches!(p.answer, Some(FsAnswer::Accept)))
            .count()
    }

    fn handle_handshake(&mut self, line: &str) -> Result<Vec<String>, B2fError> {
        // Expect the remote banner; transition to proposal exchange.
        // IRS may receive FC lines before a banner if ISS sends proposals immediately.
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let is_banner = trimmed.starts_with('[') && trimmed.ends_with(']');
        match self.role {
            SessionRole::Iss => {
                // ISS must receive a valid remote banner before advancing state.
                if !is_banner {
                    return Err(B2fError::InvalidBanner(trimmed.to_string()));
                }
                crate::banner::decode(line)?;
                self.state = SessionState::ProposalExchange;
                let mut out: Vec<String> = self
                    .proposals
                    .iter()
                    .map(|p| frame::encode(&p.fc))
                    .collect();
                out.push(frame::encode(&B2fFrame::Ff));
                Ok(out)
            }
            SessionRole::Irs => {
                if is_banner {
                    crate::banner::decode(line)?;
                }
                self.state = SessionState::ProposalExchange;
                // If we jumped straight to an FC/FF line, process it now.
                if !is_banner {
                    self.handle_proposal(line)
                } else {
                    Ok(vec![])
                }
            }
        }
    }

    fn handle_proposal(&mut self, line: &str) -> Result<Vec<String>, B2fError> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        // Banner lines can arrive here when ISS sends proposals immediately after
        // receiving the IRS banner; ignore them rather than treating as an error.
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            return Ok(vec![]);
        }
        let f = frame::decode(line)?;
        match (self.role.clone(), f) {
            (SessionRole::Iss, B2fFrame::Fs { answers }) => {
                if answers.len() != self.proposals.len() {
                    return Err(B2fError::ProposalCountMismatch {
                        expected: self.proposals.len(),
                        got: answers.len(),
                    });
                }
                for (i, answer) in answers.into_iter().enumerate() {
                    let p = &mut self.proposals[i];
                    p.answer = Some(answer.clone());
                    if answer == FsAnswer::Accept {
                        self.pending_data.push(p.compressed_data.clone());
                    }
                }
                self.state = if self.pending_data.is_empty() {
                    SessionState::Done
                } else {
                    SessionState::Transfer
                };
                Ok(vec![])
            }
            (
                SessionRole::Irs,
                B2fFrame::Fc {
                    mid,
                    size,
                    proposal_type,
                    date,
                },
            ) => {
                // Accept up to MAX_PROPOSALS; reject the rest so a hostile peer can't make us receive
                // and retain an unbounded number of (decompressed) messages in one session (audit B-2).
                // Retain at most MAX_PROPOSALS full proposals; beyond that only COUNT the overflow
                // (Reject) so the heap is bounded no matter how many FCs a hostile peer streams. The
                // Ff arm still replies one answer per proposal (recorded Accepts + overflow Rejects).
                // Type C (LZHUF) is not supported — answer Reject rather than accept a message we
                // cannot decode. Accepting it would mean either a silent corrupt decode or an abort
                // mid-transfer; a Reject leaves the peer free to re-propose as Type D (Gzip).
                let answer = match proposal_type {
                    ProposalType::D => FsAnswer::Accept,
                    ProposalType::C => FsAnswer::Reject,
                };
                if self.proposals.len() < MAX_PROPOSALS {
                    self.proposals.push(Proposal {
                        fc: B2fFrame::Fc {
                            proposal_type,
                            mid,
                            size,
                            date,
                        },
                        compressed_data: Vec::new(),
                        answer: Some(answer),
                    });
                } else {
                    self.overflow_rejected += 1;
                }
                Ok(vec![])
            }
            (SessionRole::Irs, B2fFrame::Ff) => {
                // Answer each proposal with its recorded decision (Accept within the cap, Reject beyond).
                let mut answers: Vec<FsAnswer> = self
                    .proposals
                    .iter()
                    .map(|p| p.answer.clone().unwrap_or(FsAnswer::Reject))
                    .collect();
                // One answer per proposal the peer sent, including the ones dropped past the cap.
                answers.extend(std::iter::repeat_n(
                    FsAnswer::Reject,
                    self.overflow_rejected,
                ));
                self.state = SessionState::Transfer;
                Ok(vec![frame::encode(&B2fFrame::Fs { answers })])
            }
            (_, B2fFrame::Fq) => {
                self.state = SessionState::Done;
                Ok(vec![])
            }
            (role, frame) => {
                tracing::debug!(
                    ?role,
                    ?frame,
                    state = ?self.state,
                    "B2F: unexpected (role, frame) combination; no response sent"
                );
                Ok(vec![])
            }
        }
    }

    fn handle_transfer(&mut self, _line: &str) -> Result<Vec<String>, B2fError> {
        // Data-channel bytes are handled outside the line protocol; mark done.
        self.state = SessionState::Done;
        Ok(vec![])
    }

    /// IRS: ingest compressed message data received on the data channel.
    ///
    /// Selects decompressor based on the accepted proposal type (D=Gzip, C=LZHUF).
    pub fn receive_data(&mut self, data: Vec<u8>) -> Result<Vec<u8>, B2fError> {
        if self.state != SessionState::Transfer {
            return Err(B2fError::InvalidState);
        }
        let proposal = self
            .proposals
            .get(self.receive_idx)
            .ok_or(B2fError::InvalidState)?;
        let proposal_type = if let B2fFrame::Fc { proposal_type, .. } = &proposal.fc {
            proposal_type.clone()
        } else {
            return Err(B2fError::InvalidState);
        };
        self.receive_idx += 1;
        if self.receive_idx >= self.proposals.len() {
            self.state = SessionState::Done;
        }
        let out = match proposal_type {
            ProposalType::D => decompress_gzip(&data)?,
            // Unreachable via the normal path: a Type C proposal is answered Reject, so its data is
            // never requested. Kept explicit so a future accept-path change fails loudly here.
            ProposalType::C => {
                return Err(B2fError::Compression(
                    "proposal type C (LZHUF) is not supported".into(),
                ))
            }
        };
        self.decompressed_total = self.decompressed_total.saturating_add(out.len() as u64);
        if self.decompressed_total > MAX_SESSION_DECOMPRESSED {
            return Err(B2fError::SessionTooLarge {
                limit: MAX_SESSION_DECOMPRESSED,
            });
        }
        Ok(out)
    }
}
