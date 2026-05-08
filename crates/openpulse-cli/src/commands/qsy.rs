//! `openpulse qsy` subcommand — QSY frequency-agility negotiation.

use anyhow::{bail, Result};

use openpulse_config as config;
use openpulse_qsy::{scanner::QsyScanner, QsyAction, QsySession};
use openpulse_radio::RigctldController;

use crate::cli::QsyCommands;

/// Run a `qsy` subcommand.
pub fn run(command: QsyCommands) -> Result<()> {
    match command {
        QsyCommands::Init { rig } => run_init(rig),
        QsyCommands::Status => run_status(),
    }
}

fn run_init(rig_override: String) -> Result<()> {
    let cfg = config::load()?;
    let qsy_cfg = &cfg.qsy;

    if !qsy_cfg.enabled {
        bail!("QSY is disabled in config.toml — set [qsy] enabled = true to use it");
    }
    if qsy_cfg.candidate_freqs_hz.is_empty() {
        bail!("No candidate frequencies configured — set [qsy] candidate_freqs_hz = [...]");
    }

    let rig_addr = if rig_override.is_empty() {
        cfg.radio.rigctld_addr.clone()
    } else {
        rig_override
    };

    let rig = RigctldController::connect(&rig_addr)
        .map_err(|e| anyhow::anyhow!("cannot connect to rigctld at {rig_addr}: {e}"))?;
    let mut scanner = QsyScanner::new(rig, qsy_cfg.scan_dwell_ms);

    let mut session =
        QsySession::new_initiator().with_switchover_offset_s(qsy_cfg.switchover_offset_s as u32);
    let actions = session.initiate(qsy_cfg.candidate_freqs_hz.clone())?;

    for action in &actions {
        if let QsyAction::SendFrame(frame) = action {
            println!("→ {}", openpulse_qsy::frame::encode_unsigned(frame));
        }
    }

    // Run the scan.
    println!(
        "Scanning {} candidate frequencies...",
        qsy_cfg.candidate_freqs_hz.len()
    );
    let results = scanner
        .scan(&qsy_cfg.candidate_freqs_hz)
        .map_err(|e| anyhow::anyhow!("rig scan failed: {e}"))?;
    for (freq, snr) in &results {
        println!("  {freq} Hz: {snr:.1} dBm");
    }

    let actions = session.scan_complete(results)?;
    for action in &actions {
        if let QsyAction::SendFrame(frame) = action {
            println!("→ {}", openpulse_qsy::frame::encode_unsigned(frame));
        }
    }

    println!("QSY_LIST sent. Waiting for partner's QSY_VOTE...");
    println!("(In a full integration, drive session.apply(incoming_vote_frame) next)");
    Ok(())
}

fn run_status() -> Result<()> {
    let cfg = config::load()?;
    let qsy_cfg = &cfg.qsy;
    println!("QSY enabled:           {}", qsy_cfg.enabled);
    println!(
        "Allow trust levels:    {}",
        if qsy_cfg.allow_trustlevels.is_empty() {
            "(none)".into()
        } else {
            qsy_cfg.allow_trustlevels.join(", ")
        }
    );
    println!(
        "Candidate freqs (Hz):  {}",
        if qsy_cfg.candidate_freqs_hz.is_empty() {
            "(none configured)".into()
        } else {
            qsy_cfg
                .candidate_freqs_hz
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!("Scan dwell:            {} ms", qsy_cfg.scan_dwell_ms);
    println!("Switchover offset:     {} s", qsy_cfg.switchover_offset_s);
    Ok(())
}
