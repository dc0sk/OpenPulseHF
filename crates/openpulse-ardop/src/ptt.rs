//! The TNC's PTT controller, built from `[modem]` the way the daemon builds its own (#1285).

use openpulse_radio::ptt_builder::{self, OwnedPttSpec, PttSpec, RetryingPtt};
use openpulse_radio::{PttController, PttError};

/// Build the TNC's PTT controller: a config error is returned, an unreachable backend refuses keys.
///
/// The TNC used to fall back to `NoOpPtt` on any build error, and `NoOpPtt` keys successfully, so a
/// mistyped backend or a rigctld that was not up yet made the TNC play audio into an unkeyed rig.
/// A config error cannot self-heal, so the caller refuses to start. A real backend that is merely
/// unreachable gets the daemon's `RetryingPtt`, which refuses every key until it reconnects; the
/// bridge's `keyed_transmit` then skips the emission and raises a `FAULT`.
pub fn ptt_from_spec(spec: &PttSpec<'_>) -> Result<Box<dyn PttController + Send>, PttError> {
    match ptt_builder::build_ptt(spec) {
        Ok(Some(ctrl)) => {
            tracing::info!(backend = %spec.backend, "PTT controller ready");
            Ok(ctrl)
        }
        // `"none"` (or empty): the operator keys by VOX or by hand, so the TNC must still key.
        Ok(None) => Ok(ptt_builder::no_ptt()),
        Err(e @ PttError::Config(_)) => Err(e),
        Err(e) => {
            tracing::warn!(
                backend = %spec.backend,
                error = %e,
                "PTT backend unreachable; every emission will be REFUSED until it reconnects"
            );
            Ok(Box::new(RetryingPtt::new(OwnedPttSpec {
                backend: spec.backend.to_string(),
                rigctld_addr: spec.rigctld_addr.to_string(),
                device: spec.device.to_string(),
                gpio_pin: spec.gpio_pin,
            })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_radio::SharedPtt;
    use std::time::Duration;

    fn spec<'a>(backend: &'a str, rigctld_addr: &'a str) -> PttSpec<'a> {
        PttSpec {
            backend,
            rigctld_addr,
            device: "",
            gpio_pin: 3,
        }
    }

    fn keys(ctrl: Box<dyn PttController + Send>) -> bool {
        SharedPtt::new(Some(ctrl), Duration::from_secs(180))
            .keyed(None)
            .is_ok()
    }

    /// The #1285 defect in the TNC: an unreachable backend became `NoOpPtt`, which keys.
    #[test]
    fn an_unreachable_backend_refuses_the_key() {
        // Port 1 on loopback: reserved, nothing listens, connect fails fast.
        let ctrl = ptt_from_spec(&spec("rigctld", "127.0.0.1:1"))
            .expect("an unreachable backend must still start the TNC");
        assert!(
            !keys(ctrl),
            "an unreachable PTT backend must REFUSE the key, so nothing is radiated unkeyed"
        );
    }

    /// The control: `"none"` means VOX or a hand key, and must still key.
    #[test]
    fn no_ptt_configured_still_keys() {
        for backend in ["none", ""] {
            let ctrl = ptt_from_spec(&spec(backend, "127.0.0.1:1")).expect("none builds");
            assert!(keys(ctrl), "ptt_backend = {backend:?} must key");
        }
    }

    /// A typo cannot self-heal: the caller refuses to start.
    #[test]
    fn a_config_error_is_returned_not_swallowed() {
        assert!(matches!(
            ptt_from_spec(&spec("rigctl", "127.0.0.1:4532")),
            Err(PttError::Config(_))
        ));
    }
}
