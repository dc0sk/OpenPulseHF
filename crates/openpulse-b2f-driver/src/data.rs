//! ARDOP data port — u16 big-endian length-prefixed binary framing.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::DriverError;

/// Wraps the ARDOP TNC data port (u16 BE length-prefixed frames).
pub struct DataPort {
    stream: TcpStream,
}

impl DataPort {
    pub fn new(stream: TcpStream) -> Self {
        Self { stream }
    }

    /// Write one frame: `[u16 BE length][payload]`.
    pub fn send_frame(&mut self, data: &[u8]) -> Result<(), DriverError> {
        let len = data.len() as u16;
        self.stream.write_all(&len.to_be_bytes())?;
        self.stream.write_all(data)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Read one frame: `[u16 BE length][payload]`.
    pub fn recv_frame(&mut self) -> Result<Vec<u8>, DriverError> {
        let mut len_buf = [0u8; 2];
        self.stream.read_exact(&mut len_buf)?;
        let len = u16::from_be_bytes(len_buf) as usize;
        let mut payload = vec![0u8; len];
        self.stream.read_exact(&mut payload)?;
        Ok(payload)
    }

    /// Set (or clear) the read timeout on the underlying stream.
    pub fn set_timeout(&self, t: Option<Duration>) -> Result<(), DriverError> {
        self.stream.set_read_timeout(t)?;
        Ok(())
    }
}
