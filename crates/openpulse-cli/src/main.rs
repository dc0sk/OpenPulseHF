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

use anyhow::{anyhow, Result};
use clap::Parser;
use openpulse_core::fec::FecMode;
use tracing::Level;

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;

#[cfg(feature = "cpal-backend")]
use openpulse_audio::CpalBackend;

mod cli;
mod commands;
mod output;
mod pki;
mod plugins;
mod radio;
mod state;

pub use cli::*;
use pki::PkiClient;
use state::load_policy_profile_or_default;

/// Parse a `--fec` codec name into a [`FecMode`].
fn parse_fec(s: &str) -> Result<FecMode> {
    Ok(match s.to_ascii_lowercase().replace('_', "-").as_str() {
        "none" => FecMode::None,
        "rs" => FecMode::Rs,
        "rs-interleaved" => FecMode::RsInterleaved,
        "concatenated" => FecMode::Concatenated,
        "rs-strong" => FecMode::RsStrong,
        "soft-concatenated" => FecMode::SoftConcatenated,
        "ldpc" => FecMode::Ldpc,
        "turbo" => FecMode::Turbo,
        other => {
            return Err(anyhow!(
                "unknown --fec '{other}' (expected none, rs, rs-interleaved, \
                 concatenated, rs-strong, soft-concatenated, ldpc, or turbo)"
            ))
        }
    })
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Short-circuit commands that need no hardware/network setup.
    if let Commands::Config { command } = &cli.command {
        return match command {
            cli::ConfigCommands::Init => commands::config::run_init(),
        };
    }

    if let Commands::ModeAdvisor { snr, profile } = &cli.command {
        return commands::mode_advisor::run(*snr, profile.as_deref());
    }

    if let Commands::Adaptive {
        profile,
        channel,
        snr,
        frames,
        payload_len,
        min_backlog,
        seed,
        json,
    } = &cli.command
    {
        return commands::adaptive::run(
            profile.as_deref(),
            channel,
            *snr,
            *frames,
            *payload_len,
            *min_backlog,
            *seed,
            *json,
        );
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
    plugins::register_all(&mut engine)?;
    engine.set_trust_policy_profile(load_policy_profile_or_default());
    engine.set_max_power_watts(cli.max_power);

    let pki = PkiClient::new(cli.pki_url.clone());
    let mut ptt = radio::build_ptt_controller(&cli.ptt, &cli.rig, &cli.rig_file)?;
    let mut exit_code = 0;

    match cli.command {
        Commands::Transmit {
            data,
            mode,
            device,
            fec,
            center_frequency,
        } => {
            if center_frequency != 1500.0 {
                engine.set_center_frequency(center_frequency);
            }
            let fec = parse_fec(&fec)?;
            commands::transmit::run(
                &data,
                &mode,
                fec,
                device.as_deref(),
                &mut engine,
                ptt.as_mut(),
            )?;
        }
        Commands::Receive {
            mode,
            device,
            fec,
            listen_ms,
            center_frequency,
            no_afc,
        } => {
            if center_frequency != 1500.0 {
                engine.set_center_frequency(center_frequency);
            }
            if no_afc {
                engine.disable_afc();
            }
            let fec = parse_fec(&fec)?;
            commands::receive::run(&mode, fec, device.as_deref(), listen_ms, &mut engine)?;
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
        Commands::Arq { command } => match command {
            cli::ArqCommands::Send {
                payload,
                mode,
                profile,
                retries,
                device,
            } => {
                commands::arq::run_send(
                    &mut engine,
                    &payload,
                    &mode,
                    profile.as_deref(),
                    retries,
                    device.as_deref(),
                )?;
            }
            cli::ArqCommands::Listen {
                mode,
                profile,
                frames,
                session,
                device,
            } => {
                commands::arq::run_listen(
                    &mut engine,
                    &mode,
                    profile.as_deref(),
                    frames,
                    &session,
                    device.as_deref(),
                )?;
            }
        },
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
        Commands::Adaptive { .. } => unreachable!("handled above"),
        Commands::Config { .. } => unreachable!("handled above"),
        Commands::Calibrate { .. } => unreachable!("handled above"),
        Commands::Daemon { .. } => unreachable!("handled above"),
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}
