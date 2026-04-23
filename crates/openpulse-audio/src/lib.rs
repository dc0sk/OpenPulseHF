//! Audio backend implementations for OpenPulse.
//!
//! Two backends are provided:
//!
//! * [`LoopbackBackend`] – pure in-memory loopback, ideal for testing.
//! * [`CpalBackend`] – cross-platform audio via the `cpal` crate (feature
//!   `cpal-backend`, enabled by default).  Supports ALSA, PipeWire, CoreAudio
//!   and WASAPI depending on the platform.

pub mod loopback;

#[cfg(feature = "cpal-backend")]
pub mod cpal_backend;

pub use loopback::LoopbackBackend;

#[cfg(feature = "cpal-backend")]
pub use cpal_backend::CpalBackend;
