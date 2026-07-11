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
pub mod submode;

pub use costas::CostasKind;
pub use submode::{params_for_mode, Submode, SubmodeParams};
