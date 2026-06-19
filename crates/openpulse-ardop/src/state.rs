/// TNC connection state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum TncState {
    /// No connection; also used while LISTEN is inactive.
    Disc,
    /// Listening for incoming connections.
    Listen,
    /// Outgoing connection attempt in progress.
    Connecting { peer: String },
    /// Session established.
    Connected { peer: String },
    /// Teardown in progress.
    Disconnecting,
}

impl TncState {
    /// ARDOP state label used in STATE and NEWSTATE responses.
    pub fn label(&self) -> String {
        match self {
            TncState::Disc | TncState::Listen => "DISC".into(),
            TncState::Connecting { .. } => "CONNECTING".into(),
            TncState::Connected { peer } => format!("CONNECTED {peer}"),
            TncState::Disconnecting => "DISCONNECTING".into(),
        }
    }
}
