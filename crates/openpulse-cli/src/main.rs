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
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;

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
        .register_plugin(Box::new(Psk8Plugin::new()))
        .context("failed to register 8PSK plugin")?;
    engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .context("failed to register QPSK plugin")?;
    engine.set_trust_policy_profile(load_policy_profile_or_default());

    let pki = PkiClient::new(cli.pki_url.clone());
    let mut ptt = radio::build_ptt_controller(&cli.ptt, &cli.rig)?;
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
        Commands::Config { command } => match command {
            cli::ConfigCommands::Init => {
                commands::config::run_init()?;
            }
        },
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}
