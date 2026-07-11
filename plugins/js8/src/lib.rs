//! JS8-compatible modulation (FF-15).
//!
//! JS8 is FT8's physical layer — 8-GFSK, 79 symbols (3×7 Costas sync + 58 data), LDPC(174,87) —
//! with a distinct message layer. This crate ports the waveform bit-/tone-exactly so an OpenPulse
//! station can be discovered by, and discover, stock JS8Call stations (design:
//! `docs/dev/design/js8-discovery-rendezvous-plan.md`).
//!
//! Phase A builds the transmit core. This first unit is the self-contained protocol foundation: the
//! submode table ([`submode`]) and the Costas sync arrays ([`costas`]) — both verified against
//! JS8Call-improved (plan §2.1/§2.2, Appendix B). Frame packing, LDPC, GFSK modulation, and the
//! multi-decode receiver land in the following units.

pub mod costas;
pub mod crc;
pub mod decoder;
pub mod demodulate;
pub mod encode;
pub mod frame;
pub mod grammar;
pub mod jsc;
pub mod ldpc174;
pub mod message;
pub mod modulate;
pub mod plugin;
pub mod submode;
pub mod sync;
pub mod tones;
pub mod varicode;

pub use costas::CostasKind;
pub use crc::augmented_crc12;
pub use decoder::{decode_window, DecodeCfg, Js8Decode};
pub use demodulate::demodulate_soft;
pub use encode::{pack_alphanumeric50, pack_compound_frame, pack_heartbeat_frame};
pub use frame::{pack_callsign, pack_grid, unpack_callsign, unpack_grid};
pub use grammar::{
    parse_heartbeat, unpack_compound_frame, unpack_directed_message, CompoundFrame,
    DirectedMessage, FrameType, Heartbeat,
};
pub use jsc::jsc_decompress;
pub use ldpc174::{bp_decode, BpDecode};
pub use message::{js8_info_bits, js8_message_crc12};
pub use modulate::{modulate_tones, GfskParams};
pub use plugin::Js8Plugin;
pub use submode::{params_for_mode, Submode, SubmodeParams};
pub use sync::{find_sync, SyncCandidate};
pub use tones::{codeword_to_tones, message_to_tones};
pub use varicode::{huff_decode, unpack_data_message, HUFF_TABLE};
