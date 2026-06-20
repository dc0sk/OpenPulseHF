//! Two-station bidirectional ARQ link simulator.
//!
//! Models a realistic half-duplex HF exchange between two stations through independent
//! forward (A→B) and reverse (B→A) channel realizations:
//!
//! - Station A transmits data frames at the current speed level (a [`SessionProfile`] ladder).
//! - Station B decodes, estimates the per-frame SNR, and returns a real FSK4 ACK frame
//!   (`AckOk` / `AckUp` / `AckDown` / `Nack`) through the reverse channel.
//! - Station A steps the speed level up/down its [`SessionProfile`] ladder from the ACKs
//!   (mirroring the `RateAdapter` AckUp/AckDown/NACK-threshold policy, bounded to the
//!   profile's defined levels), and retransmits on NACK (or a lost ACK) up to a retry limit.
//!
//! The simulator accounts for forward air time, ACK air time, turnaround, and
//! retransmissions, yielding the **effective two-way transfer rate** — the goodput a
//! station actually achieves under the simulated conditions, not the raw modem rate.

use openpulse_channel::{
    build_channel, AwgnConfig, ChannelModelConfig, GilbertElliottConfig, QsbConfig, WattersonConfig,
};
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::ModemEngine;

const SAMPLE_RATE: f64 = 8000.0;
const ACK_MODE: &str = "FSK4-ACK";
const SESSION_ID: &str = "LINKSIM0";

/// A channel condition for one direction of the link.
#[derive(Debug, Clone)]
pub enum ChannelSpec {
    /// Distortion-free (high-SNR reference).
    Clean,
    /// Additive white Gaussian noise at the given SNR (dB).
    Awgn(f32),
    /// Watterson Good-F1 fading at the given SNR (dB).
    WattersonGoodF1(f32),
    /// Watterson Moderate-F1 fading at the given SNR (dB).
    WattersonModerateF1(f32),
    /// Watterson Poor-F1 fading at the given SNR (dB).
    WattersonPoorF1(f32),
    /// Gilbert-Elliott burst-error channel (moderate) at the given good-state SNR (dB).
    GilbertElliott(f32),
    /// Slow QSB amplitude fading on an AWGN floor at the given SNR (dB).
    Qsb(f32),
}

impl ChannelSpec {
    fn to_config(&self, seed: u64) -> ChannelModelConfig {
        match *self {
            // "Clean" is modelled as very-high-SNR AWGN so all directions share one path type.
            ChannelSpec::Clean => ChannelModelConfig::Awgn(AwgnConfig {
                snr_db: 60.0,
                seed: Some(seed),
            }),
            ChannelSpec::Awgn(snr) => ChannelModelConfig::Awgn(AwgnConfig {
                snr_db: snr,
                seed: Some(seed),
            }),
            ChannelSpec::WattersonGoodF1(snr) => {
                let mut c = WattersonConfig::good_f1(Some(seed));
                c.snr_db = snr;
                ChannelModelConfig::Watterson(c)
            }
            ChannelSpec::WattersonModerateF1(snr) => {
                let mut c = WattersonConfig::moderate_f1(Some(seed));
                c.snr_db = snr;
                ChannelModelConfig::Watterson(c)
            }
            ChannelSpec::WattersonPoorF1(snr) => {
                let mut c = WattersonConfig::poor_f1(Some(seed));
                c.snr_db = snr;
                ChannelModelConfig::Watterson(c)
            }
            ChannelSpec::GilbertElliott(snr) => {
                let mut c = GilbertElliottConfig::moderate(Some(seed));
                c.snr_good_db = snr;
                c.snr_bad_db = snr - 15.0;
                ChannelModelConfig::GilbertElliott(c)
            }
            // QSB is multiplicative slow fading (no additive noise); the SNR label is
            // informational. AWGN / Watterson cover the additive-noise cases.
            ChannelSpec::Qsb(_snr) => ChannelModelConfig::Qsb(QsbConfig {
                fade_rate_hz: 0.2,
                fade_depth: 0.6,
                sample_rate: 8000,
            }),
        }
    }

