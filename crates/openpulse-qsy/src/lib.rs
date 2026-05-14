//! QSY frequency-agility protocol for OpenPulseHF.
//!
//! Provides frame codec, negotiation state machine, and rig frequency scanner for
//! collaborative channel-switching between two stations.

pub mod bandplan;
pub mod frame;
pub mod scanner;
pub mod session;

pub use bandplan::{BandplanError, BandplanMode, BandplanPolicy};
pub use frame::{QsyFrame, QsyFrameError};
pub use openpulse_core::trust::ConnectionTrustLevel;
pub use scanner::{QsyScanner, QsyScannerError};
pub use session::{QsyAction, QsyError, QsyPolicy, QsySession};
