use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::{PttController, PttError};

/// rigctld PTT controller — drives PTT via a hamlib rigctld TCP daemon.
///
/// Connects to `<host>:<port>` (typically `localhost:4532`) and issues
/// `T 1` / `T 0` commands to assert/release PTT.
pub struct RigctldPtt {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    asserted: bool,
}

const RIGCTLD_TIMEOUT: Duration = Duration::from_secs(5);

impl RigctldPtt {
    pub fn connect(addr: &str) -> Result<Self, PttError> {
        let stream = TcpStream::connect(addr).map_err(PttError::Io)?;
        stream
            .set_read_timeout(Some(RIGCTLD_TIMEOUT))
            .map_err(PttError::Io)?;
        stream
            .set_write_timeout(Some(RIGCTLD_TIMEOUT))
            .map_err(PttError::Io)?;
        let reader = BufReader::new(stream.try_clone().map_err(PttError::Io)?);
        Ok(Self {
            stream,
            reader,
            asserted: false,
        })
    }

    fn send_command(&mut self, cmd: &str) -> Result<(), PttError> {
        writeln!(self.stream, "{cmd}").map_err(PttError::Io)?;
        let mut response = String::new();
        self.reader.read_line(&mut response).map_err(PttError::Io)?;
        let trimmed = response.trim();
        if trimmed != "RPRT 0" {
            return Err(PttError::Rigctld(format!(
                "unexpected rigctld response: {trimmed}"
            )));
        }
        Ok(())
    }
}

impl PttController for RigctldPtt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        self.send_command("T 1")?;
        self.asserted = true;
        Ok(())
    }

    fn release_ptt(&mut self) -> Result<(), PttError> {
        self.send_command("T 0")?;
        self.asserted = false;
        Ok(())
    }

    fn is_asserted(&self) -> bool {
        self.asserted
    }
}
