//! OpenPulse – software modem CLI.
//!
//! # Usage
//!
//! ```text
//! openpulse transmit "Hello World" --mode BPSK100 [--device loopback]
//! openpulse receive  --mode BPSK100 [--device <name>]
//! openpulse devices
//! openpulse modes
//! ```

use anyhow::{Context, Result};
use clap::Parser;
use tracing::Level;

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qam64_plugin::Qam64Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

#[cfg(feature = "cpal-backend")]
use openpulse_audio::CpalBackend;

mod cli;
mod commands;
mod output;
mod pki;
mod radio;
mod state;

pub use cli::*;
use pki::PkiClient;
use state::load_policy_profile_or_default;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Short-circuit commands that need no hardware/network setup.
    if let Commands::Config { command } = &cli.command {
        return match command {
            cli::ConfigCommands::Init => commands::config::run_init(),
        };
    }

    if let Commands::ModeAdvisor { snr } = &cli.command {
        return commands::mode_advisor::run(*snr);
    }

    if let Commands::Daemon { addr, command } = &cli.command {
        let code = commands::daemon::run(addr, command.clone())?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }

    if let Commands::Calibrate { command, output } = &cli.command {
        // Validate --backend consistently even though calibrate uses loopback internally.
        match cli.backend.as_str() {
            "loopback" | "default" => {}
            #[cfg(feature = "cpal-backend")]
            "cpal" => {}
            name => anyhow::bail!("unknown backend '{name}'"),
        }
        return commands::calibrate::run(
            command,
            &cli.ptt,
            &cli.rig,
            &cli.rig_file,
            output.as_ref(),
        );
    }

    let level: Level = cli.log.parse().unwrap_or(Level::INFO);
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();

    let audio: Box<dyn openpulse_core::audio::AudioBackend> = match cli.backend.as_str() {
        "loopback" => Box::new(LoopbackBackend::new()),
        #[cfg(feature = "cpal-backend")]
        "default" | "cpal" => Box::new(CpalBackend::new()),
        #[cfg(not(feature = "cpal-backend"))]
        "default" => {
            eprintln!("note: cpal backend not compiled in; falling back to loopback");
            Box::new(LoopbackBackend::new())
        }
        name => anyhow::bail!("unknown backend '{name}'"),
    };

    let mut engine = ModemEngine::new(audio);
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .context("failed to register BPSK plugin")?;
    engine
        .register_plugin(Box::new(Fsk4Plugin::new()))
        .context("failed to register FSK4 plugin")?;
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .context("failed to register OFDM plugin")?;
    engine
        .register_plugin(Box::new(Psk8Plugin::new()))
        .context("failed to register 8PSK plugin")?;
    engine
        .register_plugin(Box::new(Qam64Plugin::new()))
        .context("failed to register 64QAM plugin")?;
    engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .context("failed to register QPSK plugin")?;
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .context("failed to register SC-FDMA plugin")?;
    engine.set_trust_policy_profile(load_policy_profile_or_default());
    engine.set_max_power_watts(cli.max_power);

    let pki = PkiClient::new(cli.pki_url.clone());
    let mut ptt = radio::build_ptt_controller(&cli.ptt, &cli.rig, &cli.rig_file)?;
    let mut exit_code = 0;

    match cli.command {
        Commands::Transmit { data, mode, device } => {
            commands::transmit::run(&data, &mode, device.as_deref(), &mut engine, ptt.as_mut())?;
        }
        Commands::Receive { mode, device } => {
            commands::receive::run(&mode, device.as_deref(), &mut engine)?;
        }
        Commands::Devices => {
            commands::devices::run(&cli.backend)?;
        }
        Commands::Modes => {
            commands::modes::run_modes(&engine)?;
        }
        Commands::SessionMetrics { opts } => {
            exit_code = commands::session_metrics::run(&engine, &opts)?;
        }
        Commands::Identity { command } => {
            exit_code = commands::modes::run_identity(command, &pki)?;
        }
        Commands::Trust { command } => {
            exit_code = commands::trust::run(command, &pki)?;
        }
        Commands::Diagnose { command } => {
            exit_code = commands::session::run_diagnose(command, &pki)?;
        }
        Commands::Session { command } => {
            exit_code = commands::session::run(command, &mut engine, &pki)?;
        }
        Commands::Benchmark { command } => {
            exit_code = commands::benchmark::run(command)?;
        }
        Commands::Monitor { mode } => {
            commands::monitor::run(&mut engine, &mode)?;
        }
        Commands::Broadcast {
            payload,
            mode,
            ttl,
            callsign,
        } => {
            commands::broadcast::run(&mut engine, &payload, &mode, ttl, &callsign)?;
        }
        Commands::Beacon {
            mode,
            interval,
            callsign,
            ttl,
        } => {
            commands::beacon::run(&mut engine, &mode, interval, &callsign, ttl)?;
        }
        Commands::Qsy { command } => {
            commands::qsy::run(command)?;
        }
        Commands::ModeAdvisor { .. } => unreachable!("handled above"),
        Commands::Config { .. } => unreachable!("handled above"),
        Commands::Calibrate { .. } => unreachable!("handled above"),
        Commands::Daemon { .. } => unreachable!("handled above"),
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}
