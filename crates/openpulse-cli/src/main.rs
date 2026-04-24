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
use clap::{Parser, Subcommand};
use tracing::Level;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;

#[cfg(feature = "cpal-backend")]
use openpulse_audio::CpalBackend;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "openpulse",
    about = "OpenPulse software modem for amateur radio data transmission",
    version
)]
struct Cli {
    /// Audio backend to use.
    ///
    /// Use `loopback` for testing without hardware.  On Linux the default
    /// is the system audio (ALSA / PipeWire via cpal).
    #[arg(long, global = true, default_value = "default")]
    backend: String,

    /// Verbosity level: error | warn | info | debug | trace.
    #[arg(long, global = true, default_value = "info")]
    log: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Transmit data over the air.
    Transmit {
        /// Data string to transmit (UTF-8).
        data: String,

        /// Modulation mode (e.g. BPSK100, BPSK31).
        #[arg(short, long, default_value = "BPSK100")]
        mode: String,

        /// Audio device name.  Omit to use the backend default.
        #[arg(short, long)]
        device: Option<String>,
    },

    /// Receive data and print to stdout.
    Receive {
        /// Modulation mode (e.g. BPSK100, BPSK31).
        #[arg(short, long, default_value = "BPSK100")]
        mode: String,

        /// Audio device name.  Omit to use the backend default.
        #[arg(short, long)]
        device: Option<String>,
    },

    /// List available audio devices.
    Devices,

    /// List registered modulation modes.
    Modes,
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialise logging.
    let level: Level = cli.log.parse().unwrap_or(Level::INFO);
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();

    // Build audio backend.
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

    // Build engine and register plugins.
    let mut engine = ModemEngine::new(audio);
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .context("failed to register BPSK plugin")?;

    // Dispatch.
    match cli.command {
        Commands::Transmit { data, mode, device } => {
            let dev = device.as_deref();
            engine
                .transmit(data.as_bytes(), &mode, dev)
                .context("transmit failed")?;
            println!("Transmitted {} bytes in {mode} mode.", data.len());
        }

        Commands::Receive { mode, device } => {
            let dev = device.as_deref();
            let payload = engine.receive(&mode, dev).context("receive failed")?;
            let text = String::from_utf8_lossy(&payload);
            println!("{text}");
        }

        Commands::Devices => {
            // The engine doesn't expose the backend directly, so re-create a
            // temporary backend just for device listing.
            list_devices(&cli.backend)?;
        }

        Commands::Modes => {
            for info in engine.plugins().list() {
                println!(
                    "{}: {} ({})",
                    info.name,
                    info.description,
                    info.supported_modes.join(", ")
                );
            }
        }
    }

    Ok(())
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn list_devices(backend: &str) -> Result<()> {
    use openpulse_core::audio::AudioBackend;

    let b: Box<dyn AudioBackend> = match backend {
        "loopback" => Box::new(LoopbackBackend::new()),
        #[cfg(feature = "cpal-backend")]
        _ => Box::new(CpalBackend::new()),
        #[cfg(not(feature = "cpal-backend"))]
        _ => Box::new(LoopbackBackend::new()),
    };

    let devices = b.list_devices().context("failed to list devices")?;
    if devices.is_empty() {
        println!("No audio devices found.");
        return Ok(());
    }
    println!("{:<40} {:<8} {:<8} Sample rates", "Name", "Input", "Output");
    println!("{}", "-".repeat(80));
    for d in devices {
        let rates: Vec<String> = d
            .supported_sample_rates
            .iter()
            .map(|r| r.to_string())
            .collect();
        println!(
            "{:<40} {:<8} {:<8} {}",
            d.name,
            if d.is_input { "yes" } else { "no" },
            if d.is_output { "yes" } else { "no" },
            rates.join(", "),
        );
    }
    Ok(())
}
