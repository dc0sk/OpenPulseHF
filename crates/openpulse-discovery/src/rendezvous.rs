//! Rendezvous protocol (plan §5.3, decision D3): a compact **2-message** JS8 exchange that agrees a
//! working frequency with a discovered `@OPULSE` peer, then hands off to the existing signed HPX
//! CONREQ/CONACK handshake after QSY.
//!
//! The messages ride as JS8 directed free-text to the peer; this module is the pure codec + session
//! logic (no I/O, no timing — the daemon supplies slots and does the QSY/handshake). Channels are
//! **indices** into a per-band channel table (2 chars, not 7 digits of Hz) so a proposal fits ~2
//! frames. There is deliberately **no signature**: rendezvous only agrees where to meet, and the
//! post-QSY signed CONREQ is the authentication (a spoof wastes ≤ one timeout and fails the handshake).
//!
//! ```text
//! DC0SK:  KN4CRD OPHF QSY? R7 C3 C9 K2   # propose token R7, ranked channels 3, 9, 2
//! KN4CRD: DC0SK  OPHF QSY  R7 C9 S4      # accept channel 9, switch in 4 slots
//! KN4CRD: DC0SK  OPHF NO   R7 F          # or reject (F = no common frequency)
//! ```

/// Why a rendezvous proposal was rejected (single-letter wire code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// `B` — the responder is busy / not accepting.
    Busy,
    /// `T` — trust policy declined.
    Trust,
    /// `F` — no proposed channel is usable here.
    NoCommonFreq,
    /// `X` — unspecified.
    Other,
}

impl RejectReason {
    fn code(self) -> char {
        match self {
            RejectReason::Busy => 'B',
            RejectReason::Trust => 'T',
            RejectReason::NoCommonFreq => 'F',
            RejectReason::Other => 'X',
        }
    }
    fn from_code(c: char) -> Self {
        match c.to_ascii_uppercase() {
            'B' => RejectReason::Busy,
            'T' => RejectReason::Trust,
            'F' => RejectReason::NoCommonFreq,
            _ => RejectReason::Other,
        }
    }
}

/// A rendezvous message (the OPHF free text of a directed JS8 message to the peer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendezvousMsg {
    /// Initiator → responder: agree on one of these ranked channel indices.
    Propose {
        /// 2-char session token.
        token: String,
        /// Ranked candidate channel indices (most-preferred first).
        channels: Vec<u8>,
    },
    /// Responder → initiator: accept `channel`, both QSY in `switch_in_slots` slots.
    Accept {
        /// Echoed session token.
        token: String,
        /// Chosen channel index.
        channel: u8,
        /// Slots until the QSY.
        switch_in_slots: u8,
    },
    /// Responder → initiator: decline.
    Reject {
        /// Echoed session token.
        token: String,
        /// Reason code.
        reason: RejectReason,
    },
}

impl RendezvousMsg {
    /// The token this message carries.
    pub fn token(&self) -> &str {
        match self {
            RendezvousMsg::Propose { token, .. }
            | RendezvousMsg::Accept { token, .. }
            | RendezvousMsg::Reject { token, .. } => token,
        }
    }

    /// Encode to OPHF free text (the JS8 directed-message body).
    pub fn encode(&self) -> String {
        match self {
            RendezvousMsg::Propose { token, channels } => {
                let mut s = format!("OPHF QSY? {token}");
                for c in channels {
                    s.push_str(&format!(" C{c}"));
                }
                s
            }
            RendezvousMsg::Accept {
                token,
                channel,
                switch_in_slots,
            } => format!("OPHF QSY {token} C{channel} S{switch_in_slots}"),
            RendezvousMsg::Reject { token, reason } => {
                format!("OPHF NO {token} {}", reason.code())
            }
        }
    }

    /// Decode OPHF free text, or `None` if it is not a well-formed rendezvous message.
    pub fn decode(text: &str) -> Option<RendezvousMsg> {
        let mut it = text.split_whitespace();
        if it.next()? != "OPHF" {
            return None;
        }
        match it.next()? {
            "QSY?" => {
                let token = valid_token(it.next()?)?;
                let channels: Vec<u8> = it
                    .map(|t| t.strip_prefix('C').and_then(|n| n.parse().ok()))
                    .collect::<Option<_>>()?;
                (!channels.is_empty()).then_some(RendezvousMsg::Propose { token, channels })
            }
            "QSY" => {
                let token = valid_token(it.next()?)?;
                let channel = it.next()?.strip_prefix('C')?.parse().ok()?;
                let switch_in_slots = it.next()?.strip_prefix('S')?.parse().ok()?;
                Some(RendezvousMsg::Accept {
                    token,
                    channel,
                    switch_in_slots,
                })
            }
            "NO" => {
                let token = valid_token(it.next()?)?;
                let reason = RejectReason::from_code(it.next()?.chars().next()?);
                Some(RendezvousMsg::Reject { token, reason })
            }
            _ => None,
        }
    }
}

