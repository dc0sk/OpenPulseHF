//! OpenPulse modem engine.
//!
//! The [`ModemEngine`] ties a [`PluginRegistry`] and an [`AudioBackend`]
//! together to provide simple `transmit` and `receive` operations.

pub mod benchmark;
pub mod diagnostics;
pub mod engine;
pub mod envelope_codec;
pub mod pipeline;

pub use engine::ModemEngine;
