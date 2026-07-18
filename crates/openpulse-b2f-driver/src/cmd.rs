//! ARDOP command port — sends ASCII commands, reads responses and events.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::DriverError;

/// Longest command-port line accepted before treating the peer as hostile/broken. Mirrors the
/// server-side `MAX_CMD_LINE` in `openpulse-ardop` — `read_line` alone grows its destination without
/// limit, so a TNC that never sends a newline could otherwise exhaust this process's memory.
const MAX_CMD_LINE: usize = 4096;

/// Wraps the ARDOP TNC command port (ASCII line protocol).
pub struct CmdPort {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl CmdPort {
    pub fn new(stream: TcpStream) -> Result<Self, crate::DriverError> {
        let writer = stream.try_clone().map_err(crate::DriverError::Io)?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
        })
    }

    /// Send an ARDOP command (CR-LF terminated).
    pub fn send(&mut self, cmd: &str) -> Result<(), DriverError> {
        write!(self.writer, "{cmd}\r\n")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Read one line from the command port (strips trailing CR/LF).
    ///
    /// Maps `TimedOut` / `WouldBlock` I/O errors to `DriverError::Timeout`.
    pub fn read_line(&mut self) -> Result<String, DriverError> {
        let mut line = String::new();
        // Bound the read to `MAX_CMD_LINE + 1` bytes so a newline-starved peer can't grow `line`
        // without limit — a length check after an unbounded `read_line` would apply too late.
        let n = (&mut self.reader)
            .take(MAX_CMD_LINE as u64 + 1)
            .read_line(&mut line)
            .map_err(|e| {
                if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock {
                    DriverError::Timeout
                } else {
                    DriverError::Io(e)
                }
            })?;
        if n == 0 {
            return Err(DriverError::Ardop("command port closed".into()));
        }
        // A line at or over the cap arrives with no trailing newline (the `Take` EOFs first).
        if n > MAX_CMD_LINE {
            return Err(DriverError::Ardop(format!(
                "command line too long (>{MAX_CMD_LINE} bytes)"
            )));
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    /// Read lines until one starts with `prefix`, returning that line.
    pub fn wait_for(&mut self, prefix: &str) -> Result<String, DriverError> {
        loop {
            let line = self.read_line()?;
            if line.starts_with(prefix) {
                return Ok(line);
            }
        }
    }

    /// Set (or clear) the read timeout on the underlying stream.
    pub fn set_timeout(&self, t: Option<Duration>) -> Result<(), DriverError> {
        self.reader.get_ref().set_read_timeout(t)?;
        Ok(())
    }
}
