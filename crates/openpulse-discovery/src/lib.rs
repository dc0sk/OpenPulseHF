//! JS8-based station discovery (FF-15): pure, no-I/O protocol logic driven by the daemon.
//!
//! The first piece is the [`hint`] codec — the in-band `@OPULSE` capability marker that lets one
//! OpenPulse station recognise another among ordinary JS8 traffic. The station table, wall-clock T/R
//! scheduler, and the discovery/rendezvous state machines land in the following units (plan §4).

/// The `@OPULSE` JS8 dialect these codecs speak: one magic and ONE version for the whole dialect.
///
/// Defined here, not in `hint.rs`, because the hint and the rendezvous codec both emit it (#1163).
/// A receiver's real question is "do I speak OPHF-dialect v1" — one namespace, one answer. Separate
/// per-codec constants would make "OPHF1 hints talking to OPHF2 rendezvous" representable, and
/// nothing would catch it; sharing one definition makes that state unrepresentable instead.
pub mod dialect {
    /// Magic prefix; the full wire token is `OPHF<version>`.
    pub const MAGIC: &str = "OPHF";
    /// Version this build emits and accepts, for every codec in the dialect.
    pub const VERSION: u8 = 1;
}

pub mod discovery_sm;
pub mod hint;
pub mod hint_assembler;
pub mod peer_map;
pub mod rendezvous;
pub mod rendezvous_assembler;
pub mod runtime;
pub mod scheduler;
pub mod station;

pub use discovery_sm::{DiscoveryAction, DiscoveryEvent, DiscoverySm, DiscoveryState};
pub use hint::{decode_hint, encode_hint, HintPayload, HINT_MAGIC, OPULSE_GROUP};
pub use hint_assembler::{HintAssembler, RecognizedHint};
pub use js8_plugin::submode::Submode;
pub use peer_map::{station_to_peer_record, CAP_HPX, CAP_PQ, CAP_QSY, CAP_RELAY, CAP_RENDEZVOUS};
pub use rendezvous::{
    respond as rendezvous_respond, RejectReason, RendezvousInitiator, RendezvousMsg,
    RendezvousOutcome, DEFAULT_SWITCH_SLOTS,
};
pub use rendezvous_assembler::{RecognizedRendezvous, RendezvousAssembler};
pub use runtime::{DiscoveryOutcome, DiscoveryParams, DiscoveryRuntime, TxMode};
pub use scheduler::{Js8Clock, SlotTracker};
pub use station::{Js8Station, Observation, OphfHint, QueryBackoff, StationTable};
