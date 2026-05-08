//! `openpulse broadcast` subcommand handler.

use anyhow::{bail, Result};
use openpulse_modem::ModemEngine;

/// Run the broadcast subcommand.
pub fn run(
    engine: &mut ModemEngine,
    payload_str: &str,
    mode: &str,
    ttl: u8,
    callsign: &str,
) -> Result<()> {
    let payload = parse_payload(payload_str)?;
    engine.set_callsign(callsign);
    engine.broadcast(&payload, mode, ttl, None)?;
    println!(
        "broadcast: {} bytes sent via {mode} (ttl={ttl})",
        payload.len()
    );
    Ok(())
}

fn parse_payload(s: &str) -> Result<Vec<u8>> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        if hex.len() % 2 != 0 {
            bail!("hex payload has odd number of nibbles");
        }
        hex.as_bytes()
            .chunks(2)
            .map(|c| {
                let hi = hex_nibble(c[0])?;
                let lo = hex_nibble(c[1])?;
                Ok((hi << 4) | lo)
            })
            .collect()
    } else {
        Ok(s.as_bytes().to_vec())
    }
}

fn hex_nibble(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => bail!("invalid hex character: {}", b as char),
    }
}
