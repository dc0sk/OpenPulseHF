//! B2F session driver — connects to a running ARDOP TNC via TCP and drives
//! a full B2F session lifecycle (ISS sending or IRS receiving).

mod cmd;
mod data;

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use openpulse_b2f::{banner, B2fSession, SessionRole, WlHeader};

pub use cmd::CmdPort;
pub use data::DataPort;

/// A decoded message received during an IRS session.
///
/// `body` is the raw decompressed payload passed by the ISS.  Full header
/// fields (From, To, Subject) are not separately available in this version
/// because the current `B2fSession` compresses only the body bytes; encoding
/// the full `WlHeader` into the compressed stream is deferred to a later phase.
pub struct DecodedMessage {
    pub body: Vec<u8>,
}

/// Errors that can occur during a B2F driver session.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("B2F protocol error: {0}")]
    B2f(#[from] openpulse_b2f::B2fError),
    #[error("ARDOP error: {0}")]
    Ardop(String),
    #[error("timeout waiting for ARDOP event")]
    Timeout,
    #[error("session aborted by remote")]
    Aborted,
}

/// Drives a B2F session over a connected ARDOP TNC.
pub struct B2fDriver {
    cmd: CmdPort,
    data: DataPort,
}

impl B2fDriver {
    /// Construct from pre-connected TCP streams.
    pub fn new(cmd: TcpStream, data: TcpStream) -> Self {
        Self {
            cmd: CmdPort::new(cmd),
            data: DataPort::new(data),
        }
    }

    /// Connect to a running ARDOP TNC at `cmd_addr` (command port) and
    /// `data_addr` (data port) with the given I/O timeout.
    pub fn connect(
        cmd_addr: impl ToSocketAddrs,
        data_addr: impl ToSocketAddrs,
        timeout: Duration,
    ) -> Result<Self, DriverError> {
        let cmd_stream = TcpStream::connect(cmd_addr)?;
        let data_stream = TcpStream::connect(data_addr)?;
        cmd_stream.set_read_timeout(Some(timeout))?;
        data_stream.set_read_timeout(Some(timeout))?;
        Ok(Self::new(cmd_stream, data_stream))
    }

    /// ISS: connect to `remote_call`, send queued messages, then disconnect.
    ///
    /// Each entry in `messages` is `(header, uncompressed body)`.  The header
    /// fields populate the B2F FC frame; the body is what gets compressed and
    /// transferred to IRS.
    pub fn run_iss(
        &mut self,
        callsign: &str,
        remote_call: &str,
        messages: Vec<(WlHeader, Vec<u8>)>,
    ) -> Result<(), DriverError> {
        self.cmd.send(&format!("MYID {callsign}"))?;
        self.cmd.wait_for("MYID")?;
        self.cmd.send(&format!("CONNECT 500 {remote_call}"))?;
        self.cmd.wait_for("CONNECTED")?;

        let mut session = B2fSession::new(SessionRole::Iss);
        for (header, body) in messages {
            session.queue_message(header, body)?;
        }

        // Receive IRS banner, reply with FC + FF.
        let banner_frame = self.data.recv_frame()?;
        let banner_line = String::from_utf8_lossy(&banner_frame).into_owned();
        let fc_ff = session.handle_line(&banner_line)?;
        for line in &fc_ff {
            self.data.send_frame(line.as_bytes())?;
        }

        // Receive FS from IRS.
        let fs_frame = self.data.recv_frame()?;
        let fs_line = String::from_utf8_lossy(&fs_frame).into_owned();
        session.handle_line(&fs_line)?;

        // Send each accepted compressed blob.
        for blob in session.drain_pending_data() {
            self.data.send_frame(&blob)?;
        }

        self.cmd.send("DISCONNECT")?;
        self.cmd.wait_for("DISCONNECTED")?;
        Ok(())
    }

    /// IRS: listen for one incoming session and decode received messages.
    ///
    /// Blocks until a CONNECTED event arrives or `timeout` elapses.
    pub fn run_irs(
        &mut self,
        callsign: &str,
        timeout: Duration,
    ) -> Result<Vec<DecodedMessage>, DriverError> {
        self.cmd.send(&format!("MYID {callsign}"))?;
        self.cmd.wait_for("MYID")?;
        self.cmd.send("LISTEN TRUE")?;
        self.cmd.wait_for("LISTEN")?;

        // Wait for incoming connection (event arrives asynchronously).
        self.cmd.set_timeout(Some(timeout))?;
        self.cmd.wait_for("CONNECTED")?;
        self.cmd.set_timeout(None)?;

        // IRS sends its banner first.
        let my_banner = banner::encode(callsign);
        self.data.send_frame(my_banner.as_bytes())?;

        // Receive FC / FF lines; drive session until FS is sent.
        let mut session = B2fSession::new(SessionRole::Irs);
        loop {
            let frame = self.data.recv_frame()?;
            let line = String::from_utf8_lossy(&frame).into_owned();
            let responses = session.handle_line(&line)?;
            for resp in &responses {
                self.data.send_frame(resp.as_bytes())?;
            }
            // Once Transfer state entered (FS sent), stop reading control lines.
            if !responses.is_empty() || session.is_done() {
                break;
            }
        }

        // Receive one compressed blob per accepted proposal.
        let count = session.accepted_count();
        let mut decoded = Vec::with_capacity(count);
        for _ in 0..count {
            let blob = self.data.recv_frame()?;
            let body = session.receive_data(blob)?;
            decoded.push(DecodedMessage { body });
        }

        self.cmd.send("DISCONNECT")?;
        self.cmd.wait_for("DISCONNECTED")?;
        Ok(decoded)
    }
}
