use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HpxStateError {
    InvalidTransition { state: HpxState, event: HpxEvent },
    TerminalState,
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
        }
    }
}

#[derive(Debug, Clone)]
pub struct HpxSession {
    state: HpxState,
    session_id: Option<String>,
    recovery_attempts: u8,
    arq_retries_for_chunk: u8,
    transitions: Vec<HpxTransition>,
}

impl Default for HpxSession {
    fn default() -> Self {
        Self::new()
    }
}

impl HpxSession {
    pub const MAX_RECOVERY_ATTEMPTS: u8 = 4;
    pub const MAX_ARQ_RETRIES_PER_CHUNK: u8 = 6;

    pub fn new() -> Self {
        Self {
            state: HpxState::Idle,
            session_id: None,
            recovery_attempts: 0,
            arq_retries_for_chunk: 0,
            transitions: Vec::new(),
        }
    }

    pub fn state(&self) -> HpxState {
        self.state
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn transitions(&self) -> &[HpxTransition] {
        &self.transitions
    }

    pub fn record_arq_retry(&mut self) -> Result<(), HpxStateError> {
        self.arq_retries_for_chunk = self.arq_retries_for_chunk.saturating_add(1);
        if self.arq_retries_for_chunk > Self::MAX_ARQ_RETRIES_PER_CHUNK {
            return Err(HpxStateError::InvalidTransition {
                state: self.state,
                event: HpxEvent::TransferError,
            });
        }
        Ok(())
    }

    pub fn reset_arq_retry_counter(&mut self) {
        self.arq_retries_for_chunk = 0;
    }

    pub fn apply_event(
        &mut self,
        event: HpxEvent,
        timestamp_ms: u64,
    ) -> Result<HpxTransition, HpxStateError> {
        if self.state == HpxState::Failed {
            return Err(HpxStateError::TerminalState);
        }

        let from_state = self.state;
        let (to_state, reason_code, reason_string) = self.resolve_transition(event)?;

        if matches!(event, HpxEvent::StartSession) && self.session_id.is_none() {
            self.session_id = Some(format!(
                "{:016x}-{:04x}",
                timestamp_ms,
                self.transitions.len()
            ));
        }

        if to_state == HpxState::Recovery && from_state != HpxState::Recovery {
            self.recovery_attempts = self.recovery_attempts.saturating_add(1);
            if self.recovery_attempts > Self::MAX_RECOVERY_ATTEMPTS {
                self.state = HpxState::Failed;
                let transition = HpxTransition {
                    timestamp_ms,
                    from_state,
                    to_state: HpxState::Failed,
                    event,
                    reason_code: HpxReasonCode::RecoveryAttemptsExhausted,
                    reason_string: "Recovery attempts exhausted".to_string(),
                    session_id: self.session_id.clone(),
                };
                self.transitions.push(transition.clone());
                return Ok(transition);
            }
        }

        if to_state == HpxState::ActiveTransfer {
            self.reset_arq_retry_counter();
        }

        if to_state == HpxState::Idle {
            self.session_id = None;
            self.recovery_attempts = 0;
            self.reset_arq_retry_counter();
        }

        self.state = to_state;
        let transition = HpxTransition {
            timestamp_ms,
            from_state,
            to_state,
            event,
            reason_code,
            reason_string,
            session_id: self.session_id.clone(),
        };
        self.transitions.push(transition.clone());
        Ok(transition)
    }

    fn resolve_transition(
        &self,
        event: HpxEvent,
    ) -> Result<(HpxState, HpxReasonCode, String), HpxStateError> {
        let result = match (self.state, event) {
            (HpxState::Idle, HpxEvent::StartSession) => (
                HpxState::Discovery,
                HpxReasonCode::Success,
                "Session started".to_string(),
            ),

            (HpxState::Discovery, HpxEvent::DiscoveryOk) => (
                HpxState::Training,
                HpxReasonCode::Success,
                "Peer discovered and verified".to_string(),
            ),
            (HpxState::Discovery, HpxEvent::DiscoveryTimeout) => (
                HpxState::Failed,
                HpxReasonCode::Timeout,
                "Discovery timeout".to_string(),
            ),
            (HpxState::Discovery, HpxEvent::SignatureVerificationFailed) => (
                HpxState::Failed,
                HpxReasonCode::SignatureFailure,
                "Discovery signature verification failed".to_string(),
            ),
            (HpxState::Discovery, HpxEvent::LocalCancel) => (
                HpxState::Teardown,
                HpxReasonCode::Success,
                "Local cancel during discovery".to_string(),
            ),

            (HpxState::Training, HpxEvent::TrainingOk) => (
                HpxState::ActiveTransfer,
                HpxReasonCode::Success,
                "Training complete".to_string(),
            ),
            (HpxState::Training, HpxEvent::RelayRouteFound) => (
                HpxState::RelayActive,
                HpxReasonCode::Success,
                "Relay route activated".to_string(),
            ),
            (HpxState::Training, HpxEvent::TrainingTimeout) => (
                HpxState::Failed,
                HpxReasonCode::Timeout,
                "Training timeout".to_string(),
            ),
            (HpxState::Training, HpxEvent::SignatureVerificationFailed) => (
                HpxState::Failed,
                HpxReasonCode::SignatureFailure,
                "Training authentication failed".to_string(),
            ),
            (HpxState::Training, HpxEvent::LocalCancel) => (
                HpxState::Teardown,
                HpxReasonCode::Success,
                "Local cancel during training".to_string(),
            ),

            (HpxState::ActiveTransfer, HpxEvent::TransferComplete) => (
                HpxState::Teardown,
                HpxReasonCode::Success,
                "Transfer complete".to_string(),
            ),
            (HpxState::ActiveTransfer, HpxEvent::TransferError) => (
                HpxState::Recovery,
                HpxReasonCode::RetriesExhausted,
                "Transfer error, entering recovery".to_string(),
            ),
            (HpxState::ActiveTransfer, HpxEvent::QualityDrop) => (
                HpxState::Recovery,
                HpxReasonCode::QualityDrop,
                "Quality drop, entering recovery".to_string(),
            ),
            (HpxState::ActiveTransfer, HpxEvent::RelayRouteFound) => (
                HpxState::RelayActive,
                HpxReasonCode::Success,
                "Relay route activated".to_string(),
            ),
            (HpxState::ActiveTransfer, HpxEvent::SignatureVerificationFailed) => (
                HpxState::Recovery,
                HpxReasonCode::SignatureFailure,
                "Frame authentication failed".to_string(),
            ),
            (HpxState::ActiveTransfer, HpxEvent::LocalCancel) => (
                HpxState::Teardown,
                HpxReasonCode::Success,
                "Local cancel during transfer".to_string(),
            ),
            (HpxState::ActiveTransfer, HpxEvent::RemoteTeardown) => (
                HpxState::Teardown,
                HpxReasonCode::Success,
                "Remote teardown requested".to_string(),
            ),

            (HpxState::Recovery, HpxEvent::RecoveryOk) => (
                HpxState::ActiveTransfer,
                HpxReasonCode::Success,
                "Recovery complete".to_string(),
            ),
            (HpxState::Recovery, HpxEvent::RecoveryTimeout) => (
                HpxState::Failed,
                HpxReasonCode::RecoveryTimeout,
                "Recovery timeout".to_string(),
            ),
            (HpxState::Recovery, HpxEvent::RelayRouteFound) => (
                HpxState::RelayActive,
                HpxReasonCode::Success,
                "Relay route activated".to_string(),
            ),
            (HpxState::Recovery, HpxEvent::LocalCancel) => (
                HpxState::Teardown,
                HpxReasonCode::Success,
                "Local cancel during recovery".to_string(),
            ),

            (HpxState::RelayActive, HpxEvent::TransferComplete) => (
                HpxState::Teardown,
                HpxReasonCode::Success,
                "Relay transfer complete".to_string(),
            ),
            (HpxState::RelayActive, HpxEvent::TrainingOk) => (
                HpxState::ActiveTransfer,
                HpxReasonCode::Success,
                "Relay path confirmed, entering active transfer".to_string(),
            ),
            (HpxState::RelayActive, HpxEvent::TransferError) => (
                HpxState::Recovery,
                HpxReasonCode::RetriesExhausted,
                "Relay transfer error".to_string(),
            ),
            (HpxState::RelayActive, HpxEvent::RelayPolicyFailed) => (
                HpxState::Failed,
                HpxReasonCode::RelayPolicyFailed,
                "Relay policy failed".to_string(),
            ),
            (HpxState::RelayActive, HpxEvent::LocalCancel) => (
                HpxState::Teardown,
                HpxReasonCode::Success,
                "Local cancel during relay transfer".to_string(),
            ),

            (HpxState::Teardown, HpxEvent::TransferComplete) => (
                HpxState::Idle,
                HpxReasonCode::Success,
                "Teardown succeeded".to_string(),
            ),
            (HpxState::Teardown, HpxEvent::TransferError) => (
                HpxState::Failed,
                HpxReasonCode::ManifestVerificationFailed,
                "Teardown failed".to_string(),
            ),

            _ => {
                return Err(HpxStateError::InvalidTransition {
                    state: self.state,
                    event,
                });
            }
        };

        Ok(result)
    }
}

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
}
