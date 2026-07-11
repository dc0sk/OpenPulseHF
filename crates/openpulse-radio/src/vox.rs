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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn vox_assert_release_tracks_state_under_50ms() {
        let mut ptt = VoxPtt::new();
        assert!(!ptt.is_asserted());
        let t = Instant::now();
        ptt.assert_ptt().unwrap();
        assert!(ptt.is_asserted());
        ptt.release_ptt().unwrap();
        assert!(!ptt.is_asserted());
        assert!(
            t.elapsed().as_millis() <= 50,
            "VOX PTT toggle must be ≤50 ms"
        );
    }
}
