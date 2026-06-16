//! `openpulse adaptive` — run an adaptive rate-control session over a simulated
//! channel and report each speed-level transition.
//!
//! This drives a real in-process session through [`ChannelSimHarness`]: each
//! frame is transmitted at the current adaptive mode, routed through the channel,
//! and decoded; the measured SNR feeds the rate adapter and a positive ACK
//! (on successful decode) or NACK (on failure) steps the speed ladder. It needs
//! no audio hardware and makes the adaptive controller runnable and observable.

use anyhow::{anyhow, bail, Result};
use serde_json::json;

use openpulse_channel::{
    awgn::AwgnChannel, watterson::WattersonChannel, AwgnConfig, ChannelModel, WattersonConfig,
};
use openpulse_core::ack::AckType;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::channel_sim::ChannelSimHarness;

use crate::commands::mode_advisor::speed_level_label;
use crate::plugins;

/// Profile name from the CLI override, else config `[modem] profile`, else the default.
fn resolve_profile_name(profile_override: Option<&str>) -> String {
    if let Some(name) = profile_override {
        return name.to_string();
    }
    openpulse_config::load()
        .map(|cfg| cfg.modem.profile)
        .unwrap_or_else(|_| "hpx_hf".to_string())
}

/// Build a channel model (`None` = clean passthrough) and the SNR hint (dB) to
/// feed the rate adapter when the receiver cannot measure one.
fn build_channel(
    name: &str,
    snr: Option<f32>,
    seed: Option<u64>,
) -> Result<(Option<Box<dyn ChannelModel>>, f32)> {
    match name {
        "clean" => Ok((None, snr.unwrap_or(40.0))),
        "awgn" => {
            let snr_db = snr.ok_or_else(|| anyhow!("--channel awgn requires --snr"))?;
            let ch = AwgnChannel::new(AwgnConfig::new(snr_db, seed.or(Some(42))))?;
            Ok((Some(Box::new(ch)), snr_db))
        }
        "watterson-good-f1" => {
            let ch = WattersonChannel::new(WattersonConfig::good_f1(seed.or(Some(1))))?;
            Ok((Some(Box::new(ch)), snr.unwrap_or(20.0)))
        }
        "watterson-poor-f1" => {
            let ch = WattersonChannel::new(WattersonConfig::poor_f1(seed.or(Some(4))))?;
            Ok((Some(Box::new(ch)), snr.unwrap_or(8.0)))
        }
        other => bail!(
            "unknown channel {other:?}; valid: clean, awgn, watterson-good-f1, watterson-poor-f1"
        ),
    }
}

fn level_str(level: Option<SpeedLevel>) -> &'static str {
    level.map(speed_level_label).unwrap_or("—")
}

fn ack_str(ack: AckType) -> &'static str {
    match ack {
        AckType::AckUp => "ACK-UP",
        AckType::Nack => "NACK",
        _ => "ACK",
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    profile_override: Option<&str>,
    channel_name: &str,
    snr: Option<f32>,
    frames: usize,
    payload_len: usize,
    seed: Option<u64>,
    json_out: bool,
) -> Result<()> {
    let name = resolve_profile_name(profile_override);
    let profile = SessionProfile::by_name(&name).ok_or_else(|| {
        anyhow!(
            "unknown session profile {name:?}; valid profiles: {}",
            SessionProfile::PROFILE_NAMES.join(", ")
        )
    })?;
    let (mut channel, channel_snr_db) = build_channel(channel_name, snr, seed)?;

    let mut h = ChannelSimHarness::new();
    plugins::register_all(&mut h.tx_engine)?;
    plugins::register_all(&mut h.rx_engine)?;
    h.tx_engine.start_adaptive_session(profile.clone());
    h.rx_engine.start_adaptive_session(profile);

    let payload: Vec<u8> = (0..payload_len).map(|i| i as u8).collect();
    let initial_mode = h
        .tx_engine
        .current_adaptive_mode()
        .unwrap_or("UNMAPPED")
        .to_string();
    let initial_level = h.tx_engine.current_tx_level();

    if !json_out {
        println!(
            "adaptive session: profile={name} channel={channel_name} frames={frames} payload={payload_len}B"
        );
        println!(
            "  start: level={} mode={}",
            level_str(initial_level),
            initial_mode
        );
    }

    let mut frames_ok = 0usize;
    let mut transitions = 0usize;
    let mut total_tx_samples = 0usize;
    let mut total_bytes_rx = 0usize;

    for i in 0..frames {
        let mode = h
            .tx_engine
            .current_adaptive_mode()
            .unwrap_or(&initial_mode)
            .to_string();

        h.tx_engine.transmit(&payload, &mode, None)?;
        total_tx_samples += match channel.as_deref_mut() {
            Some(ch) => h.route(ch),
            None => h.route_clean(),
        };
        let decoded_ok = match h.rx_engine.receive(&mode, None) {
            Ok(data) => {
                total_bytes_rx += data.len();
                data == payload
            }
            Err(_) => false,
        };
        if decoded_ok {
            frames_ok += 1;
        }

        // Feed the simulated channel's configured SNR to the adapter. The
        // receiver's LLR-derived estimate (`last_rx_snr_db`) is mode-dependent and
        // unreliable for narrowband BPSK, which would sabotage the ladder.
        let level_before = h.tx_engine.current_tx_level();
        h.tx_engine.apply_snr_hint(channel_snr_db);
        let ack = if decoded_ok {
            AckType::AckUp
        } else {
            AckType::Nack
        };
        let rate_event = h.tx_engine.apply_ack(ack);
        let level_after = h.tx_engine.current_tx_level();
        let mode_after = h
            .tx_engine
            .current_adaptive_mode()
            .unwrap_or("UNMAPPED")
            .to_string();
        let changed = level_before != level_after;
        if changed {
            transitions += 1;
        }

        if json_out {
            println!(
                "{}",
                json!({
                    "frame": i,
                    "mode": mode,
                    "decoded": decoded_ok,
                    "snr_db": channel_snr_db,
                    "ack": ack_str(ack),
                    "rate_event": format!("{rate_event:?}"),
                    "level_after": level_str(level_after),
                    "mode_after": mode_after,
                    "changed": changed,
                })
            );
        } else {
            print!(
                "  frame {i}: mode={mode} decoded={} snr={channel_snr_db:.1}dB ack={}",
                if decoded_ok { "ok" } else { "FAIL" },
                ack_str(ack),
            );
            if changed {
                println!(" → {} ({mode_after})", level_str(level_after));
            } else {
                println!();
            }
        }
    }

    let final_mode = h
        .tx_engine
        .current_adaptive_mode()
        .unwrap_or("UNMAPPED")
        .to_string();
    let final_level = h.tx_engine.current_tx_level();
    let on_air_s = total_tx_samples as f64 / 8000.0;
    let eff_bps = if on_air_s > 0.0 {
        total_bytes_rx as f64 * 8.0 / on_air_s
    } else {
        0.0
    };

    if json_out {
        println!(
            "{}",
            json!({
                "summary": true,
                "profile": name,
                "channel": channel_name,
                "frames": frames,
                "frames_ok": frames_ok,
                "transitions": transitions,
                "final_level": level_str(final_level),
                "final_mode": final_mode,
                "effective_bps": eff_bps,
            })
        );
    } else {
        println!(
            "  final: level={} mode={final_mode} | {frames_ok}/{frames} frames decoded, {transitions} transitions, ~{eff_bps:.0} bps",
            level_str(final_level)
        );
    }

    Ok(())
}