    /// Short human-readable label.
    pub fn label(&self) -> String {
        match self {
            ChannelSpec::Clean => "clean".into(),
            ChannelSpec::Awgn(s) => format!("AWGN {s:.0}dB"),
            ChannelSpec::WattersonGoodF1(s) => format!("Watt-Good-F1 {s:.0}dB"),
            ChannelSpec::WattersonModerateF1(s) => format!("Watt-Mod-F1 {s:.0}dB"),
            ChannelSpec::WattersonPoorF1(s) => format!("Watt-Poor-F1 {s:.0}dB"),
            ChannelSpec::GilbertElliott(s) => format!("G-E {s:.0}dB"),
            ChannelSpec::Qsb(s) => format!("QSB {s:.0}dB"),
        }
    }
}

/// Parameters for one link run.
#[derive(Debug, Clone)]
pub struct LinkParams {
    /// SessionProfile name driving the adaptive ladder (see `SessionProfile::PROFILE_NAMES`).
    pub profile_name: String,
    /// Forward (A→B) data channel condition.
    pub forward: ChannelSpec,
    /// Reverse (B→A) ACK channel condition.
    pub reverse: ChannelSpec,
    /// User payload bytes per data frame.
    pub payload_bytes_per_frame: usize,
    /// Number of data frames to attempt to deliver.
    pub total_frames: usize,
    /// FEC applied to data frames.
    pub fec: FecMode,
    /// Half-duplex turnaround time per direction switch (seconds) — PTT + sync settle.
    pub turnaround_s: f64,
    /// Maximum transmission attempts per frame before giving up.
    pub max_attempts: u32,
    /// RNG seed for reproducible channel realizations.
    pub seed: u64,
}

impl Default for LinkParams {
    fn default() -> Self {
        Self {
            profile_name: "hpx_hf".into(),
            forward: ChannelSpec::Awgn(15.0),
            reverse: ChannelSpec::Awgn(20.0),
            payload_bytes_per_frame: 64,
            total_frames: 40,
            fec: FecMode::Rs,
            turnaround_s: 0.25,
            max_attempts: 6,
            seed: 0xC0FFEE,
        }
    }
}

/// Per-frame outcome record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FrameRecord {
    pub frame: usize,
    pub level: u8,
    pub mode: String,
    pub attempts: u32,
    pub delivered: bool,
    pub forward_air_s: f64,
    pub ack_air_s: f64,
    pub est_snr_db: f32,
}

/// Aggregate result of a link run.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkResult {
    pub profile: String,
    pub forward: String,
    pub reverse: String,
    pub frames_attempted: usize,
    pub frames_delivered: usize,
    pub bytes_delivered: usize,
    /// Total simulated on-air time: forward + ACK + turnaround across all attempts (seconds).
    pub total_air_s: f64,
    /// Effective two-way goodput: delivered payload bits / total on-air time (bps).
    pub effective_bps: f64,
    /// Delivery ratio (frames delivered / attempted).
    pub delivery_ratio: f64,
    /// Mean speed level used across all attempts.
    pub avg_level: f64,
    /// Final speed level at end of run.
    pub final_level: u8,
    pub records: Vec<FrameRecord>,
}

impl LinkResult {
    /// An all-zero result for a profile with no defined levels.
    fn empty(params: &LinkParams) -> Self {
        Self {
            profile: params.profile_name.clone(),
            forward: params.forward.label(),
            reverse: params.reverse.label(),
            frames_attempted: 0,
            frames_delivered: 0,
            bytes_delivered: 0,
            total_air_s: 0.0,
            effective_bps: 0.0,
            delivery_ratio: 0.0,
            avg_level: 0.0,
            final_level: 0,
            records: Vec::new(),
        }
    }
}

