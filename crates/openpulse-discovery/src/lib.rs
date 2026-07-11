//! JS8-based station discovery (FF-15): pure, no-I/O protocol logic driven by the daemon.
//!
//! The first piece is the [`hint`] codec — the in-band `@OPULSE` capability marker that lets one
//! OpenPulse station recognise another among ordinary JS8 traffic. The station table, wall-clock T/R
//! scheduler, and the discovery/rendezvous state machines land in the following units (plan §4).

pub mod hint;
pub mod peer_map;
pub mod scheduler;
pub mod station;

pub use hint::{decode_hint, encode_hint, HintPayload, HINT_MAGIC, OPULSE_GROUP};
pub use peer_map::{station_to_peer_record, CAP_HPX, CAP_PQ, CAP_QSY, CAP_RELAY, CAP_RENDEZVOUS};
pub use scheduler::{Js8Clock, SlotTracker};
pub use station::{Js8Station, Observation, OphfHint, QueryBackoff, StationTable};
