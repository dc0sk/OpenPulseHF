//! Pilot-framed waveform plugin for OpenPulse.
//!
//! A new modem waveform whose frames carry **known in-band pilot symbols** at a
//! fixed cadence, so the receiver recovers the carrier with a
//! [`openpulse_dsp::pilot_tracker::PilotTracker`] driven only by known symbols.
//! That data-aided loop is immune to the decision-directed cycle slips that limit
//! the existing preamble-only single-Costas modes on dense constellations and
//! through carrier offset — the convergent lesson from the qo100 / liquid-dsp /
//! gnuradio references.
//!
//! Bring-up order (see `docs/dev/reference-mining-plan.md`, Tier 2):
//! 1. **`frame` — the symbol-level pilot-framed codec (this module).** Maps bytes
//!    to a preamble-plus-pilot-interleaved QPSK symbol stream and recovers them
//!    through a carrier offset using the preamble-seeded, pilot-tracked loop.
//! 2. *(next)* the passband audio `ModulationPlugin` impl (pulse shaping,
//!    up/down-conversion, symbol timing) wrapping the `frame` codec, then engine
//!    and profile integration.

pub mod frame;

pub use frame::PilotFrame;
