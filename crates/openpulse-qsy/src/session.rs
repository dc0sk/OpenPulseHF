//! QSY negotiation state machine.
//!
//! Drive via `initiate()` (initiator role) or `apply()` (both roles) plus
//! `scan_complete()` after the rig scan finishes.

use openpulse_core::trust::ConnectionTrustLevel;
use rand::Rng;
use thiserror::Error;

use crate::bandplan::{BandplanMode, BandplanPolicy};
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
#[derive(Debug, Clone)]
pub struct QsyPolicy {
    /// When false, all incoming `QSY_REQ` frames are immediately rejected.
    pub enabled: bool,
    /// Trust levels from which `QSY_REQ` is accepted.  An empty list accepts any level.
    pub allow_trustlevels: Vec<ConnectionTrustLevel>,
    /// Bandplan awareness settings for QSY frequencies.
    pub bandplan: BandplanPolicy,
}

impl QsyPolicy {
    /// Build a `QsyPolicy` from config values, parsing trust-level strings.
    ///
    /// Accepts both kebab-case (`"psk-verified"`) and underscore variants (`"psk_verified"`).
    /// Returns `Err` listing any unrecognised strings so misconfiguration is visible at startup
    /// rather than silently opening trust gating.
    pub fn from_config(
        enabled: bool,
        allow_trustlevels: &[String],
        bandplan_mode: &str,
        bandplan_awareness_enabled: bool,
        enforce_max_channel_width: bool,
        enforce_segment_conventions: bool,
    ) -> Result<Self, String> {
        let mut levels = Vec::new();
        let mut unknown = Vec::new();
        for s in allow_trustlevels {
            match s.parse::<ConnectionTrustLevel>() {
                Ok(level) => levels.push(level),
                Err(_) => unknown.push(s.as_str()),
            }
        }
        if !unknown.is_empty() {
            return Err(format!(
                "unknown trust level(s) in allow_trustlevels: {}",
                unknown.join(", ")
            ));
        }

        let parsed_mode = bandplan_mode
            .parse::<BandplanMode>()
            .map_err(|_| format!("unknown qsy.bandplan_mode: {bandplan_mode}"))?;

        Ok(Self {
            enabled,
            allow_trustlevels: levels,
            bandplan: BandplanPolicy {
                awareness_enabled: bandplan_awareness_enabled,
                mode: parsed_mode,
                enforce_max_channel_width,
                enforce_segment_conventions,
            },
        })
    }
}

impl Default for QsyPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_trustlevels: vec![],
            bandplan: BandplanPolicy {
                awareness_enabled: false,
                ..BandplanPolicy::default()
            },
        }
    }
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
        n_candidates: u32,
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
    peer_trust: ConnectionTrustLevel,
    switchover_offset_s: u32,
    operating_mode: Option<String>,
}

impl QsySession {
    /// Create a session for the station that will start the negotiation.
    pub fn new_initiator() -> Self {
        Self {
            role: Role::Initiator,
            state: State::Idle,
            policy: QsyPolicy::default(),
            peer_trust: ConnectionTrustLevel::Unverified,
            switchover_offset_s: 5,
            operating_mode: None,
        }
    }

    /// Create a session for the station that will respond to a negotiation request.
    ///
    /// `peer_trust` is the classified trust level of the connected peer (from the HPX handshake).
    /// The policy's `allow_trustlevels` list is checked against it when a `QSY_REQ` arrives.
    pub fn new_responder(policy: QsyPolicy, peer_trust: ConnectionTrustLevel) -> Self {
        Self {
            role: Role::Responder,
            state: State::Idle,
            policy,
            peer_trust,
            switchover_offset_s: 5,
            operating_mode: None,
        }
    }

