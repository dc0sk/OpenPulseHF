//! ARDOP data port — u16 big-endian length-prefixed binary framing.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::{deadline_slice, DriverError, DEFAULT_IO_TIMEOUT};

/// Wraps the ARDOP TNC data port (u16 BE length-prefixed frames).
pub struct DataPort {
    stream: TcpStream,
    timeout: Option<Duration>,
}

impl DataPort {
    pub fn new(stream: TcpStream) -> Self {
        let mut port = Self {
            stream,
            timeout: None,
        };
        // A port with no timeout hangs forever on a silent peer; install a default the caller can
        // override. Failure to set it is not fatal — the port still works, just unbounded.
        let _ = port.set_timeout(Some(DEFAULT_IO_TIMEOUT));
        port
    }

    /// Write one frame: `[u16 BE length][payload]`.
    pub fn send_frame(&mut self, data: &[u8]) -> Result<(), DriverError> {
        let len: u16 = data.len().try_into().map_err(|_| {
            DriverError::Ardop(format!(
                "frame payload {} bytes exceeds u16::MAX",
                data.len()
            ))
        })?;
        self.stream.write_all(&len.to_be_bytes())?;
        self.stream.write_all(data)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Read one frame: `[u16 BE length][payload]`.
    ///
    /// The timeout is a deadline for the whole frame, so a peer that dribbles bytes cannot hold the
    /// read open indefinitely (`SO_RCVTIMEO` alone restarts on every partial read).
    pub fn recv_frame(&mut self) -> Result<Vec<u8>, DriverError> {
        let started = Instant::now();
        let mut len_buf = [0u8; 2];
        self.read_exact_by(started, &mut len_buf)?;
        let len = u16::from_be_bytes(len_buf) as usize;
        let mut payload = vec![0u8; len];
        self.read_exact_by(started, &mut payload)?;
        Ok(payload)
    }

    /// `read_exact`, but bounded by a single deadline measured from `started` rather than by a
    /// per-syscall timeout that any inbound byte resets.
    fn read_exact_by(&mut self, started: Instant, buf: &mut [u8]) -> Result<(), DriverError> {
        let mut filled = 0usize;
        while filled < buf.len() {
            if let Some(total) = self.timeout {
                let remaining = deadline_slice(started, total).ok_or(DriverError::Timeout)?;
                self.stream.set_read_timeout(Some(remaining))?;
            }
            match self.stream.read(&mut buf[filled..]) {
                Ok(0) => {
                    return Err(DriverError::Io(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "data port closed mid-frame",
                    )))
                }
                Ok(n) => filled += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e)
                    if e.kind() == io::ErrorKind::TimedOut
                        || e.kind() == io::ErrorKind::WouldBlock =>
                {
                    return Err(DriverError::Timeout)
                }
                Err(e) => return Err(DriverError::Io(e)),
            }
        }
        Ok(())
    }

    /// The deadline applied to each whole read operation.
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Set (or clear) the read timeout. Applies to each whole frame, not per syscall.
    pub fn set_timeout(&mut self, t: Option<Duration>) -> Result<(), DriverError> {
        self.stream.set_read_timeout(t)?;
        self.timeout = t;
        Ok(())
    }
}
