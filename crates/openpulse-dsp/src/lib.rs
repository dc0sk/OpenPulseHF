//! DSP primitives for OpenPulseHF: FIR filter, RRC coefficient generation,
//! Gardner timing recovery, Costas carrier PLL, preamble synchronization,
//! carrier-phase-insensitive acquisition, Doppler tracking, and LMS/DFE
//! equalizer.

pub mod acquisition;
pub mod constellation;
pub mod doppler_tracker;
pub mod equalizer;
pub mod farrow;
pub mod filter;
pub mod pll;
pub mod preamble;
pub mod rrc;
pub mod timing;
