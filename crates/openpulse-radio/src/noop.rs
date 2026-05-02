use crate::{PttController, PttError};

/// No-op PTT controller for loopback and testing.
#[derive(Debug, Default)]
pub struct NoOpPtt {
    asserted: bool,
}

impl NoOpPtt {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PttController for NoOpPtt {
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
    fn noop_assert_release_round_trip_under_50ms() {
        let mut ptt = NoOpPtt::new();
        let start = Instant::now();
        ptt.assert_ptt().expect("assert");
        assert!(ptt.is_asserted());
        ptt.release_ptt().expect("release");
        assert!(!ptt.is_asserted());
        assert!(
            start.elapsed().as_millis() < 50,
            "PTT round-trip exceeded 50 ms"
        );
    }

    #[test]
    fn noop_starts_released() {
        let ptt = NoOpPtt::new();
        assert!(!ptt.is_asserted());
    }

    #[test]
    fn noop_double_assert_is_idempotent() {
        let mut ptt = NoOpPtt::new();
        ptt.assert_ptt().expect("first assert");
        ptt.assert_ptt().expect("second assert");
        assert!(ptt.is_asserted());
        ptt.release_ptt().expect("release");
        assert!(!ptt.is_asserted());
    }
}
