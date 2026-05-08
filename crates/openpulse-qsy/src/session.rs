//! QSY negotiation state machine.
//!
//! Drive via `initiate()` (initiator role) or `apply()` (both roles) plus
//! `scan_complete()` after the rig scan finishes.

use rand::Rng;
use thiserror::Error;

use crate::frame::{QsyFrame, QsyFrameError};

#[derive(Debug, Error)]
pub enum QsyError {
    #[error("invalid state transition: {0}")]
    InvalidTransition(String),
    #[error("token mismatch: expected {expected}, got {got}")]
    TokenMismatch { expected: String, got: String },
    #[error("frame error: {0}")]
    Frame(#[from] QsyFrameError),
}

/// Policy governing whether this node accepts QSY requests.
#[derive(Debug, Clone, Default)]
pub struct QsyPolicy {
    /// When false, all incoming `QSY_REQ` frames are immediately rejected.
    pub enabled: bool,
}

/// Actions returned by the state machine for the caller to execute.
#[derive(Debug, Clone)]
pub enum QsyAction {
    /// Encode and transmit this frame to the peer.
    SendFrame(QsyFrame),
    /// Begin scanning these candidate frequencies.
    StartScan { candidates: Vec<u64> },
    /// Command the rig to this frequency, then resume the session.
    QsyNow { freq_hz: u64 },
    /// Negotiation was rejected; caller may send `QSY_REJECT` if not already done.
    Reject { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
enum Role {
    Initiator,
    Responder,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum State {
    Idle,
    /// Initiator: local scan in progress.
    InitScanning {
        token: String,
        candidates: Vec<u64>,
    },
    /// Initiator: sent QSY_LIST, waiting for partner's VOTE.
    Listed {
        token: String,
        my_votes: Vec<(u64, f32)>,
    },
    /// Responder: received QSY_REQ, waiting for initiator's QSY_LIST.
    WaitingForList {
        token: String,
    },
    /// Responder: scanning candidates received in QSY_LIST.
    RespScanning {
        token: String,
    },
    /// Responder: sent QSY_VOTE, waiting for QSY_ACK.
    Voted {
        token: String,
    },
    Agreed {
        freq_hz: u64,
    },
    Rejected,
}

/// State machine for one QSY negotiation.
pub struct QsySession {
    role: Role,
    state: State,
    policy: QsyPolicy,
}

impl QsySession {
    /// Create a session for the station that will start the negotiation.
    pub fn new_initiator() -> Self {
        Self {
            role: Role::Initiator,
            state: State::Idle,
            policy: QsyPolicy { enabled: true },
        }
    }

    /// Create a session for the station that will respond to a negotiation request.
    pub fn new_responder(policy: QsyPolicy) -> Self {
        Self {
            role: Role::Responder,
            state: State::Idle,
            policy,
        }
    }

    /// Begin a QSY negotiation (initiator only).
    ///
    /// Returns `[SendFrame(QSY_REQ), StartScan{candidates}]`.
    pub fn initiate(&mut self, candidates: Vec<u64>) -> Result<Vec<QsyAction>, QsyError> {
        if self.role != Role::Initiator {
            return Err(QsyError::InvalidTransition(
                "initiate() called on responder".into(),
            ));
        }
        if !matches!(self.state, State::Idle) {
            return Err(QsyError::InvalidTransition(
                "initiate() called in non-Idle state".into(),
            ));
        }
        let token = random_token();
        let n = candidates.len() as u32;
        self.state = State::InitScanning {
            token: token.clone(),
            candidates: candidates.clone(),
        };
        Ok(vec![
            QsyAction::SendFrame(QsyFrame::Req {
                token,
                n_candidates: n,
            }),
            QsyAction::StartScan { candidates },
        ])
    }

    /// Supply scan results after `StartScan` completes (both roles).
    ///
    /// Initiator: transitions to Listed, returns `[SendFrame(QSY_LIST)]`.
    /// Responder: transitions to Voted, returns `[SendFrame(QSY_VOTE)]`.
    pub fn scan_complete(&mut self, results: Vec<(u64, f32)>) -> Result<Vec<QsyAction>, QsyError> {
        match &self.state {
            State::InitScanning { token, .. } => {
                let token = token.clone();
                self.state = State::Listed {
                    token: token.clone(),
                    my_votes: results.clone(),
                };
                Ok(vec![QsyAction::SendFrame(QsyFrame::List {
                    token,
                    candidates: results,
                })])
            }
            State::RespScanning { token } => {
                let token = token.clone();
                self.state = State::Voted {
                    token: token.clone(),
                };
                Ok(vec![QsyAction::SendFrame(QsyFrame::Vote {
                    token,
                    votes: results,
                })])
            }
            other => Err(QsyError::InvalidTransition(format!(
                "scan_complete() in unexpected state: {other:?}"
            ))),
        }
    }

    /// Apply an incoming frame and return the resulting actions.
    pub fn apply(&mut self, frame: QsyFrame) -> Result<Vec<QsyAction>, QsyError> {
        match frame {
            QsyFrame::Reject { token: _, reason } => {
                self.state = State::Rejected;
                Ok(vec![QsyAction::Reject {
                    reason: reason.clone(),
                }])
            }
            QsyFrame::Req {
                token,
                n_candidates,
            } => {
                if !matches!(self.state, State::Idle) || self.role != Role::Responder {
                    return Err(QsyError::InvalidTransition(
                        "QSY_REQ received in unexpected state/role".into(),
                    ));
                }
                if !self.policy.enabled {
                    self.state = State::Rejected;
                    return Ok(vec![
                        QsyAction::SendFrame(QsyFrame::Reject {
                            token,
                            reason: "qsy disabled".into(),
                        }),
                        QsyAction::Reject {
                            reason: "qsy disabled".into(),
                        },
                    ]);
                }
                let _ = n_candidates;
                self.state = State::WaitingForList { token };
                Ok(vec![])
            }
            QsyFrame::List { token, candidates } => {
                match &self.state {
                    State::WaitingForList { token: t } => {
                        if *t != token {
                            return Err(QsyError::TokenMismatch {
                                expected: t.clone(),
                                got: token,
                            });
                        }
                    }
                    other => {
                        return Err(QsyError::InvalidTransition(format!(
                            "QSY_LIST in unexpected state: {other:?}"
                        )))
                    }
                }
                self.state = State::RespScanning {
                    token: token.clone(),
                };
                let freq_list: Vec<u64> = candidates.iter().map(|(f, _)| *f).collect();
                Ok(vec![QsyAction::StartScan {
                    candidates: freq_list,
                }])
            }
            QsyFrame::Vote { token, votes } => match &self.state {
                State::Listed { token: t, my_votes } => {
                    if *t != token {
                        return Err(QsyError::TokenMismatch {
                            expected: t.clone(),
                            got: token.clone(),
                        });
                    }
                    let best = pick_best_freq(my_votes, &votes);
                    self.state = State::Agreed { freq_hz: best };
                    Ok(vec![
                        QsyAction::SendFrame(QsyFrame::Ack {
                            token,
                            agreed_freq_hz: best,
                            switchover_offset_s: 5,
                        }),
                        QsyAction::QsyNow { freq_hz: best },
                    ])
                }
                other => Err(QsyError::InvalidTransition(format!(
                    "QSY_VOTE in unexpected state: {other:?}"
                ))),
            },
            QsyFrame::Ack {
                token,
                agreed_freq_hz,
                ..
            } => match &self.state {
                State::Voted { token: t } => {
                    if *t != token {
                        return Err(QsyError::TokenMismatch {
                            expected: t.clone(),
                            got: token,
                        });
                    }
                    self.state = State::Agreed {
                        freq_hz: agreed_freq_hz,
                    };
                    Ok(vec![QsyAction::QsyNow {
                        freq_hz: agreed_freq_hz,
                    }])
                }
                other => Err(QsyError::InvalidTransition(format!(
                    "QSY_ACK in unexpected state: {other:?}"
                ))),
            },
        }
    }
}

/// Pick the frequency with the highest combined (initiator + partner) SNR.
fn pick_best_freq(my: &[(u64, f32)], partner: &[(u64, f32)]) -> u64 {
    let mut best_freq = my.first().map(|(f, _)| *f).unwrap_or(0);
    let mut best_score = f32::NEG_INFINITY;
    for (freq, my_snr) in my {
        let partner_snr = partner
            .iter()
            .find_map(|(f, s)| if f == freq { Some(*s) } else { None })
            .unwrap_or(f32::NEG_INFINITY);
        let score = my_snr + partner_snr;
        if score > best_score {
            best_score = score;
            best_freq = *freq;
        }
    }
    best_freq
}

fn random_token() -> String {
    let v: u32 = rand::thread_rng().gen();
    format!("{v:08x}")
}
