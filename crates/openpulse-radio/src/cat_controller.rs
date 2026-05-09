//! `CatController` trait for full rig CAT control.

use crate::error::RadioError;
use crate::rig_mode::RigMode;

/// Full CAT rig control (frequency, mode).
///
/// Implemented by [`crate::RigctldController`] (via hamlib TCP) and
/// [`crate::generic_cat::GenericSerialCat`] (via TOML-scripted serial).
/// Methods return [`RadioError::Unsupported`] when the backend or rig
/// definition omits the relevant command.
pub trait CatController {
    fn set_frequency(&mut self, hz: u64) -> Result<(), RadioError>;
    fn get_frequency(&mut self) -> Result<u64, RadioError>;
    fn set_mode(&mut self, mode: &RigMode) -> Result<(), RadioError>;
}
