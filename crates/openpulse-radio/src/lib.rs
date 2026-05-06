/// PTT and full CAT rig controller traits and implementations for OpenPulseHF.
pub mod error;
pub mod noop;
pub mod rig_controller;
pub mod rig_mode;
pub mod rigctld;
pub mod serial;
pub mod vox;

pub use error::{PttError, RadioError};
pub use noop::NoOpPtt;
pub use rig_controller::RigctldController;
pub use rig_mode::RigMode;
pub use rigctld::RigctldPtt;
pub use vox::VoxPtt;

/// Controls transmitter PTT (push-to-talk) state.
pub trait PttController {
    fn assert_ptt(&mut self) -> Result<(), PttError>;
    fn release_ptt(&mut self) -> Result<(), PttError>;
    fn is_asserted(&self) -> bool;
}
