//! `openpulse daemon <cmd>` — drive a running daemon via its NDJSON-over-TCP control port.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use openpulse_daemon::protocol::{
    CommandResponse, ControlCommand, ControlEvent, DaemonConfig, MessageSummary,
};

use crate::cli::DaemonCommands;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Connect to the daemon control port, send `cmd`, then drain server frames
/// until a [`CommandResponse`] arrives.  Any [`ControlEvent`] frames seen
/// before the response are returned in order so subcommands can pull request
/// payloads (e.g. `MessageList`, `MessageData`, `ConfigData`).
fn run_command(addr: &str, cmd: &ControlCommand) -> Result<(Vec<ControlEvent>, CommandResponse)> {
    let stream =
        TcpStream::connect(addr).with_context(|| format!("connect to daemon at {addr}"))?;
    stream
        .set_read_timeout(Some(DEFAULT_TIMEOUT))
        .context("set read timeout")?;
    stream
        .set_write_timeout(Some(DEFAULT_TIMEOUT))
        .context("set write timeout")?;
    let mut writer = stream.try_clone().context("clone tcp stream")?;
    let mut reader = BufReader::new(stream);

    let line = serde_json::to_string(cmd).context("serialize command")?;
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    let mut events = Vec::new();
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).context("read daemon reply")?;
        if n == 0 {
            return Err(anyhow!("daemon closed connection before response"));
        }
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(resp) = serde_json::from_str::<CommandResponse>(trimmed) {
            return Ok((events, resp));
        }
        if let Ok(ev) = serde_json::from_str::<ControlEvent>(trimmed) {
            events.push(ev);
            continue;
        }
        // Unrecognized NDJSON line (e.g. an event variant the CLI doesn't model yet);
        // skip it.  Binary spectrum frames cannot appear here because we never send
        // `SubscribeSpectrum` on this connection.
    }
}

pub fn run(addr: &str, cmd: DaemonCommands) -> Result<i32> {
    match cmd {
        DaemonCommands::ConnectPeer { callsign } => {
            simple(addr, ControlCommand::ConnectPeer { callsign })
        }
        DaemonCommands::DisconnectPeer => simple(addr, ControlCommand::DisconnectPeer),
        DaemonCommands::EnableRepeater => simple(addr, ControlCommand::EnableRepeater),
        DaemonCommands::DisableRepeater => simple(addr, ControlCommand::DisableRepeater),
        DaemonCommands::DeleteMessage { id } => simple(addr, ControlCommand::DeleteMessage { id }),
        DaemonCommands::ListMessages => list_messages(addr),
        DaemonCommands::GetMessage { id } => get_message(addr, id),
        DaemonCommands::GetConfig => get_config(addr),
        DaemonCommands::SetConfig {
            mode,
            tx_attenuation_db,
            qsy_enabled,
            bandplan_mode,
            allow_tuner_on_high_swr,
        } => set_config(
            addr,
            mode,
            tx_attenuation_db,
            qsy_enabled,
            bandplan_mode,
            allow_tuner_on_high_swr,
        ),
        DaemonCommands::SubscribeSpectrum { fps, frames } => subscribe_spectrum(addr, fps, frames),
    }
}

fn simple(addr: &str, cmd: ControlCommand) -> Result<i32> {
    let (_events, resp) = run_command(addr, &cmd)?;
    if resp.ok {
        println!("ok");
        Ok(0)
    } else {
        eprintln!(
            "error: {}",
            resp.error.unwrap_or_else(|| "unknown".to_string())
        );
        Ok(1)
    }
}

fn list_messages(addr: &str) -> Result<i32> {
    let (events, resp) = run_command(addr, &ControlCommand::ListMessages)?;
    if !resp.ok {
        eprintln!(
            "error: {}",
            resp.error.unwrap_or_else(|| "unknown".to_string())
        );
        return Ok(1);
    }
    let mut messages: Vec<MessageSummary> = Vec::new();
    for ev in events {
        if let ControlEvent::MessageList { messages: m } = ev {
            messages = m;
        }
    }
    println!("{}", serde_json::to_string_pretty(&messages)?);
    Ok(0)
}