/// A token is exactly 2 uppercase base-36 chars.
fn valid_token(s: &str) -> Option<String> {
    let up = s.to_ascii_uppercase();
    (up.len() == 2
        && up
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
    .then_some(up)
}

/// Default slots between an `Accept` and the QSY (≈ time for the accept to be heard + both to retune).
pub const DEFAULT_SWITCH_SLOTS: u8 = 4;

/// What the caller (daemon) should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendezvousOutcome {
    /// Transmit this message to the peer over JS8.
    Send(RendezvousMsg),
    /// Both agreed — QSY to channel index `channel` in `switch_in_slots` slots, then run CONREQ.
    Qsy {
        /// Agreed channel index.
        channel: u8,
        /// Slots until the QSY.
        switch_in_slots: u8,
    },
    /// The exchange failed (peer rejected, or we could not accept).
    Rejected(RejectReason),
    /// The proposal expired with no response.
    TimedOut,
}

/// Initiator-side rendezvous session: propose, then await accept / reject / timeout.
#[derive(Debug, Clone)]
pub struct RendezvousInitiator {
    peer: String,
    token: String,
    slots_waited: u64,
    timeout_slots: u64,
    done: bool,
}

impl RendezvousInitiator {
    /// Start a rendezvous with `peer`, proposing `channels` (ranked). Returns the session and the
    /// `Propose` message to transmit.
    pub fn start(
        peer: &str,
        token: &str,
        channels: Vec<u8>,
        timeout_slots: u64,
    ) -> (Self, RendezvousMsg) {
        let token = token.to_ascii_uppercase();
        let msg = RendezvousMsg::Propose {
            token: token.clone(),
            channels,
        };
        (
            Self {
                peer: peer.trim().to_ascii_uppercase(),
                token,
                slots_waited: 0,
                timeout_slots,
                done: false,
            },
            msg,
        )
    }

    /// The peer we are rendezvousing with.
    pub fn peer(&self) -> &str {
        &self.peer
    }

    /// Handle a decoded rendezvous message from the peer. Returns `Qsy` on accept (with a matching
    /// token) or `Rejected`; ignores mismatched tokens / stray messages.
    pub fn on_message(&mut self, msg: &RendezvousMsg) -> Option<RendezvousOutcome> {
        if self.done || msg.token() != self.token {
            return None;
        }
        match msg {
            RendezvousMsg::Accept {
                channel,
                switch_in_slots,
                ..
            } => {
                self.done = true;
                Some(RendezvousOutcome::Qsy {
                    channel: *channel,
                    switch_in_slots: *switch_in_slots,
                })
            }
            RendezvousMsg::Reject { reason, .. } => {
                self.done = true;
                Some(RendezvousOutcome::Rejected(*reason))
            }
            RendezvousMsg::Propose { .. } => None,
        }
    }

    /// Advance one slot; returns `TimedOut` once the proposal has waited past `timeout_slots`.
    pub fn on_slot(&mut self) -> Option<RendezvousOutcome> {
        if self.done {
            return None;
        }
        self.slots_waited += 1;
        if self.slots_waited >= self.timeout_slots {
            self.done = true;
            return Some(RendezvousOutcome::TimedOut);
        }
        None
    }

    /// Whether the session has concluded.
    pub fn is_done(&self) -> bool {
        self.done
    }
}

