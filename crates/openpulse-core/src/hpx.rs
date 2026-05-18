use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// State nodes in the HPX session state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HpxState {
    Idle,
    Discovery,
    Training,
    ActiveTransfer,
    Recovery,
    RelayActive,
    Teardown,
    Failed,
}

/// Events that drive `HpxState` transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HpxEvent {
    StartSession,
    LocalCancel,
    RemoteTeardown,
    DiscoveryOk,
    DiscoveryTimeout,
    TrainingOk,
    TrainingTimeout,
    TransferComplete,
    TransferError,
    QualityDrop,
    RecoveryOk,
    RecoveryTimeout,
    RelayRouteFound,
    RelayPolicyFailed,
    SignatureVerificationFailed,
}

/// Numeric reason codes embedded in `HpxTransition` for machine-readable diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HpxReasonCode {
    Success = 0x00,
    Timeout = 0x01,
    SignatureFailure = 0x02,
    QualityDrop = 0x03,
    RetriesExhausted = 0x04,
    RecoveryTimeout = 0x05,
    RelayPolicyFailed = 0x06,
    RecoveryAttemptsExhausted = 0x07,
    ManifestVerificationFailed = 0x08,
    Unclassified = 0xFF,
}

/// Audit record for a single state transition in an HPX session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HpxTransition {
    pub timestamp_ms: u64,
    pub from_state: HpxState,
    pub to_state: HpxState,
    pub event: HpxEvent,
    pub reason_code: HpxReasonCode,
    pub reason_string: String,
    pub session_id: Option<String>,
}

/// Errors returned by `HpxReactor::apply_event`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HpxStateError {
    InvalidTransition { state: HpxState, event: HpxEvent },
    TerminalState,
    SessionNotFound(String),
}

impl fmt::Display for HpxStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HpxStateError::InvalidTransition { state, event } => {
                write!(f, "invalid HPX transition: {state:?} + {event:?}")
            }
            HpxStateError::TerminalState => {
                write!(f, "HPX session is already in terminal failed state")
            }
            HpxStateError::SessionNotFound(id) => {
                write!(f, "HPX session not found: {id}")
            }
        }
    }
}

// ── Per-session state (owned by the reactor) ──────────────────────────────────

struct SessionState {
    state: HpxState,
    session_id: Option<String>,
    recovery_attempts: u8,
    arq_retries_for_chunk: u8,
    transitions: Vec<HpxTransition>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            state: HpxState::Idle,
            session_id: None,
            recovery_attempts: 0,
            arq_retries_for_chunk: 0,
            transitions: Vec::new(),
        }
    }
}

// ── Per-state handler functions ───────────────────────────────────────────────
//
// Each handler takes the event and returns the target (state, reason_code,
// reason_string) or Err(HpxStateError) if the event is not valid in that state.
// No session state is mutated inside a handler; all mutations happen in
// `HpxReactor::dispatch` after the handler returns.

type HandlerResult = Result<(HpxState, HpxReasonCode, String), HpxStateError>;

fn ok(to: HpxState, msg: &str) -> HandlerResult {
    Ok((to, HpxReasonCode::Success, msg.to_string()))
}

fn fail_inv(state: HpxState, event: HpxEvent) -> HandlerResult {
    Err(HpxStateError::InvalidTransition { state, event })
}

fn handle_idle(event: HpxEvent) -> HandlerResult {
    match event {
        HpxEvent::StartSession => ok(HpxState::Discovery, "Session started"),
        _ => fail_inv(HpxState::Idle, event),
    }
}

fn handle_discovery(event: HpxEvent) -> HandlerResult {
    match event {
        HpxEvent::DiscoveryOk => ok(HpxState::Training, "Peer discovered and verified"),
        HpxEvent::DiscoveryTimeout => Ok((
            HpxState::Failed,
            HpxReasonCode::Timeout,
            "Discovery timeout".to_string(),
        )),
        HpxEvent::SignatureVerificationFailed => Ok((
            HpxState::Failed,
            HpxReasonCode::SignatureFailure,
            "Discovery signature verification failed".to_string(),
        )),
        HpxEvent::LocalCancel => ok(HpxState::Teardown, "Local cancel during discovery"),
        _ => fail_inv(HpxState::Discovery, event),
    }
}