    /// Override policy (useful for initiator config wiring).
    pub fn with_policy(mut self, policy: QsyPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Set modem operating mode used for bandplan channel-width checks.
    pub fn with_operating_mode(mut self, mode: impl Into<String>) -> Self {
        self.operating_mode = Some(mode.into());
        self
    }

    /// Override the switchover offset (seconds after QSY_ACK to tune).
    pub fn with_switchover_offset_s(mut self, v: u32) -> Self {
        self.switchover_offset_s = v;
        self
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
        if candidates.is_empty() {
            return Err(QsyError::InvalidTransition(
                "candidate list must not be empty".into(),
            ));
        }

        self.validate_frequencies(candidates.iter().copied())?;

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
                if !self.policy.allow_trustlevels.is_empty()
                    && !self.policy.allow_trustlevels.contains(&self.peer_trust)
                {
                    self.state = State::Rejected;
                    return Ok(vec![
                        QsyAction::SendFrame(QsyFrame::Reject {
                            token,
                            reason: "trust level not permitted".into(),
                        }),
                        QsyAction::Reject {
                            reason: "trust level not permitted".into(),
                        },
                    ]);
                }
                self.state = State::WaitingForList {
                    token,
                    n_candidates,
                };
                Ok(vec![])
            }
            QsyFrame::List { token, candidates } => {
                match &self.state {
                    State::WaitingForList {
                        token: t,
                        n_candidates,
                    } => {
                        if *t != token {
                            return Err(QsyError::TokenMismatch {
                                expected: t.clone(),
                                got: token,
                            });
                        }
                        let expected = *n_candidates as usize;
                        if candidates.len() != expected {
                            return Err(QsyError::InvalidTransition(format!(
                                "QSY_LIST has {} candidates, expected {expected}",
                                candidates.len()
                            )));
                        }
                    }
                    other => {
                        return Err(QsyError::InvalidTransition(format!(
                            "QSY_LIST in unexpected state: {other:?}"
                        )))
                    }
                }

                self.validate_frequencies(candidates.iter().map(|(freq_hz, _)| *freq_hz))?;

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
                    match pick_best_freq(my_votes, &votes) {
                        Ok(best) => {
                            self.state = State::Agreed { freq_hz: best };
                            Ok(vec![
                                QsyAction::SendFrame(QsyFrame::Ack {
                                    token,
                                    agreed_freq_hz: best,
                                    switchover_offset_s: self.switchover_offset_s,
                                }),
                                QsyAction::QsyNow { freq_hz: best },
                            ])
                        }
                        Err(_) => {
                            // No common candidate — send explicit rejection so the peer
                            // doesn't hang waiting for an ACK that will never arrive.
                            let reason = "no common candidate frequency".to_string();
                            self.state = State::Rejected;
                            Ok(vec![
                                QsyAction::SendFrame(QsyFrame::Reject {
                                    token,
                                    reason: reason.clone(),
                                }),
                                QsyAction::Reject { reason },
                            ])
                        }
                    }
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
                    self.validate_frequencies(std::iter::once(agreed_freq_hz))?;
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

impl QsySession {
    fn validate_frequencies(&self, freqs: impl IntoIterator<Item = u64>) -> Result<(), QsyError> {
        if !self.policy.bandplan.awareness_enabled {
            return Ok(());
        }

        if self.policy.bandplan.enforce_max_channel_width && self.operating_mode.is_none() {
            return Err(QsyError::InvalidTransition(
                "operating_mode must be set when max-channel-width bandplan enforcement is enabled"
                    .into(),
            ));
        }

        // Segment-only validation does not require a mode string.
        let mode = self.operating_mode.as_deref().unwrap_or("");

        for freq_hz in freqs {
            self.policy
                .bandplan
                .validate_frequency(freq_hz, mode)
                .map_err(|e| {
                    QsyError::InvalidTransition(format!("bandplan policy violation: {e}"))
                })?;
        }
        Ok(())
    }
}

/// Pick the frequency with the highest combined (initiator + partner) SNR.
///
/// Only considers frequencies present in both lists; returns an error if the
/// intersection is empty (no common candidate to agree on).
fn pick_best_freq(my: &[(u64, f32)], partner: &[(u64, f32)]) -> Result<u64, QsyError> {
    let mut best_freq: Option<u64> = None;
    let mut best_score = f32::NEG_INFINITY;
    for (freq, my_snr) in my {
        if let Some((_, partner_snr)) = partner.iter().find(|(f, _)| f == freq) {
            let score = my_snr + partner_snr;
            if score > best_score {
                best_score = score;
                best_freq = Some(*freq);
            }
        }
    }
    best_freq.ok_or_else(|| {
        QsyError::InvalidTransition(
            "no common candidate in initiator and partner vote lists".into(),
        )
    })
}

fn random_token() -> String {
    let v: u32 = rand::thread_rng().gen();
    format!("{v:08x}")
}