/// Responder decision: given a `Propose` and the channels usable here, pick the highest-ranked common
/// channel and `Accept` it, else `Reject` with `NoCommonFreq`. `available` is checked in the order the
/// initiator ranked them.
pub fn respond(
    propose: &RendezvousMsg,
    available: &[u8],
    switch_in_slots: u8,
) -> RendezvousOutcome {
    let RendezvousMsg::Propose { token, channels } = propose else {
        return RendezvousOutcome::Rejected(RejectReason::Other);
    };
    match channels.iter().find(|c| available.contains(c)) {
        Some(&channel) => RendezvousOutcome::Send(RendezvousMsg::Accept {
            token: token.clone(),
            channel,
            switch_in_slots,
        }),
        None => RendezvousOutcome::Send(RendezvousMsg::Reject {
            token: token.clone(),
            reason: RejectReason::NoCommonFreq,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_codec_round_trips() {
        let msgs = [
            RendezvousMsg::Propose {
                token: "R7".into(),
                channels: vec![3, 9, 2],
            },
            RendezvousMsg::Accept {
                token: "R7".into(),
                channel: 9,
                switch_in_slots: 4,
            },
            RendezvousMsg::Reject {
                token: "R7".into(),
                reason: RejectReason::NoCommonFreq,
            },
        ];
        for m in msgs {
            assert_eq!(RendezvousMsg::decode(&m.encode()), Some(m.clone()), "{m:?}");
        }
        assert_eq!(
            RendezvousMsg::Propose {
                token: "R7".into(),
                channels: vec![3, 9, 2]
            }
            .encode(),
            "OPHF QSY? R7 C3 C9 C2"
        );
    }

    #[test]
    fn decode_rejects_non_rendezvous_text() {
        assert_eq!(RendezvousMsg::decode("HELLO WORLD"), None);
        assert_eq!(RendezvousMsg::decode("OPHF1 A1B2C3D4"), None); // the capability hint, not rendezvous
        assert_eq!(RendezvousMsg::decode("OPHF QSY?"), None); // no token/channels
        assert_eq!(RendezvousMsg::decode("OPHF QSY? TOOLONG C3"), None); // bad token
    }

    #[test]
    fn responder_picks_the_highest_ranked_common_channel() {
        let propose = RendezvousMsg::Propose {
            token: "R7".into(),
            channels: vec![3, 9, 2],
        };
        // Channel 3 is unavailable here; 9 is the next ranked common channel.
        match respond(&propose, &[9, 2, 5], DEFAULT_SWITCH_SLOTS) {
            RendezvousOutcome::Send(RendezvousMsg::Accept { channel, .. }) => {
                assert_eq!(channel, 9)
            }
            other => panic!("expected accept, got {other:?}"),
        }
    }

    #[test]
    fn responder_rejects_when_no_common_channel() {
        let propose = RendezvousMsg::Propose {
            token: "R7".into(),
            channels: vec![3, 9],
        };
        match respond(&propose, &[1, 2], DEFAULT_SWITCH_SLOTS) {
            RendezvousOutcome::Send(RendezvousMsg::Reject { reason, .. }) => {
                assert_eq!(reason, RejectReason::NoCommonFreq)
            }
            other => panic!("expected reject, got {other:?}"),
        }
    }

    #[test]
    fn initiator_accept_flow_yields_qsy() {
        let (mut init, propose) = RendezvousInitiator::start("KN4CRD", "R7", vec![3, 9], 8);
        assert_eq!(propose.token(), "R7");
        // A response with a different token is ignored.
        let stray = RendezvousMsg::Accept {
            token: "Z9".into(),
            channel: 3,
            switch_in_slots: 4,
        };
        assert_eq!(init.on_message(&stray), None);
        // The matching accept yields QSY.
        let accept = RendezvousMsg::Accept {
            token: "R7".into(),
            channel: 9,
            switch_in_slots: 4,
        };
        assert_eq!(
            init.on_message(&accept),
            Some(RendezvousOutcome::Qsy {
                channel: 9,
                switch_in_slots: 4
            })
        );
        assert!(init.is_done());
    }

    #[test]
    fn initiator_times_out_without_a_response() {
        let (mut init, _) = RendezvousInitiator::start("KN4CRD", "R7", vec![3], 3);
        assert_eq!(init.on_slot(), None);
        assert_eq!(init.on_slot(), None);
        assert_eq!(init.on_slot(), Some(RendezvousOutcome::TimedOut));
        assert!(init.is_done());
    }

    #[test]
    fn initiator_reject_flow() {
        let (mut init, _) = RendezvousInitiator::start("KN4CRD", "R7", vec![3], 8);
        let reject = RendezvousMsg::Reject {
            token: "R7".into(),
            reason: RejectReason::Busy,
        };
        assert_eq!(
            init.on_message(&reject),
            Some(RendezvousOutcome::Rejected(RejectReason::Busy))
        );
    }
}