fn handle_training(event: HpxEvent) -> HandlerResult {
    match event {
        HpxEvent::TrainingOk => ok(HpxState::ActiveTransfer, "Training complete"),
        HpxEvent::RelayRouteFound => ok(HpxState::RelayActive, "Relay route activated"),
        HpxEvent::TrainingTimeout => Ok((
            HpxState::Failed,
            HpxReasonCode::Timeout,
            "Training timeout".to_string(),
        )),
        HpxEvent::SignatureVerificationFailed => Ok((
            HpxState::Failed,
            HpxReasonCode::SignatureFailure,
            "Training authentication failed".to_string(),
        )),
        HpxEvent::LocalCancel => ok(HpxState::Teardown, "Local cancel during training"),
        _ => fail_inv(HpxState::Training, event),
    }
}

fn handle_active_transfer(event: HpxEvent) -> HandlerResult {
    match event {
        HpxEvent::TransferComplete => ok(HpxState::Teardown, "Transfer complete"),
        HpxEvent::TransferError => Ok((
            HpxState::Recovery,
            HpxReasonCode::RetriesExhausted,
            "Transfer error, entering recovery".to_string(),
        )),
        HpxEvent::QualityDrop => Ok((
            HpxState::Recovery,
            HpxReasonCode::QualityDrop,
            "Quality drop, entering recovery".to_string(),
        )),
        HpxEvent::RelayRouteFound => ok(HpxState::RelayActive, "Relay route activated"),
        HpxEvent::SignatureVerificationFailed => Ok((
            HpxState::Recovery,
            HpxReasonCode::SignatureFailure,
            "Frame authentication failed".to_string(),
        )),
        HpxEvent::LocalCancel => ok(HpxState::Teardown, "Local cancel during transfer"),
        HpxEvent::RemoteTeardown => ok(HpxState::Teardown, "Remote teardown requested"),
        _ => fail_inv(HpxState::ActiveTransfer, event),
    }
}

fn handle_recovery(event: HpxEvent) -> HandlerResult {
    match event {
        HpxEvent::RecoveryOk => ok(HpxState::ActiveTransfer, "Recovery complete"),
        HpxEvent::RecoveryTimeout => Ok((
            HpxState::Failed,
            HpxReasonCode::RecoveryTimeout,
            "Recovery timeout".to_string(),
        )),
        HpxEvent::RelayRouteFound => ok(HpxState::RelayActive, "Relay route activated"),
        HpxEvent::LocalCancel => ok(HpxState::Teardown, "Local cancel during recovery"),
        _ => fail_inv(HpxState::Recovery, event),
    }
}

fn handle_relay_active(event: HpxEvent) -> HandlerResult {
    match event {
        HpxEvent::TransferComplete => ok(HpxState::Teardown, "Relay transfer complete"),
        HpxEvent::TrainingOk => ok(
            HpxState::ActiveTransfer,
            "Relay path confirmed, entering active transfer",
        ),
        HpxEvent::TransferError => Ok((
            HpxState::Recovery,
            HpxReasonCode::RetriesExhausted,
            "Relay transfer error".to_string(),
        )),
        HpxEvent::RelayPolicyFailed => Ok((
            HpxState::Failed,
            HpxReasonCode::RelayPolicyFailed,
            "Relay policy failed".to_string(),
        )),
        HpxEvent::LocalCancel => ok(HpxState::Teardown, "Local cancel during relay transfer"),
        _ => fail_inv(HpxState::RelayActive, event),
    }
}

fn handle_teardown(event: HpxEvent) -> HandlerResult {
    match event {
        HpxEvent::TransferComplete => ok(HpxState::Idle, "Teardown succeeded"),
        HpxEvent::TransferError => Ok((
            HpxState::Failed,
            HpxReasonCode::ManifestVerificationFailed,
            "Teardown failed".to_string(),
        )),
        _ => fail_inv(HpxState::Teardown, event),
    }
}

// ── HpxReactor ────────────────────────────────────────────────────────────────

/// Event-driven reactor managing one or more concurrent HPX sessions.
///
/// Each session is identified by a string key returned from [`create_session`].
/// Events are routed to per-state handler functions; no state mutation happens
/// outside [`dispatch`].
pub struct HpxReactor {
    sessions: HashMap<String, SessionState>,
    next_id: u64,
}

impl Default for HpxReactor {
    fn default() -> Self {
        Self::new()
    }
}

