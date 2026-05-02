use anyhow::{Context, Result};
use openpulse_audio::LoopbackBackend;
use openpulse_core::audio::AudioBackend;

#[cfg(feature = "cpal-backend")]
use openpulse_audio::CpalBackend;

pub fn run(backend: &str) -> Result<()> {
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