fn get_message(addr: &str, id: u64) -> Result<i32> {
    let (events, resp) = run_command(addr, &ControlCommand::GetMessage { id })?;
    if !resp.ok {
        eprintln!(
            "error: {}",
            resp.error.unwrap_or_else(|| "unknown".to_string())
        );
        return Ok(1);
    }
    for ev in events {
        if let ControlEvent::MessageData {
            id,
            from,
            to,
            subject,
            body,
        } = ev
        {
            let out = serde_json::json!({
                "id": id,
                "from": from,
                "to": to,
                "subject": subject,
                "body": body,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(0);
        }
    }
    eprintln!("error: daemon returned no message data");
    Ok(1)
}

fn get_config(addr: &str) -> Result<i32> {
    let (events, resp) = run_command(addr, &ControlCommand::GetConfig)?;
    if !resp.ok {
        eprintln!(
            "error: {}",
            resp.error.unwrap_or_else(|| "unknown".to_string())
        );
        return Ok(1);
    }
    for ev in events {
        if let ControlEvent::ConfigData { config } = ev {
            println!("{}", serde_json::to_string_pretty(&config)?);
            return Ok(0);
        }
    }
    eprintln!("error: daemon returned no config data");
    Ok(1)
}

fn set_config(
    addr: &str,
    mode: Option<String>,
    tx_attenuation_db: Option<f32>,
    qsy_enabled: Option<bool>,
    bandplan_mode: Option<String>,
    allow_tuner_on_high_swr: Option<bool>,
) -> Result<i32> {
    let (events, resp) = run_command(addr, &ControlCommand::GetConfig)?;
    if !resp.ok {
        eprintln!(
            "error: failed to read current config: {}",
            resp.error.unwrap_or_else(|| "unknown".to_string())
        );
        return Ok(1);
    }
    let mut current: Option<DaemonConfig> = None;
    for ev in events {
        if let ControlEvent::ConfigData { config } = ev {
            current = Some(config);
        }
    }
    let mut cfg = current.ok_or_else(|| anyhow!("daemon returned no config snapshot"))?;
    if let Some(m) = mode {
        cfg.mode = m;
    }
    if let Some(db) = tx_attenuation_db {
        cfg.tx_attenuation_db = db;
    }
    if let Some(q) = qsy_enabled {
        cfg.qsy_enabled = q;
    }
    if let Some(bp) = bandplan_mode {
        cfg.bandplan_mode = bp;
    }
    if let Some(v) = allow_tuner_on_high_swr {
        cfg.allow_tuner_on_high_swr = v;
    }
    simple(addr, ControlCommand::SetConfig { config: cfg })
}

fn subscribe_spectrum(addr: &str, fps: u32, frames: u32) -> Result<i32> {
    use openpulse_daemon::protocol::{decode_spectrum_frame, SPECTRUM_MAGIC};
    use std::io::Read;

    let stream =
        TcpStream::connect(addr).with_context(|| format!("connect to daemon at {addr}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .context("set read timeout")?;
    let mut writer = stream.try_clone().context("clone tcp stream")?;
    let mut reader = BufReader::new(stream);

    let cmd = ControlCommand::SubscribeSpectrum { fps };
    let line = serde_json::to_string(&cmd)?;
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    let mut received = 0u32;
    loop {
        let buf = reader.fill_buf().context("read daemon stream")?;
        if buf.is_empty() {
            break;
        }
        if buf[0] == b'{' {
            // NDJSON frame: consume one line.
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let trimmed = line.trim();
            if let Ok(resp) = serde_json::from_str::<CommandResponse>(trimmed) {
                if !resp.ok {
                    eprintln!(
                        "error: {}",
                        resp.error.unwrap_or_else(|| "unknown".to_string())
                    );
                    return Ok(1);
                }
                continue;
            }
            // Forward events as NDJSON to stdout for piping.
            println!("{trimmed}");
            continue;
        }
        if buf[0] == SPECTRUM_MAGIC[0] {
            // Read and verify the full 4-byte magic before trusting the header.
            let mut magic = [0u8; 4];
            reader
                .read_exact(&mut magic)
                .context("read spectrum magic")?;
            if magic != *SPECTRUM_MAGIC {
                return Err(anyhow!("bad spectrum magic: {magic:02X?}"));
            }
            let mut tail = [0u8; 6];
            reader
                .read_exact(&mut tail)
                .context("read spectrum header tail")?;
            let fft_size = u16::from_le_bytes([tail[0], tail[1]]) as usize;
            let mut bins = vec![0u8; fft_size * 4];
            reader.read_exact(&mut bins).context("read spectrum bins")?;
            let mut frame = Vec::with_capacity(10 + bins.len());
            frame.extend_from_slice(&magic);
            frame.extend_from_slice(&tail);
            frame.extend_from_slice(&bins);
            let (sample_rate, bins) =
                decode_spectrum_frame(&frame).map_err(|e| anyhow!("decode spectrum: {e}"))?;
            let out = serde_json::json!({
                "type": "spectrum",
                "sample_rate": sample_rate,
                "bins": bins,
            });
            println!("{out}");
            received += 1;
            if frames > 0 && received >= frames {
                return Ok(0);
            }
            continue;
        }
        // Unknown lead byte — drop a byte and retry to resynchronize.
        reader.consume(1);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;

    /// Spawn a one-shot mock daemon on an ephemeral port.  Returns the bound
    /// address and the join handle.  The mock reads one NDJSON command from
    /// the client and replies with `reply_lines` (newline-terminated) in order.
    fn mock_daemon(reply_lines: Vec<String>) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let sock_clone = sock.try_clone().unwrap();
            let mut reader = BufReader::new(sock_clone);
            let mut req = String::new();
            reader.read_line(&mut req).unwrap();
            let mut writer = sock;
            for line in reply_lines {
                writer.write_all(line.as_bytes()).unwrap();
                writer.write_all(b"\n").unwrap();
            }
            writer.flush().ok();
            req
        });
        (addr, handle)
    }

    #[test]
    fn connect_peer_sends_and_parses_ok() {
        let (addr, handle) = mock_daemon(vec![r#"{"ok":true}"#.into()]);
        let (events, resp) = run_command(
            &addr,
            &ControlCommand::ConnectPeer {
                callsign: "W1AW".into(),
            },
        )
        .unwrap();
        let req = handle.join().unwrap();
        assert!(events.is_empty());
        assert!(resp.ok);
        let parsed: ControlCommand = serde_json::from_str(req.trim()).unwrap();
        assert!(matches!(parsed, ControlCommand::ConnectPeer { callsign } if callsign == "W1AW"));
    }

    #[test]
    fn error_response_propagates() {
        let (addr, _) = mock_daemon(vec![r#"{"ok":false,"error":"no peer"}"#.into()]);
        let (_, resp) = run_command(&addr, &ControlCommand::DisconnectPeer).unwrap();
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("no peer"));
    }

    #[test]
    fn list_messages_drains_event_before_response() {
        let (addr, _) = mock_daemon(vec![
            r#"{"type":"message_list","messages":[{"id":1,"from":"A","to":"B","subject":"hi","timestamp_secs":0}]}"#
                .into(),
            r#"{"ok":true}"#.into(),
        ]);
        let (events, resp) = run_command(&addr, &ControlCommand::ListMessages).unwrap();
        assert!(resp.ok);
        assert_eq!(events.len(), 1);
        match &events[0] {
            ControlEvent::MessageList { messages } => assert_eq!(messages.len(), 1),
            _ => panic!("expected MessageList event"),
        }
    }
}