impl HpxReactor {
    /// Maximum consecutive recovery attempts before transitioning to Failed.
    pub const MAX_RECOVERY_ATTEMPTS: u8 = 4;
    /// Maximum ARQ retries per chunk before treating the chunk as unrecoverable.
    pub const MAX_ARQ_RETRIES_PER_CHUNK: u8 = 6;

    /// Create an empty reactor with no sessions.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_id: 0,
        }
    }

    /// Allocate a new session and return its key.
    pub fn create_session(&mut self) -> String {
        let key = format!("s{:08x}", self.next_id);
        self.next_id += 1;
        self.sessions.insert(key.clone(), SessionState::new());
        key
    }

    /// Return the current state of a session.
    pub fn session_state(&self, key: &str) -> Option<HpxState> {
        self.sessions.get(key).map(|s| s.state)
    }

    /// Return the wire-level session identifier assigned at `StartSession`.
    pub fn session_wire_id(&self, key: &str) -> Option<&str> {
        self.sessions.get(key)?.session_id.as_deref()
    }

    /// Return all recorded transitions for a session, in order.
    pub fn transitions(&self, key: &str) -> Option<&[HpxTransition]> {
        self.sessions.get(key).map(|s| s.transitions.as_slice())
    }

    /// Record an ARQ retry for a session.  Returns `Err` when retries are exhausted.
    pub fn record_arq_retry(&mut self, key: &str) -> Result<(), HpxStateError> {
        let s = self
            .sessions
            .get_mut(key)
            .ok_or_else(|| HpxStateError::SessionNotFound(key.to_string()))?;
        s.arq_retries_for_chunk = s.arq_retries_for_chunk.saturating_add(1);
        if s.arq_retries_for_chunk > Self::MAX_ARQ_RETRIES_PER_CHUNK {
            return Err(HpxStateError::InvalidTransition {
                state: s.state,
                event: HpxEvent::TransferError,
            });
        }
        Ok(())
    }

    /// Reset the ARQ retry counter for a session.
    pub fn reset_arq_retry_counter(&mut self, key: &str) {
        if let Some(s) = self.sessions.get_mut(key) {
            s.arq_retries_for_chunk = 0;
        }
    }

    /// Dispatch `event` to the handler for the session's current state.
    ///
    /// All state mutation is performed here after the handler returns; handler
    /// functions are pure (no side effects).
    pub fn dispatch(
        &mut self,
        key: &str,
        event: HpxEvent,
        timestamp_ms: u64,
    ) -> Result<HpxTransition, HpxStateError> {
        let s = self
            .sessions
            .get_mut(key)
            .ok_or_else(|| HpxStateError::SessionNotFound(key.to_string()))?;

        if s.state == HpxState::Failed {
            return Err(HpxStateError::TerminalState);
        }

        let from_state = s.state;

        // Route to the per-state handler.
        let (to_state, reason_code, reason_string) = match from_state {
            HpxState::Idle => handle_idle(event),
            HpxState::Discovery => handle_discovery(event),
            HpxState::Training => handle_training(event),
            HpxState::ActiveTransfer => handle_active_transfer(event),
            HpxState::Recovery => handle_recovery(event),
            HpxState::RelayActive => handle_relay_active(event),
            HpxState::Teardown => handle_teardown(event),
            HpxState::Failed => return Err(HpxStateError::TerminalState),
        }?;

        // ── Post-handler state bookkeeping ────────────────────────────────────

        if matches!(event, HpxEvent::StartSession) && s.session_id.is_none() {
            s.session_id = Some(format!("{:016x}-{:04x}", timestamp_ms, s.transitions.len()));
        }

        if to_state == HpxState::Recovery && from_state != HpxState::Recovery {
            s.recovery_attempts = s.recovery_attempts.saturating_add(1);
            if s.recovery_attempts > Self::MAX_RECOVERY_ATTEMPTS {
                s.state = HpxState::Failed;
                let transition = HpxTransition {
                    timestamp_ms,
                    from_state,
                    to_state: HpxState::Failed,
                    event,
                    reason_code: HpxReasonCode::RecoveryAttemptsExhausted,
                    reason_string: "Recovery attempts exhausted".to_string(),
                    session_id: s.session_id.clone(),
                };
                s.transitions.push(transition.clone());
                return Ok(transition);
            }
        }

        if to_state == HpxState::ActiveTransfer {
            s.arq_retries_for_chunk = 0;
        }

        if to_state == HpxState::Idle {
            s.session_id = None;
            s.recovery_attempts = 0;
            s.arq_retries_for_chunk = 0;
        }

        s.state = to_state;
        let transition = HpxTransition {
            timestamp_ms,
            from_state,
            to_state,
            event,
            reason_code,
            reason_string,
            session_id: s.session_id.clone(),
        };
        s.transitions.push(transition.clone());
        Ok(transition)
    }
}

