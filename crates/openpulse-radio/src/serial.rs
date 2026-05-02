#[cfg(feature = "serial")]
use crate::PttController;
#[cfg(feature = "serial")]
use crate::PttError;

/// Serial RTS/DTR PTT controller. Requires the `serial` feature.
///
/// `pin` selects which serial control line drives PTT:
/// - `"rts"` — Request To Send
/// - `"dtr"` — Data Terminal Ready
#[cfg(feature = "serial")]
pub struct SerialRtsDtrPtt {
    port: Box<dyn serialport::SerialPort>,
    pin: SerialPin,
    asserted: bool,
}

#[cfg(feature = "serial")]
#[derive(Debug, Clone, Copy)]
pub enum SerialPin {
    Rts,
    Dtr,
}

#[cfg(feature = "serial")]
impl SerialRtsDtrPtt {
    pub fn open(path: &str, pin: SerialPin) -> Result<Self, PttError> {
        let port = serialport::new(path, 9600)
            .open()
            .map_err(|e| PttError::Serial(e.to_string()))?;
        Ok(Self {
            port,
            pin,
            asserted: false,
        })
    }
}

#[cfg(feature = "serial")]
impl PttController for SerialRtsDtrPtt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        match self.pin {
            SerialPin::Rts => self
                .port
                .write_request_to_send(true)
                .map_err(|e| PttError::Serial(e.to_string()))?,
            SerialPin::Dtr => self
                .port
                .write_data_terminal_ready(true)
                .map_err(|e| PttError::Serial(e.to_string()))?,
        }
        self.asserted = true;
        Ok(())
    }

    fn release_ptt(&mut self) -> Result<(), PttError> {
        match self.pin {
            SerialPin::Rts => self
                .port
                .write_request_to_send(false)
                .map_err(|e| PttError::Serial(e.to_string()))?,
            SerialPin::Dtr => self
                .port
                .write_data_terminal_ready(false)
                .map_err(|e| PttError::Serial(e.to_string()))?,
        }
        self.asserted = false;
        Ok(())
    }

    fn is_asserted(&self) -> bool {
        self.asserted
    }
}

// Stub so the module compiles without the feature.
#[cfg(not(feature = "serial"))]
pub struct SerialRtsDtrPtt {
    _priv: (),
}