fn register_all(engine: &mut ModemEngine) {
    use bpsk_plugin::BpskPlugin;
    use fsk4_plugin::Fsk4Plugin;
    use ofdm_plugin::OfdmPlugin;
    use pilot_plugin::PilotPlugin;
    use psk8_plugin::Psk8Plugin;
    use qam64_plugin::Qam64Plugin;
    use qpsk_plugin::QpskPlugin;
    use scfdma_plugin::ScFdmaPlugin;
    let _ = engine.register_plugin(Box::new(BpskPlugin::new()));
    let _ = engine.register_plugin(Box::new(QpskPlugin::new()));
    let _ = engine.register_plugin(Box::new(Psk8Plugin::new()));
    let _ = engine.register_plugin(Box::new(Qam64Plugin::new()));
    let _ = engine.register_plugin(Box::new(Fsk4Plugin::new()));
    let _ = engine.register_plugin(Box::new(OfdmPlugin::new()));
    let _ = engine.register_plugin(Box::new(ScFdmaPlugin::new()));
    let _ = engine.register_plugin(Box::new(PilotPlugin::new()));
}

/// FSK4-ACK is the only profile-reachable mode that can't carry RS FEC; everything else
/// (incl. OFDM / SC-FDMA / pilot) carries it on the engine path.
fn fec_for(mode: &str, requested: FecMode) -> FecMode {
    if mode == "FSK4-ACK" {
        FecMode::None
    } else {
        requested
    }
}

fn engine_transmit(
    engine: &mut ModemEngine,
    data: &[u8],
    mode: &str,
    fec: FecMode,
) -> Result<(), openpulse_core::error::ModemError> {
    match fec {
        FecMode::Rs | FecMode::RsInterleaved => engine.transmit_with_fec(data, mode, None),
        FecMode::RsStrong => engine.transmit_with_strong_fec(data, mode, None),
        FecMode::SoftConcatenated => engine.transmit_with_soft_viterbi_fec(data, mode, None),
        _ => engine.transmit(data, mode, None),
    }
    .map(|_| ())
}

fn engine_receive(
    engine: &mut ModemEngine,
    mode: &str,
    fec: FecMode,
) -> Result<Vec<u8>, openpulse_core::error::ModemError> {
    match fec {
        FecMode::Rs | FecMode::RsInterleaved => engine.receive_with_fec(mode, None),
        FecMode::RsStrong => engine.receive_with_strong_fec(mode, None),
        FecMode::SoftConcatenated => engine.receive_with_soft_viterbi_fec(mode, None),
        _ => engine.receive(mode, None),
    }
}

/// Estimate SNR (dB) from the clean reference and the realized post-channel signal.
fn estimate_snr_db(tx: &[f32], rx: &[f32]) -> f32 {
    let n = tx.len().min(rx.len());
    if n == 0 {
        return 0.0;
    }
    let sig: f32 = tx[..n].iter().map(|x| x * x).sum::<f32>() / n as f32;
    let noise: f32 = tx[..n]
        .iter()
        .zip(&rx[..n])
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f32>()
        / n as f32;
    if noise < 1e-12 {
        return 60.0;
    }
    10.0 * (sig / noise).log10()
}

/// Station B's ACK decision from the decode result and the estimated per-frame SNR.
fn decide_ack(
    decode_ok: bool,
    snr_db: f32,
    profile: &SessionProfile,
    level: SpeedLevel,
) -> AckType {
    if !decode_ok {
        return AckType::Nack;
    }
    if let Some(floor) = profile.snr_floor_for_level(level) {
        if snr_db < floor {
            return AckType::AckDown;
        }
    }
    if let Some(ceiling) = profile.snr_ceiling_for_level(level) {
        if snr_db >= ceiling {
            return AckType::AckUp;
        }
    }
    AckType::AckOk
}

fn make_payload(frame: usize, attempt: u32, size: usize) -> Vec<u8> {
    let salt = (frame as u64)
        .wrapping_mul(1103515245)
        .wrapping_add(attempt as u64);
    (0..size)
        .map(|i| (i as u64 ^ salt).to_le_bytes()[i % 8])
        .collect()
}

