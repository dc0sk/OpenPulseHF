//! DSP primitives for OpenPulseHF: FIR filter, RRC coefficient generation,
//! Gardner timing recovery, Costas carrier PLL, pilot-aided carrier tracking,
//! preamble synchronization, carrier-phase-insensitive acquisition, Doppler
//! tracking, automatic gain control, and LMS/DFE equalizer.

pub mod acquisition;
pub mod agc;
pub mod cessb;
pub mod constellation;
pub mod doppler_tracker;
pub mod equalizer;
pub mod farrow;
pub mod filter;
pub mod freq_acquire;
pub mod pilot_tracker;
pub mod pll;
pub mod preamble;
pub mod rrc;
pub mod timing;
