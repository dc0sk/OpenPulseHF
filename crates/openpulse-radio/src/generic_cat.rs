//! `GenericSerialCat` — TOML-scripted serial CAT backend.
//!
//! Production use requires the `generic-serial` feature (unix only).  Tests use
//! `MockTransport` which is always available.

use std::io;

use crate::cat_controller::CatController;
use crate::error::{PttError, RadioError};
use crate::rig_definition::{decode_response_value, expand_command, RigDefinition};
use crate::rig_mode::RigMode;
use crate::PttController;

/// Abstraction over the serial I/O channel; enables `MockTransport` in tests.
pub trait RigTransport: Send {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()>;
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()>;
}

// ── Production transport ───────────────────────────────────────────────────────

/// Wraps a real `serialport::SerialPort` as a `RigTransport`.
#[cfg(all(unix, feature = "generic-serial"))]
pub struct SerialTransport(pub Box<dyn serialport::SerialPort>);

#[cfg(all(unix, feature = "generic-serial"))]
impl RigTransport for SerialTransport {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.0.write_all(buf)
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        use std::io::Read;
        self.0.read_exact(buf)
    }
}

// ── Test transport ─────────────────────────────────────────────────────────────

/// In-memory transport for unit and integration tests.
pub struct MockTransport {
    /// Bytes written by the CAT controller (inspectable by tests).
    pub write_log: Vec<u8>,
    /// Bytes returned when the controller calls `read_exact`.
    pub read_data: io::Cursor<Vec<u8>>,
}

impl MockTransport {
    pub fn new(read_data: Vec<u8>) -> Self {
        Self {
            write_log: Vec::new(),
            read_data: io::Cursor::new(read_data),
        }
    }
}

impl RigTransport for MockTransport {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.write_log.extend_from_slice(buf);
        Ok(())
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        use std::io::Read;
        self.read_data.read_exact(buf)
    }
}

// ── GenericSerialCat ───────────────────────────────────────────────────────────

/// TOML-scripted CAT controller for rigs not supported by hamlib.
pub struct GenericSerialCat {
    transport: Box<dyn RigTransport>,
    def: RigDefinition,
    ptt_asserted: bool,
}

impl GenericSerialCat {
    /// Open a real serial port using settings from `rig_file`.
    ///
    /// Requires the `generic-serial` feature (unix only).
    #[cfg(all(unix, feature = "generic-serial"))]
    pub fn open(port: &str, rig_file: &str) -> Result<Self, RadioError> {
        use std::time::Duration;

        let src = std::fs::read_to_string(rig_file).map_err(|e| {
            RadioError::GenericCat(format!("cannot read rig file '{rig_file}': {e}"))
        })?;
        let def = RigDefinition::from_toml(&src)
            .map_err(|e| RadioError::GenericCat(format!("invalid rig file: {e}")))?;

        let data_bits = match def.rig.data_bits {
            5 => serialport::DataBits::Five,
            6 => serialport::DataBits::Six,
            7 => serialport::DataBits::Seven,
            _ => serialport::DataBits::Eight,
        };
        let stop_bits = match def.rig.stop_bits {
            2 => serialport::StopBits::Two,
            _ => serialport::StopBits::One,
        };
        let parity = match def.rig.parity.to_lowercase().as_str() {
            "odd" => serialport::Parity::Odd,
            "even" => serialport::Parity::Even,
            _ => serialport::Parity::None,
        };
        let serial = serialport::new(port, def.rig.baud)
            .data_bits(data_bits)
            .stop_bits(stop_bits)
            .parity(parity)
            .timeout(Duration::from_millis(500))
            .open()
            .map_err(|e| {
                RadioError::GenericCat(format!("cannot open serial port '{port}': {e}"))
            })?;

        Ok(Self {
            transport: Box::new(SerialTransport(serial)),
            def,
            ptt_asserted: false,
        })
    }

    /// Inject a custom transport; used in unit and integration tests.
    pub fn with_transport(transport: Box<dyn RigTransport>, def: RigDefinition) -> Self {
        Self {
            transport,
            def,
            ptt_asserted: false,
        }
    }

    /// Send a named command and return the response bytes (may be empty).
    ///
    /// Returns `ErrorKind::NotFound` when the command is absent from the rig
    /// definition so callers can map it to `RadioError::Unsupported`.
    fn send_named_command(&mut self, name: &str, freq_hz: Option<u64>) -> io::Result<Vec<u8>> {
        let cmd = self.def.commands.get(name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("command '{name}' not defined in rig file"),
            )
        })?;
        let bytes = expand_command(&cmd.send, &self.def.params, freq_hz)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let n = cmd.response_bytes;
        self.transport.write_all(&bytes)?;
        if n > 0 {
            let mut buf = vec![0u8; n];
            self.transport.read_exact(&mut buf)?;
            Ok(buf)
        } else {
            Ok(Vec::new())
        }
    }
}

// ── PttController ──────────────────────────────────────────────────────────────

impl PttController for GenericSerialCat {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        self.send_named_command("ptt_on", None)?;
        self.ptt_asserted = true;
        Ok(())
    }

    fn release_ptt(&mut self) -> Result<(), PttError> {
        self.send_named_command("ptt_off", None)?;
        self.ptt_asserted = false;
        Ok(())
    }

    fn is_asserted(&self) -> bool {
        self.ptt_asserted
    }
}

// ── CatController ──────────────────────────────────────────────────────────────

impl CatController for GenericSerialCat {
    fn set_frequency(&mut self, hz: u64) -> Result<(), RadioError> {
        self.send_named_command("set_frequency", Some(hz))
            .map(|_| ())
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    RadioError::Unsupported("set_frequency")
                } else {
                    RadioError::GenericCat(e.to_string())
                }
            })
    }

    fn get_frequency(&mut self) -> Result<u64, RadioError> {
        // Clone the extract descriptor before taking &mut self for the send.
        let extract = self
            .def
            .commands
            .get("get_frequency")
            .ok_or(RadioError::Unsupported("get_frequency"))?
            .response_extract
            .clone()
            .ok_or_else(|| {
                RadioError::GenericCat("get_frequency command has no response_extract".into())
            })?;

        let buf = self
            .send_named_command("get_frequency", None)
            .map_err(|e| RadioError::GenericCat(e.to_string()))?;

        let end = extract.offset + extract.length;
        if buf.len() < end {
            return Err(RadioError::GenericCat(format!(
                "response too short: need {} bytes at offset {}, got {}",
                extract.length,
                extract.offset,
                buf.len()
            )));
        }
        decode_response_value(&buf[extract.offset..end], &extract.encoding)
            .map_err(RadioError::GenericCat)
    }

    fn set_mode(&mut self, mode: &RigMode) -> Result<(), RadioError> {
        let cmd_name = format!("set_mode_{}", mode.as_str().to_lowercase());
        if !self.def.commands.contains_key(cmd_name.as_str()) {
            return Err(RadioError::Unsupported("set_mode"));
        }
        self.send_named_command(&cmd_name, None)
            .map(|_| ())
            .map_err(|e| RadioError::GenericCat(e.to_string()))
    }
}