// ── HpxSession (backward-compat wrapper over HpxReactor) ─────────────────────

/// Single-session wrapper over [`HpxReactor`] with the original `HpxSession` API.
///
/// Maintains full backward compatibility: all callers that held an `HpxSession`
/// continue to work without change.  Internally all state lives in the reactor.
#[derive(Debug, Clone)]
pub struct HpxSession {
    reactor: HpxReactor,
    key: String,
}

impl Default for HpxSession {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for HpxReactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HpxReactor")
            .field("session_count", &self.sessions.len())
            .finish()
    }
}

impl Clone for HpxReactor {
    fn clone(&self) -> Self {
        // Clone the sessions map; each SessionState is re-created from its fields.
        let sessions = self
            .sessions
            .iter()
            .map(|(k, v)| {
                let s = SessionState {
                    state: v.state,
                    session_id: v.session_id.clone(),
                    recovery_attempts: v.recovery_attempts,
                    arq_retries_for_chunk: v.arq_retries_for_chunk,
                    transitions: v.transitions.clone(),
                };
                (k.clone(), s)
            })
            .collect();
        Self {
            sessions,
            next_id: self.next_id,
        }
    }
}

impl HpxSession {
    /// Re-export of `HpxReactor::MAX_RECOVERY_ATTEMPTS` for convenience.
    pub const MAX_RECOVERY_ATTEMPTS: u8 = HpxReactor::MAX_RECOVERY_ATTEMPTS;
    /// Re-export of `HpxReactor::MAX_ARQ_RETRIES_PER_CHUNK` for convenience.
    pub const MAX_ARQ_RETRIES_PER_CHUNK: u8 = HpxReactor::MAX_ARQ_RETRIES_PER_CHUNK;

    /// Create a new single-session facade backed by a fresh `HpxReactor`.
    pub fn new() -> Self {
        let mut reactor = HpxReactor::new();
        let key = reactor.create_session();
        Self { reactor, key }
    }

    /// Return the current HPX state of this session.
    pub fn state(&self) -> HpxState {
        self.reactor
            .session_state(&self.key)
            .unwrap_or(HpxState::Idle)
    }

    /// Return the wire-level session ID assigned at `StartSession`, if set.
    pub fn session_id(&self) -> Option<&str> {
        self.reactor.session_wire_id(&self.key)
    }

    /// Return the ordered list of state transitions recorded for this session.
    pub fn transitions(&self) -> &[HpxTransition] {
        self.reactor.transitions(&self.key).unwrap_or(&[])
    }

    /// Increment the ARQ retry counter; returns `Err` when the limit is exceeded.
    pub fn record_arq_retry(&mut self) -> Result<(), HpxStateError> {
        self.reactor.record_arq_retry(&self.key)
    }

    /// Reset the per-chunk ARQ retry counter after a successful chunk delivery.
    pub fn reset_arq_retry_counter(&mut self) {
        self.reactor.reset_arq_retry_counter(&self.key);
    }

