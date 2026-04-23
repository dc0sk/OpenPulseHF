//! OpenPulse – core types, traits and frame format.
//!
//! Every other crate in the workspace depends on this crate.  It intentionally
//! has no heavy dependencies so it can be embedded in plugins without pulling
//! in audio or DSP libraries.

pub mod audio;
pub mod error;
pub mod frame;
pub mod plugin;

pub use audio::*;
pub use error::*;
pub use frame::*;
pub use plugin::*;