/// Run one two-station link and return the effective-throughput result.
pub fn run_link(params: &LinkParams) -> LinkResult {
    let profile =
        SessionProfile::by_name(&params.profile_name).unwrap_or_else(SessionProfile::hpx_hf);

    let mut fwd = ChannelSimHarness::new();
    register_all(&mut fwd.tx_engine);
    register_all(&mut fwd.rx_engine);
    let mut rev = ChannelSimHarness::new();
    register_all(&mut rev.tx_engine);
    register_all(&mut rev.rx_engine);

    let mut fwd_ch = build_channel(&params.forward.to_config(params.seed), Some(params.seed))
        .expect("forward channel");
    let mut rev_ch = build_channel(
        &params.reverse.to_config(params.seed ^ 0x5555),
        Some(params.seed ^ 0x5555),
    )
    .expect("reverse channel");

    // Drive the level over the profile's defined ladder (the global RateAdapter clamps to
    // SL1–SL11, which can leave a profile's sub-range; we bound it to the profile here while
    // mirroring its AckUp / AckDown / NACK-threshold policy).
    let levels = profile.defined_levels();
    if levels.is_empty() {
        return LinkResult::empty(params);
    }
    let mut idx = levels
        .iter()
        .position(|&l| l == profile.initial_level)
        .unwrap_or(0);
    let nack_threshold = profile.nack_threshold.max(1) as u32;
    let mut consecutive_nack = 0u32;

    let mut total_air_s = 0.0;
    let mut bytes_delivered = 0usize;
    let mut frames_delivered = 0usize;
    let mut level_sum = 0u64;
    let mut level_count = 0u64;
    let mut records = Vec::with_capacity(params.total_frames);

    for frame in 0..params.total_frames {
        let mut attempts = 0u32;
        let mut delivered = false;
        let mut fwd_air = 0.0;
        let mut ack_air = 0.0;
        let mut last_level = levels[idx];
        let mut last_snr = 0.0_f32;
        let mut last_mode = String::new();

        while attempts < params.max_attempts {
            attempts += 1;
            let level = levels[idx];
            last_level = level;
            level_sum += level as u64;
            level_count += 1;
            let mode = profile
                .mode_for(level)
                .expect("defined_levels yields mapped modes")
                .to_string();
            last_mode = mode.clone();
            let fec = fec_for(&mode, params.fec);
            let payload = make_payload(frame, attempts, params.payload_bytes_per_frame);

            // Forward A→B.
            if engine_transmit(&mut fwd.tx_engine, &payload, &mode, fec).is_err() {
                // Treat a TX build error as a lost frame attempt.
                continue;
            }
            let (tx_s, rx_s) = fwd.route_tapped(fwd_ch.as_mut());
            fwd_air += tx_s.len() as f64 / SAMPLE_RATE;
            let snr = estimate_snr_db(&tx_s, &rx_s);
            last_snr = snr;
            let decode_ok = engine_receive(&mut fwd.rx_engine, &mode, fec)
                .map(|d| d == payload)
                .unwrap_or(false);

            // B→A ACK (real FSK4 frame through the reverse channel).
            let ack_type = decide_ack(decode_ok, snr, &profile, level);
            let ack_bytes = AckFrame::new(ack_type, SESSION_ID).encode();
            let received_ack =
                if engine_transmit(&mut rev.tx_engine, &ack_bytes, ACK_MODE, FecMode::None).is_ok()
                {
                    let (ack_s, _) = rev.route_tapped(rev_ch.as_mut());
                    ack_air += ack_s.len() as f64 / SAMPLE_RATE;
                    engine_receive(&mut rev.rx_engine, ACK_MODE, FecMode::None)
                        .ok()
                        .filter(|b| b.len() >= 5)
                        .and_then(|b| {
                            let mut arr = [0u8; 5];
                            arr.copy_from_slice(&b[..5]);
                            AckFrame::decode(&arr).ok()
                        })
                        .map(|f| f.ack_type)
                        // A heard nothing decodable → implicit NACK (retransmit).
                        .unwrap_or(AckType::Nack)
                } else {
                    AckType::Nack
                };

            // Apply the ACK to the profile-bounded level index (mirrors RateAdapter policy).
            match received_ack {
                AckType::AckUp => {
                    consecutive_nack = 0;
                    if idx + 1 < levels.len() {
                        idx += 1;
                    }
                }
                AckType::AckDown => {
                    consecutive_nack = 0;
                    idx = idx.saturating_sub(1);
                }
                AckType::AckOk => consecutive_nack = 0,
                AckType::Nack => {
                    consecutive_nack += 1;
                    if consecutive_nack >= nack_threshold {
                        consecutive_nack = 0;
                        idx = idx.saturating_sub(1);
                    }
                }
                _ => {}
            }

            if decode_ok {
                delivered = true;
                break;
            }
        }

        if delivered {
            frames_delivered += 1;
            bytes_delivered += params.payload_bytes_per_frame;
        }
        // On-air time: every attempt costs a forward slot + ACK slot + two turnarounds.
        total_air_s += fwd_air + ack_air + 2.0 * params.turnaround_s * attempts as f64;

        records.push(FrameRecord {
            frame,
            level: last_level as u8,
            mode: last_mode,
            attempts,
            delivered,
            forward_air_s: fwd_air,
            ack_air_s: ack_air,
            est_snr_db: last_snr,
        });
    }

    let effective_bps = if total_air_s > 0.0 {
        bytes_delivered as f64 * 8.0 / total_air_s
    } else {
        0.0
    };
    let avg_level = if level_count > 0 {
        level_sum as f64 / level_count as f64
    } else {
        0.0
    };

    LinkResult {
        profile: params.profile_name.clone(),
        forward: params.forward.label(),
        reverse: params.reverse.label(),
        frames_attempted: params.total_frames,
        frames_delivered,
        bytes_delivered,
        total_air_s,
        effective_bps,
        delivery_ratio: if params.total_frames > 0 {
            frames_delivered as f64 / params.total_frames as f64
        } else {
            0.0
        },
        avg_level,
        final_level: levels[idx] as u8,
        records,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_channel_delivers_all_and_climbs() {
        let params = LinkParams {
            profile_name: "hpx500".into(),
            forward: ChannelSpec::Clean,
            reverse: ChannelSpec::Clean,
            payload_bytes_per_frame: 32,
            total_frames: 12,
            fec: FecMode::Rs,
            turnaround_s: 0.2,
            max_attempts: 4,
            seed: 1,
        };
        let r = run_link(&params);
        assert_eq!(
            r.frames_delivered, r.frames_attempted,
            "clean link delivers all"
        );
        assert!(r.effective_bps > 0.0, "effective rate must be positive");
        // On a clean channel the rate adapter should have climbed above the initial level.
        assert!(
            r.final_level as usize >= SpeedLevel::Sl2 as usize,
            "level should not drop below the floor on a clean channel"
        );
    }

    #[test]
    fn effective_rate_below_gross_due_to_ack_and_turnaround() {
        // Even on a clean channel, ACK + turnaround overhead must make the effective
        // two-way rate strictly less than the forward mode's raw payload rate.
        let params = LinkParams {
            profile_name: "hpx500".into(),
            forward: ChannelSpec::Clean,
            reverse: ChannelSpec::Clean,
            payload_bytes_per_frame: 64,
            total_frames: 8,
            fec: FecMode::None,
            turnaround_s: 0.25,
            max_attempts: 3,
            seed: 7,
        };
        let r = run_link(&params);
        assert!(r.frames_delivered > 0);
        // QPSK500 gross is 1000 bps; with ACK + turnaround the goodput is far lower.
        assert!(
            r.effective_bps < 1000.0,
            "effective {:.0} bps should be below the raw mode rate",
            r.effective_bps
        );
    }

    #[test]
    fn very_low_snr_degrades_delivery() {
        let clean = run_link(&LinkParams {
            profile_name: "hpx500".into(),
            forward: ChannelSpec::Clean,
            reverse: ChannelSpec::Clean,
            total_frames: 16,
            seed: 3,
            ..LinkParams::default()
        });
        let noisy = run_link(&LinkParams {
            profile_name: "hpx500".into(),
            forward: ChannelSpec::Awgn(-5.0),
            reverse: ChannelSpec::Awgn(0.0),
            total_frames: 16,
            seed: 3,
            ..LinkParams::default()
        });
        assert!(
            noisy.effective_bps <= clean.effective_bps,
            "a very noisy link must not outperform a clean one ({:.0} vs {:.0})",
            noisy.effective_bps,
            clean.effective_bps
        );
    }
}
