//! ARDOP command port — sends ASCII commands, reads responses and events.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::DriverError;

/// Wraps the ARDOP TNC command port (ASCII line protocol).
pub struct CmdPort {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl CmdPort {
    pub fn new(stream: TcpStream) -> Self {
        let writer = stream.try_clone().expect("TcpStream clone for cmd write");
        Self {
            reader: BufReader::new(stream),
            writer,
        }
    }

    /// Send an ARDOP command (CR-LF terminated).
    pub fn send(&mut self, cmd: &str) -> Result<(), DriverError> {
        write!(self.writer, "{cmd}\r\n")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Read one line from the command port (strips trailing CR/LF).
    pub fn read_line(&mut self) -> Result<String, DriverError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Err(DriverError::Ardop("command port closed".into()));
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
