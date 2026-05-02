/// PTT controller trait and implementations for OpenPulseHF.
pub mod error;
pub mod noop;
pub mod rigctld;
pub mod serial;
pub mod vox;

pub use error::PttError;
pub use noop::NoOpPtt;
pub use rigctld::RigctldPtt;
pub use vox::VoxPtt;

/// Controls transmitter PTT (push-to-talk) state.
pub trait PttController {
    fn assert_ptt(&mut self) -> Result<(), PttError>;
    fn release_ptt(&mut self) -> Result<(), PttError>;
    fn is_asserted(&self) -> bool;
}
