//! FreeDV authenticated voice shim for OpenPulseHF.
//!
//! Sends Ed25519-signed authentication beacons via the FreeDV Qt-GUI UDP data
//! port (`127.0.0.1:10001` by default) so listeners can verify the transmitting
//! station's identity without modifying FreeDV.
//!
//! # Components
//!
//! - [`beacon`]: [`AuthBeacon`] — signed identity beacon, encode/decode, sign/verify.
//! - [`data_port`]: [`FreeDvDataPort`] — async UDP send/receive to the FreeDV data port.
//! - [`verdict`]: [`TrustVerdict`] — authentication result; [`VerdictServer`] exposes
//!   the current verdict on a Unix socket for companion UI polling.
//! - [`scheduler`]: [`BeaconScheduler`] — fires a beacon at a configurable interval.

pub mod beacon;
pub mod data_port;
pub mod scheduler;
pub mod verdict;
