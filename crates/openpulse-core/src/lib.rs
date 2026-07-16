//! OpenPulse – core types, traits and frame format.
//!
//! Every other crate in the workspace depends on this crate.  It intentionally
//! has no heavy dependencies so it can be embedded in plugins without pulling
//! in audio or DSP libraries.

pub mod ack;
pub mod adif;
pub mod audio;
pub mod compression;
pub mod conv;
pub mod cw_id;
pub mod dcd;
pub mod error;
pub mod fec;
pub mod frame;
pub mod handshake;
pub mod hpx;
pub mod iq;
pub mod ldpc;
pub mod len_prefix;
pub mod manifest;
pub mod ota_rate;
pub mod peer_cache;
pub mod peer_descriptor;
pub mod plugin;
pub mod pq_handshake;
pub mod profile;
pub mod query_propagation;
pub mod rate;
pub mod relay;
pub mod remote_control;
pub mod route_discovery;
pub mod sar;
pub mod session_key;
pub mod signed_envelope;
pub mod signing;
pub mod snr_estimate;
pub mod snr_hysteresis;
pub mod soft_viterbi;
pub mod station_id;
pub mod trust;
pub mod trust_store_file;
pub mod turbo;
pub mod tx_metadata;
pub mod wire_query;

pub use ack::*;
pub use audio::*;
pub use compression::*;
pub use dcd::*;
pub use error::*;
pub use fec::*;
pub use frame::*;
pub use handshake::*;
pub use hpx::*;
pub use manifest::*;
pub use peer_cache::*;
pub use peer_descriptor::*;
pub use plugin::*;
pub use pq_handshake::*;
pub use profile::*;
pub use query_propagation::*;
pub use rate::*;
pub use relay::*;
pub use route_discovery::*;
pub use sar::*;
pub use signed_envelope::*;
pub use signing::*;
pub use trust::*;
pub use turbo::*;
pub use tx_metadata::*;
pub use wire_query::*;