    /// Drive the state machine with `event` at `timestamp_ms`.
    pub fn apply_event(
        &mut self,
        event: HpxEvent,
        timestamp_ms: u64,
    ) -> Result<HpxTransition, HpxStateError> {
        self.reactor.dispatch(&self.key, event, timestamp_ms)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_reaches_idle_again() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::DiscoveryOk, 2).unwrap();
        s.apply_event(HpxEvent::TrainingOk, 3).unwrap();
        s.apply_event(HpxEvent::TransferComplete, 4).unwrap();
        s.apply_event(HpxEvent::TransferComplete, 5).unwrap();
        assert_eq!(s.state(), HpxState::Idle);
    }

    #[test]
    fn discovery_timeout_fails() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::DiscoveryTimeout, 2).unwrap();
        assert_eq!(s.state(), HpxState::Failed);
    }

    #[test]
    fn training_timeout_fails() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::DiscoveryOk, 2).unwrap();
        s.apply_event(HpxEvent::TrainingTimeout, 3).unwrap();
        assert_eq!(s.state(), HpxState::Failed);
    }

    #[test]
    fn signature_rejection_in_discovery_fails() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::SignatureVerificationFailed, 2)
            .unwrap();
        assert_eq!(s.state(), HpxState::Failed);
    }

    #[test]
    fn signature_rejection_in_transfer_enters_recovery() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::DiscoveryOk, 2).unwrap();
        s.apply_event(HpxEvent::TrainingOk, 3).unwrap();
        s.apply_event(HpxEvent::SignatureVerificationFailed, 4)
            .unwrap();
        assert_eq!(s.state(), HpxState::Recovery);
    }

    #[test]
    fn quality_drop_then_recovery_returns_to_transfer() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::DiscoveryOk, 2).unwrap();
        s.apply_event(HpxEvent::TrainingOk, 3).unwrap();
        s.apply_event(HpxEvent::QualityDrop, 4).unwrap();
        s.apply_event(HpxEvent::RecoveryOk, 5).unwrap();
        assert_eq!(s.state(), HpxState::ActiveTransfer);
    }

    #[test]
    fn recovery_exhaustion_fails() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::DiscoveryOk, 2).unwrap();
        s.apply_event(HpxEvent::TrainingOk, 3).unwrap();

        for i in 0..4 {
            s.apply_event(HpxEvent::QualityDrop, 10 + i).unwrap();
            s.apply_event(HpxEvent::RecoveryOk, 20 + i).unwrap();
        }
        s.apply_event(HpxEvent::QualityDrop, 30).unwrap();
        assert_eq!(s.state(), HpxState::Failed);
    }

    #[test]
    fn local_cancel_goes_to_teardown_and_idle() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::DiscoveryOk, 2).unwrap();
        s.apply_event(HpxEvent::LocalCancel, 3).unwrap();
        assert_eq!(s.state(), HpxState::Teardown);
        s.apply_event(HpxEvent::TransferComplete, 4).unwrap();
        assert_eq!(s.state(), HpxState::Idle);
    }

    #[test]
    fn remote_teardown_is_accepted_in_transfer() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::DiscoveryOk, 2).unwrap();
        s.apply_event(HpxEvent::TrainingOk, 3).unwrap();
        s.apply_event(HpxEvent::RemoteTeardown, 4).unwrap();
        assert_eq!(s.state(), HpxState::Teardown);
    }

    #[test]
    fn relay_activation_path_works() {
        let mut s = HpxSession::new();
        s.apply_event(HpxEvent::StartSession, 1).unwrap();
        s.apply_event(HpxEvent::DiscoveryOk, 2).unwrap();
        s.apply_event(HpxEvent::RelayRouteFound, 3).unwrap();
        assert_eq!(s.state(), HpxState::RelayActive);
        s.apply_event(HpxEvent::TransferError, 4).unwrap();
        assert_eq!(s.state(), HpxState::Recovery);
    }

    // ── Reactor-specific tests ─────────────────────────────────────────────────

    #[test]
    fn reactor_two_independent_sessions() {
        let mut reactor = HpxReactor::new();
        let s1 = reactor.create_session();
        let s2 = reactor.create_session();

        // Advance s1 to ActiveTransfer.
        reactor.dispatch(&s1, HpxEvent::StartSession, 1).unwrap();
        reactor.dispatch(&s1, HpxEvent::DiscoveryOk, 2).unwrap();
        reactor.dispatch(&s1, HpxEvent::TrainingOk, 3).unwrap();

        // s2 still Idle.
        assert_eq!(reactor.session_state(&s1), Some(HpxState::ActiveTransfer));
        assert_eq!(reactor.session_state(&s2), Some(HpxState::Idle));
    }

    #[test]
    fn reactor_unknown_session_returns_error() {
        let mut reactor = HpxReactor::new();
        let err = reactor
            .dispatch("nope", HpxEvent::StartSession, 0)
            .unwrap_err();
        assert!(matches!(err, HpxStateError::SessionNotFound(_)));
    }

    #[test]
    fn reactor_terminal_state_rejects_events() {
        let mut reactor = HpxReactor::new();
        let key = reactor.create_session();
        reactor.dispatch(&key, HpxEvent::StartSession, 1).unwrap();
        reactor
            .dispatch(&key, HpxEvent::DiscoveryTimeout, 2)
            .unwrap(); // → Failed
        let err = reactor
            .dispatch(&key, HpxEvent::StartSession, 3)
            .unwrap_err();
        assert_eq!(err, HpxStateError::TerminalState);
    }
}
