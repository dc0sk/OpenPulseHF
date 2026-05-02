use crate::{PttController, PttError};

/// VOX (voice-operated) PTT controller.
///
/// In VOX mode the transmitter is keyed automatically when audio energy exceeds
/// a threshold. This implementation tracks software state only — the actual
/// VOX trigger is handled by the transceiver hardware or a downstream audio
/// pipeline. Callers that integrate with `ModemEngine` should enable audio
/// output before calling `assert_ptt` and stop it after `release_ptt`.
#[derive(Debug, Default)]
pub struct VoxPtt {
    asserted: bool,
}

impl VoxPtt {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PttController for VoxPtt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        self.asserted = true;
        Ok(())
    }

    fn release_ptt(&mut self) -> Result<(), PttError> {
        self.asserted = false;
        Ok(())
    }

    fn is_asserted(&self) -> bool {
        self.asserted
    }
}
