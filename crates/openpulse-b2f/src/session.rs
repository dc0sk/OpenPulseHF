//! B2F session state machine for ISS (sending) and IRS (receiving) roles.

use crate::compress::{compress_gzip, decompress_gzip, decompress_lzhuf};
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

/// B2F session state machine.
///
/// Feed inbound lines via `handle_line`; call `drain_pending_data` to get
/// any compressed message bytes that should be written to the data channel.
pub struct B2fSession {
    pub role: SessionRole,
    proposals: Vec<Proposal>,
    state: SessionState,
    pending_data: Vec<Vec<u8>>,
    /// IRS: index of the next proposal whose data arrives via `receive_data`.
    receive_idx: usize,
}

impl B2fSession {
    pub fn new(role: SessionRole) -> Self {
        Self {
            role,
            proposals: Vec::new(),
            state: SessionState::Handshake,
            pending_data: Vec::new(),
            receive_idx: 0,
        }
    }

    /// ISS: queue a message to propose in the next `ProposalExchange`.
    pub fn queue_message(&mut self, header: WlHeader, body: Vec<u8>) -> Result<(), B2fError> {
        let compressed = compress_gzip(&body)?;
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

    /// IRS: number of proposals that were accepted (data frames still to receive).
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
                // Record answers and stage accepted data.
                for (i, answer) in answers.into_iter().enumerate() {
                    if let Some(p) = self.proposals.get_mut(i) {
                        p.answer = Some(answer.clone());
                        if answer == FsAnswer::Accept {
                            self.pending_data.push(p.compressed_data.clone());
                        }
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
                self.proposals.push(Proposal {
                    fc: B2fFrame::Fc {
                        proposal_type,
                        mid,
                        size,
                        date,
                    },
                    compressed_data: Vec::new(),
                    answer: Some(FsAnswer::Accept),
                });
                Ok(vec![])
            }
            (SessionRole::Irs, B2fFrame::Ff) => {
                // Send FS response accepting all proposals.
                let answers: Vec<FsAnswer> =
                    self.proposals.iter().map(|_| FsAnswer::Accept).collect();
                self.state = SessionState::Transfer;
                Ok(vec![frame::encode(&B2fFrame::Fs { answers })])
            }
            (_, B2fFrame::Fq) => {
                self.state = SessionState::Done;
                Ok(vec![])
            }
            _ => Ok(vec![]),
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
        match proposal_type {
            ProposalType::D => decompress_gzip(&data),
            ProposalType::C => decompress_lzhuf(&data),
        }
    }
}
