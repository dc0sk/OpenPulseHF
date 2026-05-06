use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use crate::rig_mode::RigMode;
use crate::{PttController, PttError, RadioError};

/// Full CAT controller via a hamlib `rigctld` TCP daemon.
///
/// Connects to `<host>:<port>` (default `127.0.0.1:4532`) and issues long-form
/// rigctld commands (`\get_freq`, `\set_freq`, `\get_level`, etc.) for rig control,
/// plus short-form `T 1`/`T 0` for PTT — replacing `RigctldPtt` for use cases that
/// need more than just PTT.
pub struct RigctldController {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    ptt_asserted: bool,
}

impl RigctldController {
    /// Connect to a rigctld daemon at `addr` (e.g. `"127.0.0.1:4532"`).
    pub fn connect(addr: &str) -> Result<Self, RadioError> {
        let stream = TcpStream::connect(addr)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self {
            stream,
            reader,
            ptt_asserted: false,
        })
    }

    /// Set the VFO frequency in Hz.
    pub fn set_frequency(&mut self, hz: u64) -> Result<(), RadioError> {
        self.send_cmd_no_value(&format!("\\set_freq {hz}"))
    }

    /// Get the current VFO frequency in Hz.
    pub fn get_frequency(&mut self) -> Result<u64, RadioError> {
        let lines = self.send_cmd_with_value("\\get_freq")?;
        parse_value_line(&lines, "Frequency:")
            .ok_or_else(|| RadioError::RigctldProtocol("missing Frequency line".into()))
            .and_then(|v| {
                v.trim()
                    .parse::<f64>()
                    .map(|f| f as u64)
                    .map_err(|e| RadioError::Parse(format!("frequency: {e}")))
            })
    }

    /// Set the operating mode.  Passband defaults to `0` (rig default for the mode).
    pub fn set_mode(&mut self, mode: &RigMode) -> Result<(), RadioError> {
        self.send_cmd_no_value(&format!("\\set_mode {} 0", mode.as_str()))
    }

    /// Get the current operating mode.
    pub fn get_mode(&mut self) -> Result<RigMode, RadioError> {
        let lines = self.send_cmd_with_value("\\get_mode")?;
        let raw = parse_value_line(&lines, "Mode:")
            .ok_or_else(|| RadioError::RigctldProtocol("missing Mode line".into()))?;
        RigMode::from_str(raw.trim())
    }

    /// Get S-meter signal strength in dBm.
    pub fn get_signal_strength(&mut self) -> Result<i32, RadioError> {
        let lines = self.send_cmd_with_value("\\get_level STRENGTH")?;
        parse_value_line(&lines, "Level:")
            .ok_or_else(|| RadioError::RigctldProtocol("missing Level line".into()))
            .and_then(|v| {
                v.trim()
                    .parse::<f64>()
                    .map(|f| f as i32)
                    .map_err(|e| RadioError::Parse(format!("strength: {e}")))
            })
    }

    /// Get forward power output in watts.
    pub fn get_power_out(&mut self) -> Result<f32, RadioError> {
        let lines = self.send_cmd_with_value("\\get_level RFPOWER_METER_WATTS")?;
        parse_value_line(&lines, "Level:")
            .ok_or_else(|| RadioError::RigctldProtocol("missing Level line".into()))
            .and_then(|v| {
                v.trim()
                    .parse::<f32>()
                    .map_err(|e| RadioError::Parse(format!("power: {e}")))
            })
    }

    /// Get ALC level (0.0–1.0).
    pub fn get_alc(&mut self) -> Result<f32, RadioError> {
        let lines = self.send_cmd_with_value("\\get_level ALC")?;
        parse_value_line(&lines, "Level:")
            .ok_or_else(|| RadioError::RigctldProtocol("missing Level line".into()))
            .and_then(|v| {
                v.trim()
                    .parse::<f32>()
                    .map_err(|e| RadioError::Parse(format!("alc: {e}")))
            })
    }

    /// Get SWR reading.
    pub fn get_swr(&mut self) -> Result<f32, RadioError> {
        let lines = self.send_cmd_with_value("\\get_level SWR")?;
        parse_value_line(&lines, "Level:")
            .ok_or_else(|| RadioError::RigctldProtocol("missing Level line".into()))
            .and_then(|v| {
                v.trim()
                    .parse::<f32>()
                    .map_err(|e| RadioError::Parse(format!("swr: {e}")))
            })
    }

    fn send_cmd_no_value(&mut self, cmd: &str) -> Result<(), RadioError> {
        writeln!(self.stream, "{cmd}")?;
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed != "RPRT 0" {
            return Err(RadioError::RigctldProtocol(format!(
                "unexpected response: {trimmed}"
            )));
        }
        Ok(())
    }

    /// Send a command and collect all response lines up to (but not including) `RPRT 0`.
    fn send_cmd_with_value(&mut self, cmd: &str) -> Result<Vec<String>, RadioError> {
        writeln!(self.stream, "{cmd}")?;
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line)?;
            let trimmed = line.trim();
            if trimmed.starts_with("RPRT ") {
                if trimmed != "RPRT 0" {
                    return Err(RadioError::RigctldProtocol(format!(
                        "command failed: {trimmed}"
                    )));
                }
                break;
            }
            lines.push(trimmed.to_string());
        }
        Ok(lines)
    }
}

/// Find a response line that starts with `prefix:` and return the value after the colon.
fn parse_value_line<'a>(lines: &'a [String], prefix: &str) -> Option<&'a str> {
    lines
        .iter()
        .find_map(|l| l.strip_prefix(prefix).map(|v| v.trim()))
}

impl PttController for RigctldController {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        writeln!(self.stream, "T 1").map_err(PttError::Io)?;
        let mut response = String::new();
        self.reader.read_line(&mut response).map_err(PttError::Io)?;
        let trimmed = response.trim();
        if trimmed != "RPRT 0" {
            return Err(PttError::Rigctld(format!(
                "unexpected rigctld response: {trimmed}"
            )));
        }
        self.ptt_asserted = true;
        Ok(())
    }

    fn release_ptt(&mut self) -> Result<(), PttError> {
        writeln!(self.stream, "T 0").map_err(PttError::Io)?;
        let mut response = String::new();
        self.reader.read_line(&mut response).map_err(PttError::Io)?;
        let trimmed = response.trim();
        if trimmed != "RPRT 0" {
            return Err(PttError::Rigctld(format!(
                "unexpected rigctld response: {trimmed}"
            )));
        }
        self.ptt_asserted = false;
        Ok(())
    }

    fn is_asserted(&self) -> bool {
        self.ptt_asserted
    }
}
