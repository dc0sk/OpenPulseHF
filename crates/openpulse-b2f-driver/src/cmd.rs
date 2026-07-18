//! ARDOP command port — sends ASCII commands, reads responses and events.

use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::{deadline_slice, DriverError, DEFAULT_IO_TIMEOUT};

/// Longest command-port line accepted before treating the peer as hostile/broken. Mirrors the
/// server-side `MAX_CMD_LINE` in `openpulse-ardop` — reading a line without a cap grows the
/// destination without limit, so a TNC that never sends a newline could otherwise exhaust memory.
const MAX_CMD_LINE: usize = 4096;

/// Wraps the ARDOP TNC command port (ASCII line protocol).
pub struct CmdPort {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    timeout: Option<Duration>,
}

impl CmdPort {
    pub fn new(stream: TcpStream) -> Result<Self, DriverError> {
        let writer = stream.try_clone().map_err(DriverError::Io)?;
        let mut port = Self {
            reader: BufReader::new(stream),
            writer,
            timeout: None,
        };
        port.set_timeout(Some(DEFAULT_IO_TIMEOUT))?;
        Ok(port)
    }

    /// Send an ARDOP command (CR-LF terminated).
    pub fn send(&mut self, cmd: &str) -> Result<(), DriverError> {
        write!(self.writer, "{cmd}\r\n")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Read one line from the command port (strips trailing CR/LF).
    ///
    /// The configured timeout is a deadline for the WHOLE line, not per syscall: a peer that dribbles
    /// bytes without ever sending a newline cannot keep the read alive. Lines over `MAX_CMD_LINE` are
    /// rejected rather than buffered. Maps `TimedOut` / `WouldBlock` to `DriverError::Timeout`.
    pub fn read_line(&mut self) -> Result<String, DriverError> {
        let started = Instant::now();
        let mut line: Vec<u8> = Vec::new();
        loop {
            if let Some(total) = self.timeout {
                let remaining = deadline_slice(started, total).ok_or(DriverError::Timeout)?;
                self.reader.get_ref().set_read_timeout(Some(remaining))?;
            }
            let available = match self.reader.fill_buf() {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(map_read_err(e)),
            };
            if available.is_empty() {
                if line.is_empty() {
                    return Err(DriverError::Ardop("command port closed".into()));
                }
                break;
            }
            let (chunk, done) = match available.iter().position(|&b| b == b'\n') {
                Some(i) => (&available[..=i], true),
                None => (available, false),
            };
            let taken = chunk.len();
            line.extend_from_slice(chunk);
            self.reader.consume(taken);
            if line.len() > MAX_CMD_LINE {
                return Err(DriverError::Ardop(format!(
                    "command line too long (>{MAX_CMD_LINE} bytes)"
                )));
            }
            if done {
                break;
            }
        }
        let text = String::from_utf8_lossy(&line).into_owned();
        Ok(text.trim_end_matches(['\r', '\n']).to_string())
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

    /// Read lines until one starts with `want`, or one starts with any of `abort_on`.
    ///
    /// Without this, a peer that answers CONNECT with a terminal event instead of the expected one
    /// leaves the caller spinning until the read deadline — a timeout reported for what is really a
    /// refused session.
    pub fn wait_for_or_abort(
        &mut self,
        want: &str,
        abort_on: &[&str],
    ) -> Result<String, DriverError> {
        loop {
            let line = self.read_line()?;
            if line.starts_with(want) {
                return Ok(line);
            }
            if abort_on.iter().any(|p| line.starts_with(p)) {
                return Err(DriverError::Aborted);
            }
        }
    }

    /// The deadline applied to each whole read operation.
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Set (or clear) the read timeout. Applies to each whole read operation, not per syscall.
    pub fn set_timeout(&mut self, t: Option<Duration>) -> Result<(), DriverError> {
        self.reader.get_ref().set_read_timeout(t)?;
        self.timeout = t;
        Ok(())
    }
}

fn map_read_err(e: io::Error) -> DriverError {
    if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock {
        DriverError::Timeout
    } else {
        DriverError::Io(e)
    }
}
