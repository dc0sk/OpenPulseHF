//! Shared bandplan guardrails for non-QSY CLI commands.

use anyhow::Result;
use openpulse_config as config;
use openpulse_qsy::{BandplanMode, BandplanPolicy};
use openpulse_radio::RigctldController;
use tracing::warn;

/// Validate the current rig frequency and operating mode against configured bandplan policy.
///
/// If policy enforcement is enabled but rig frequency cannot be read, this emits a warning and
/// allows the command to continue to preserve loopback/simulated workflows.
pub fn enforce_mode_guardrails(mode: &str) -> Result<()> {
    let cfg = config::load()?;
    let qsy_cfg = &cfg.qsy;

    if !qsy_cfg.bandplan_awareness_enabled {
        warn!(
            "bandplan-awareness override active for operating mode guardrails (qsy.bandplan_awareness_enabled=false)"
        );
        return Ok(());
    }

    let policy = BandplanPolicy {
        awareness_enabled: qsy_cfg.bandplan_awareness_enabled,
        mode: qsy_cfg
            .bandplan_mode
            .parse::<BandplanMode>()
            .map_err(|_| anyhow::anyhow!("invalid qsy.bandplan_mode: {}", qsy_cfg.bandplan_mode))?,
        enforce_max_channel_width: qsy_cfg.enforce_max_channel_width,
        enforce_segment_conventions: qsy_cfg.enforce_segment_conventions,
    };

    let mut rig = match RigctldController::connect(&cfg.radio.rigctld_addr) {
        Ok(rig) => rig,
        Err(err) => {
            warn!(
                "bandplan awareness enabled but rig frequency unavailable via rigctld at {}: {}; skipping operating-mode guardrail check",
                cfg.radio.rigctld_addr,
                err
            );
            return Ok(());
        }
    };

    let freq_hz = match rig.get_frequency() {
        Ok(freq_hz) => freq_hz,
        Err(err) => {
            warn!(
                "bandplan awareness enabled but rig frequency read failed from {}: {}; skipping operating-mode guardrail check",
                cfg.radio.rigctld_addr,
                err
            );
            return Ok(());
        }
    };

    policy.validate_frequency(freq_hz, mode).map_err(|err| {
        anyhow::anyhow!(
            "bandplan guardrail rejected mode {} at {} Hz: {}",
            mode,
            freq_hz,
            err
        )
    })
}

#[cfg(test)]
mod tests {
    use openpulse_qsy::{BandplanMode, BandplanPolicy};

    #[test]
    fn segment_and_width_enforced_by_policy() {
        let policy = BandplanPolicy {
            awareness_enabled: true,
            mode: BandplanMode::HamIaru,
            enforce_max_channel_width: true,
            enforce_segment_conventions: true,
        };

        assert!(policy.validate_frequency(14_074_000, "BPSK250").is_ok());
        assert!(policy.validate_frequency(14_200_000, "BPSK250").is_err());
    }

    #[test]
    fn segment_can_be_relaxed_while_awareness_stays_enabled() {
        let policy = BandplanPolicy {
            awareness_enabled: true,
            mode: BandplanMode::HamIaru,
            enforce_max_channel_width: true,
            enforce_segment_conventions: false,
        };

        assert!(policy.validate_frequency(14_200_000, "BPSK250").is_ok());
    }
}
