//! OpenPulse modem engine.
//!
//! The [`ModemEngine`] ties a [`PluginRegistry`] and an [`AudioBackend`]
//! together to provide simple `transmit` and `receive` operations.

pub mod engine;

pub use engine::ModemEngine;
