//! HARQ policy selection for Item 6.
//!
//! This module provides a deterministic mapping from measured channel quality
//! to retry FEC strategy and ACK timeout.

use openpulse_core::fec::FecMode;

/// HARQ decision for one transmit attempt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HarqDecision {
    /// FEC mode selected for this attempt.
    pub fec_mode: FecMode,
    /// Effective code rate (k/n) of the selected mode.
    pub code_rate: f32,
    /// ACK timeout in milliseconds for this attempt.
    pub ack_timeout_ms: u16,
}

/// Deterministic HARQ policy for SNR/fading-aware retry selection.
#[derive(Debug, Clone, Copy)]
pub struct HarqPolicy {
    /// Lower SNR threshold below which strong RS is used immediately.
    pub strong_snr_floor_db: f32,
    /// Mid SNR threshold where soft-concatenated becomes unnecessary.
    pub soft_snr_floor_db: f32,
    /// Fading depth threshold forcing stronger coding.
    pub fading_strong_db: f32,
    /// Fading depth threshold for soft-concatenated selection.
    pub fading_soft_db: f32,
    /// SNR floor (dB) above which a soft-capable rung uses high-rate LDPC
    /// (rate ≈8/9) on the first attempt instead of RS.  Sits above
    /// `soft_snr_floor_db` so only genuinely strong channels trade redundancy
    /// for throughput.
    pub high_rate_ldpc_floor_db: f32,
    /// Whether the active mode produces genuine soft LLRs.  Set per mode by the
    /// engine; high-rate LDPC is selected only when `true`, since a
    /// hard-decision plugin gains nothing from a soft code.
    pub soft_capable: bool,
}

impl Default for HarqPolicy {
    fn default() -> Self {
        Self {
            strong_snr_floor_db: 14.0,
            soft_snr_floor_db: 21.0,
            fading_strong_db: 9.0,
            fading_soft_db: 7.0,
            high_rate_ldpc_floor_db: 26.0,
            soft_capable: false,
        }
    }
}

impl HarqPolicy {
    /// Return a copy with `soft_capable` set — used by the engine to specialise
    /// the default policy to the active mode's demodulator capability.
    pub fn with_soft_capable(mut self, soft_capable: bool) -> Self {
        self.soft_capable = soft_capable;
        self
    }

    /// Select FEC mode and ACK timeout from channel state and retry index.
    ///
    /// `retry_index`: 0 for first attempt, 1+ for retransmissions.
    pub fn select(&self, snr_db: f32, fading_depth_db: f32, retry_index: u8) -> HarqDecision {
        // Highest tier: on the first attempt, a soft-capable rung on a strong,
        // low-fade channel uses high-rate LDPC for throughput.  A failed attempt
        // (retry ≥ 1) drops back to the protective ladder below.
        let high_rate_ldpc = self.soft_capable
            && retry_index == 0
            && fading_depth_db < self.fading_soft_db
            && snr_db >= self.high_rate_ldpc_floor_db;

        // Escalate coding strength across retries to improve delivery probability.
        let base_mode = if high_rate_ldpc {
            FecMode::LdpcHighRate
        } else if snr_db < self.strong_snr_floor_db || fading_depth_db >= self.fading_strong_db {
            FecMode::RsStrong
        } else if snr_db < self.soft_snr_floor_db || fading_depth_db >= self.fading_soft_db {
            FecMode::SoftConcatenated
        } else {
            FecMode::Rs
        };

        let fec_mode = match retry_index {
            0 => base_mode,
            1 => match base_mode {
                FecMode::Rs => FecMode::RsStrong,
                other => other,
            },
            _ => FecMode::SoftConcatenated,
        };

        HarqDecision {
            fec_mode,
            code_rate: code_rate_for_fec(fec_mode),
            ack_timeout_ms: ack_timeout_ms_for_snr(snr_db),
        }
    }
}

/// Return nominal code rate for each FEC mode.
fn code_rate_for_fec(fec: FecMode) -> f32 {
    match fec {
        FecMode::None => 1.0,
        FecMode::Rs => 223.0 / 255.0,
        FecMode::RsInterleaved => 223.0 / 255.0,
        FecMode::Concatenated => (223.0 / 255.0) * 0.5,
        FecMode::ShortRs => 247.0 / 255.0,
        FecMode::RsStrong => 191.0 / 255.0,
        FecMode::SoftConcatenated => (223.0 / 255.0) * 0.5,
        FecMode::Ldpc => 0.5,
        FecMode::LdpcHighRate => 1024.0 / 1152.0,
        FecMode::Turbo => 1.0 / 3.0,
    }
}

/// SNR-dependent ACK timeout curve.
///
/// Policy anchor points:
/// - 15 dB -> 800 ms
/// - 25 dB -> 400 ms
///
/// Values are linearly interpolated and clamped to [400, 800].
pub fn ack_timeout_ms_for_snr(snr_db: f32) -> u16 {
    if snr_db <= 15.0 {
        return 800;
    }
    if snr_db >= 25.0 {
        return 400;
    }
    let t = (snr_db - 15.0) / 10.0;
    let timeout = 800.0 - 400.0 * t;
    timeout.round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_curve_hits_anchor_points() {
        assert_eq!(ack_timeout_ms_for_snr(15.0), 800);
        assert_eq!(ack_timeout_ms_for_snr(25.0), 400);
        assert_eq!(ack_timeout_ms_for_snr(20.0), 600);
    }

    #[test]
    fn retry_escalates_strength() {
        let policy = HarqPolicy::default();
        let a0 = policy.select(24.0, 1.0, 0);
        let a1 = policy.select(24.0, 1.0, 1);
        let a2 = policy.select(24.0, 1.0, 2);
        assert!(a1.fec_mode.strength() >= a0.fec_mode.strength());
        assert!(a2.fec_mode.strength() >= a1.fec_mode.strength());
    }

    #[test]
    fn high_snr_soft_capable_selects_high_rate_ldpc() {
        // Default policy (not soft-capable) keeps RS at high SNR — preserves the
        // behaviour of mode-agnostic `select_harq_decision`.
        let plain = HarqPolicy::default();
        assert_eq!(plain.select(28.0, 1.0, 0).fec_mode, FecMode::Rs);

        // A soft-capable mode on a strong, low-fade channel uses high-rate LDPC.
        let soft = HarqPolicy::default().with_soft_capable(true);
        assert_eq!(soft.select(28.0, 1.0, 0).fec_mode, FecMode::LdpcHighRate);

        // Just below the high-rate floor it stays on the protective ladder.
        assert_eq!(soft.select(25.0, 1.0, 0).fec_mode, FecMode::Rs);

        // Deep fade vetoes the high-rate tier even on a soft-capable rung.
        assert_ne!(soft.select(28.0, 10.0, 0).fec_mode, FecMode::LdpcHighRate);

        // A failed first attempt drops back to more-protective coding.
        let retry = soft.select(28.0, 1.0, 1).fec_mode;
        assert!(retry.strength() > FecMode::LdpcHighRate.strength());
    }
}
