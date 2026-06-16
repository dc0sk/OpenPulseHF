//! `openpulse arq` — reliable two-way ARQ over the modem (FSK4 ACK return +
//! retransmit). Targets VOX or wired/full-duplex audio paths; keying is per
//! transmission (no manual half-duplex PTT turnaround across the ACK).

use anyhow::{anyhow, Result};

use openpulse_core::profile::SessionProfile;
use openpulse_modem::ModemEngine;

fn maybe_start_session(engine: &mut ModemEngine, profile: Option<&str>) -> Result<()> {
    if let Some(name) = profile {
        let p = SessionProfile::by_name(name).ok_or_else(|| {
            anyhow!(
                "unknown session profile {name:?}; valid profiles: {}",
                SessionProfile::PROFILE_NAMES.join(", ")
            )
        })?;
        engine.start_adaptive_session(p);
    }
    Ok(())
}

/// ISS role: transmit `payload` with ARQ, retransmitting until an ACK arrives or
/// `retries` is exhausted.
pub fn run_send(
    engine: &mut ModemEngine,
    payload: &str,
    mode: &str,
    profile: Option<&str>,
    retries: usize,
    device: Option<&str>,
) -> Result<()> {
    maybe_start_session(engine, profile)?;
    let event = engine.transmit_arq(payload.as_bytes(), mode, device, retries)?;
    let final_mode = engine.current_adaptive_mode().unwrap_or(mode);
    println!("arq send: delivered ({event:?}) final_mode={final_mode}");
    Ok(())
}

/// IRS role: receive `frames` data frames, ACKing each clean decode and NACKing
/// failures so the peer retransmits.
pub fn run_listen(
    engine: &mut ModemEngine,
    mode: &str,
    profile: Option<&str>,
    frames: usize,
    session: &str,
    device: Option<&str>,
) -> Result<()> {
    maybe_start_session(engine, profile)?;
    for i in 0..frames {
        match engine.respond_arq(mode, session, device) {
            Ok(payload) => println!(
                "arq listen: frame {i} ok ({} bytes): {}",
                payload.len(),
                String::from_utf8_lossy(&payload)
            ),
            Err(e) => println!("arq listen: frame {i} NACK ({e})"),
        }
    }
    Ok(())
}
